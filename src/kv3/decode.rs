use super::value::Document;
use super::{binary, legacy};
use crate::budget::Budget;
use crate::error::{Result, malformed};

/// `VKV\x03`: the first binary KV3 header, with an encoding GUID.
pub(crate) const MAGIC_LEGACY: u32 = 0x0356_4B56;
/// `\x0n3VK` for versions 1..=5.
const MAGIC_VERSIONED: u32 = 0x4B56_3300;

/// Node types of binary KV3.
pub(crate) mod node {
    pub const NULL: u8 = 1;
    pub const BOOLEAN: u8 = 2;
    pub const INT64: u8 = 3;
    pub const UINT64: u8 = 4;
    pub const DOUBLE: u8 = 5;
    pub const STRING: u8 = 6;
    pub const BINARY_BLOB: u8 = 7;
    pub const ARRAY: u8 = 8;
    pub const OBJECT: u8 = 9;
    pub const ARRAY_TYPED: u8 = 10;
    pub const INT32: u8 = 11;
    pub const UINT32: u8 = 12;
    pub const BOOLEAN_TRUE: u8 = 13;
    pub const BOOLEAN_FALSE: u8 = 14;
    pub const INT64_ZERO: u8 = 15;
    pub const INT64_ONE: u8 = 16;
    pub const DOUBLE_ZERO: u8 = 17;
    pub const DOUBLE_ONE: u8 = 18;
    pub const FLOAT: u8 = 19;
    pub const INT16: u8 = 20;
    pub const UINT16: u8 = 21;
    pub const INT8: u8 = 22;
    pub const UINT8: u8 = 23;
    pub const ARRAY_TYPE_BYTE_LENGTH: u8 = 24;
    pub const ARRAY_TYPE_AUXILIARY_BUFFER: u8 = 25;
    pub const UINT32_ZERO: u8 = 26;
    pub const UINT32_ONE: u8 = 27;
}

/// Largest buffer zstd data of unknown decoded size may claim; real ones are a few MB at most.
const MAX_BUFFER: usize = 256 << 20;
/// LZ4 expands a byte at most 255 times.
pub(crate) const LZ4_RATIO: usize = 255;
/// A zstd block regenerates 128 KiB at most and takes 4 bytes at least (its header and one byte
/// repeated).
const ZSTD_RATIO: usize = (128 << 10) / 4;
/// A zstd frame's magic number, and that of a skippable frame (its low four bits any).
const ZSTD_MAGIC: u32 = 0xFD2F_B528;
const ZSTD_SKIPPABLE: u32 = 0x184D_2A50;
/// What a zstd decoder stages past its window: it decodes a mebibyte at a time, and a block of
/// 128 KiB at most past that, before the output takes it (ruzstd 0.9.1
/// `FrameDecoder::decode_all`, `BlockDecodingStrategy::UptoBytes(1024 * 1024)`).
const ZSTD_STAGING: usize = (1 << 20) + (128 << 10);
/// The ring buffer's slack: it grows to `next_power_of_two(required - SLACK) + SLACK + 1`
/// (ruzstd 0.9.1 `decoding/ringbuffer.rs`, `reserve_amortized`, `SLACK = 2 * MAX_BLOCK_SIZE`).
const ZSTD_SLACK: usize = 2 * (128 << 10);
/// The decoder's tables and block buffers besides its ring buffer (Huffman and FSE tables,
/// literals and sequences of one block): a fraction of this.
const ZSTD_SCRATCH: usize = 1 << 20;

/// The most a zstd decoder of `window` allocates at once: its ring buffer holds the window
/// and the staging, `need` bytes; each growth rounds the capacity up to below twice `need`
/// plus the slack, and allocates the new buffer before it frees the old one, so two such
/// buffers are live at once; and its scratch.
fn zstd_worst(window: usize) -> usize {
    let need = window.saturating_add(ZSTD_STAGING);
    let capacity = need.saturating_mul(2).saturating_add(ZSTD_SLACK + 1);
    capacity.saturating_mul(2).saturating_add(ZSTD_SCRATCH)
}

/// Whether `bytes` start with a binary KV3 signature this crate reads.
pub fn is_kv3(bytes: &[u8]) -> bool {
    let Some(&magic) = bytes.first_chunk::<4>() else {
        return false;
    };
    let magic = u32::from_le_bytes(magic);
    magic == MAGIC_LEGACY
        || (magic & 0xFFFF_FF00 == MAGIC_VERSIONED && (1..=5).contains(&(magic & 0xFF)))
}

/// Decodes a binary KV3 document (a resource block, or a whole file) within the default
/// budget of a decoded document.
pub fn decode(bytes: &[u8]) -> Result<Document> {
    decode_in(bytes, &mut Budget::named("KV3"))
}

/// Decodes a binary KV3 document within `budget`: every buffer, string table and value is
/// charged to it before it is allocated.
pub fn decode_in(bytes: &[u8], budget: &mut Budget) -> Result<Document> {
    if !is_kv3(bytes) {
        return Err(malformed("KV3", "no KV3 signature"));
    }
    if bytes[..4] == MAGIC_LEGACY.to_le_bytes() {
        legacy::decode(bytes, budget)
    } else {
        binary::decode(bytes, budget)
    }
}

/// A zeroed buffer of `len` bytes, charged to `budget` before it is allocated.
pub(crate) fn buffer(len: usize, budget: &mut Budget) -> Result<Vec<u8>> {
    budget.bytes(len)?;
    Ok(vec![0; len])
}

/// One LZ4 block that decompresses to exactly `len` bytes; a length the input cannot reach is
/// refused before anything is allocated.
pub(crate) fn lz4(input: &[u8], len: usize, budget: &mut Budget) -> Result<Vec<u8>> {
    if len / LZ4_RATIO > input.len() {
        return Err(malformed(
            "KV3 LZ4",
            format!("{} bytes cannot expand to {len}", input.len()),
        ));
    }
    let mut out = buffer(len, budget)?;
    match lz4_flex::block::decompress_into(input, &mut out) {
        Ok(n) if n == out.len() => Ok(out),
        Ok(n) => Err(malformed(
            "KV3 LZ4",
            format!("{n} bytes, expected {}", out.len()),
        )),
        Err(e) => Err(malformed("KV3 LZ4", e.to_string())),
    }
}

/// Zstd frames that decompress to exactly `len` bytes; a length the input cannot reach is
/// refused before anything is allocated.
pub(crate) fn zstd(input: &[u8], len: usize, budget: &mut Budget) -> Result<Vec<u8>> {
    if len / ZSTD_RATIO > input.len() {
        return Err(malformed(
            "KV3 zstd",
            format!("{} bytes cannot expand to {len}", input.len()),
        ));
    }
    let mut out = buffer(len, budget)?;
    match zstd_into(input, &mut out, budget)? {
        Ok(n) if n == out.len() => Ok(out),
        Ok(n) => Err(malformed(
            "KV3 zstd",
            format!("{n} bytes, expected {}", out.len()),
        )),
        Err(e) => Err(malformed("KV3 zstd", e.to_string())),
    }
}

/// Decompresses zstd frames of unknown decoded size, at most `max` bytes, and no more than the
/// input can expand to: that much is allocated up front.
pub(crate) fn zstd_to_vec(input: &[u8], max: usize) -> Result<Vec<u8>> {
    let len = max.min(input.len().saturating_mul(ZSTD_RATIO));
    // The buffer and a decoder of a window as large.
    let mut budget = Budget::with_limit("zstd", MAX_BUFFER + zstd_worst(MAX_BUFFER));
    let mut out = buffer(len, &mut budget)?;
    let len =
        zstd_into(input, &mut out, &mut budget)?.map_err(|e| malformed("zstd", e.to_string()))?;
    out.truncate(len);
    Ok(out)
}

/// Decodes zstd frames into `out`. The decoder keeps a frame's window and stages past it
/// ([`ZSTD_STAGING`]): the most it allocates for the widest window the frames declare
/// ([`zstd_worst`]) is charged to `budget` before the decoder is made, and that window is the
/// most it may keep.
fn zstd_into(
    input: &[u8],
    out: &mut [u8],
    budget: &mut Budget,
) -> Result<std::result::Result<usize, ruzstd::decoding::errors::FrameDecoderError>> {
    let window = zstd_window(input)?;
    budget.bytes(zstd_worst(usize::try_from(window).unwrap_or(usize::MAX)))?;
    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    decoder.set_max_window_size(window);
    Ok(decoder.decode_all(input, out))
}

/// The widest window the zstd frames of `input` declare: each frame's window descriptor, or a
/// single-segment frame's content size (RFC 8878 3.1.1). The frames are walked by their block
/// headers, nothing decoded; skippable frames are passed over.
fn zstd_window(mut input: &[u8]) -> Result<u64> {
    let short = || malformed("zstd", "a frame cut short");
    let mut widest = 0u64;
    while !input.is_empty() {
        let magic = input
            .first_chunk::<4>()
            .map(|magic| u32::from_le_bytes(*magic))
            .ok_or_else(short)?;
        if magic & 0xFFFF_FFF0 == ZSTD_SKIPPABLE {
            let len = input.get(4..8).ok_or_else(short)?;
            let len = u32::from_le_bytes(len.try_into().expect("four bytes")) as usize;
            input = input.get(8usize.saturating_add(len)..).ok_or_else(short)?;
            continue;
        }
        if magic != ZSTD_MAGIC {
            return Err(malformed("zstd", "not a zstd frame"));
        }
        let descriptor = *input.get(4).ok_or_else(short)?;
        let single = descriptor & 0x20 != 0;
        let dictionary = [0, 1, 2, 4][usize::from(descriptor & 3)];
        let size_len = match descriptor >> 6 {
            0 => usize::from(single),
            flag => [0, 2, 4, 8][usize::from(flag)],
        };
        let mut at = 5;
        let window = if single {
            let size = input
                .get(at + dictionary..at + dictionary + size_len)
                .ok_or_else(short)?;
            let size = size
                .iter()
                .rev()
                .fold(0u64, |size, &byte| size << 8 | u64::from(byte));
            if size_len == 2 { size + 256 } else { size }
        } else {
            let descriptor = *input.get(at).ok_or_else(short)?;
            at += 1;
            let base = 1u64 << (10 + (descriptor >> 3));
            base + base / 8 * u64::from(descriptor & 7)
        };
        widest = widest.max(window);
        at += dictionary + size_len;
        loop {
            let header = input.get(at..at + 3).ok_or_else(short)?;
            let header =
                u32::from(header[0]) | u32::from(header[1]) << 8 | u32::from(header[2]) << 16;
            let content = match (header >> 1) & 3 {
                1 => 1,
                3 => return Err(malformed("zstd", "a reserved block type")),
                _ => (header >> 3) as usize,
            };
            at += 3 + content;
            if header & 1 != 0 {
                break;
            }
        }
        if descriptor & 0x04 != 0 {
            at += 4;
        }
        input = input.get(at..).ok_or_else(short)?;
    }
    Ok(widest)
}

/// A string-table entry, charged to `budget`; ids out of range (-1 is the empty string) read
/// as empty.
pub(crate) fn string(strings: &[String], id: i32, budget: &mut Budget) -> Result<String> {
    let text = usize::try_from(id).ok().and_then(|i| strings.get(i));
    budget.copy(text.map_or("", String::as_str))
}
