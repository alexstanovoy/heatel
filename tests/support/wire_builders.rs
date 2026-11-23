//! Shared synthetic binary wire builders for Source 2 `Resource`, `VTEX`, `NTRO`, and PNG
//! fixtures used across integration tests.
#![allow(dead_code)]

/// Packs `u32` words as little-endian bytes.
pub fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Packs `f32` values as little-endian bytes.
pub fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Builds a compiled Source 2 resource from `(kind, payload)` blocks; each block's offset counts
/// from its own field.
pub fn resource(blocks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_end = 16 + 12 * blocks.len();
    let size = table_end + blocks.iter().map(|(_, b)| b.len()).sum::<usize>();
    let mut out = (size as u32).to_le_bytes().to_vec();
    out.extend(12u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(8u32.to_le_bytes());
    out.extend((blocks.len() as u32).to_le_bytes());
    let mut at = table_end;
    for (kind, block) in blocks {
        let field = out.len() + 4;
        out.extend(*kind);
        out.extend(((at - field) as u32).to_le_bytes());
        out.extend((block.len() as u32).to_le_bytes());
        at += block.len();
    }
    for (_, block) in blocks {
        out.extend(block);
    }
    out
}

/// A synthetic `vtex_c` resource builder.
pub struct Vtex {
    pub flags: u16,
    pub width: u16,
    pub height: u16,
    pub format: u8,
    pub mips: u8,
    /// Extra data entries: `(type, payload)`.
    pub extras: Vec<(u32, Vec<u8>)>,
    /// Mip levels after the resource, smallest first.
    pub levels: Vec<Vec<u8>>,
    /// Optional `REDI` block bytes.
    pub redi: Option<Vec<u8>>,
}

impl Vtex {
    pub fn new(width: u16, height: u16, format: u8, levels: Vec<Vec<u8>>) -> Self {
        Self {
            flags: 0,
            width,
            height,
            format,
            mips: levels.len() as u8,
            extras: Vec::new(),
            levels,
            redi: None,
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&self.flags.to_le_bytes());
        data.extend_from_slice(&[0; 16]);
        for v in [self.width, self.height, 1] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(&[self.format, self.mips]);
        data.extend_from_slice(&words(&[0, 8, self.extras.len() as u32]));
        let mut payload_at = data.len() + 12 * self.extras.len();
        let mut payloads = Vec::new();
        for (kind, payload) in &self.extras {
            let field = data.len() + 4;
            data.extend_from_slice(&words(&[
                *kind,
                (payload_at - field) as u32,
                payload.len() as u32,
            ]));
            payload_at += payload.len();
            payloads.extend_from_slice(payload);
        }
        data.extend_from_slice(&payloads);
        let mut blocks = vec![(b"DATA", data)];
        if let Some(redi) = &self.redi {
            blocks.push((b"REDI", redi.clone()));
        }
        let mut out = resource(&blocks);
        for level in &self.levels {
            out.extend_from_slice(level);
        }
        out
    }
}

/// Shorthand for a single-mip `vtex_c` with `extras` and trailing `raw` bytes.
pub fn vtex_bytes(
    width: u16,
    height: u16,
    format: u8,
    extras: &[(u32, Vec<u8>)],
    raw: &[u8],
) -> Vec<u8> {
    let mut v = Vtex::new(width, height, format, vec![raw.to_vec()]);
    v.extras = extras.to_vec();
    v.bytes()
}

/// Builds a VTEX sprite-sheet extra-data block with a single frame `rect`.
pub fn sheet_extra(rect: [f32; 4]) -> Vec<u8> {
    let mut out = words(&[8, 1]);
    out.extend_from_slice(&words(&[0, 0, 24, 1, 1.0f32.to_bits(), 0, 0, 0]));
    out.extend_from_slice(&words(&[1.0f32.to_bits(), 8, 1]));
    for v in rect.into_iter().chain([0.0, 0.0, 1.0, 1.0]) {
        out.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    out
}

/// Builds a minimal valid PNG chunk (`len || kind || data || crc32`).
pub fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut h = crc32fast::Hasher::new();
    h.update(kind);
    h.update(data);
    out.extend_from_slice(&h.finalize().to_be_bytes());
    out
}

/// Wraps raw uncompressed bytes in a valid zlib stream using stored deflate blocks.
pub fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = if raw.is_empty() {
        vec![&[]]
    } else {
        raw.chunks(65535).collect()
    };
    for (idx, chunk) in chunks.iter().enumerate() {
        let last = u8::from(idx + 1 == chunks.len());
        out.push(last);
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    let (mut s1, mut s2) = (1u32, 0u32);
    for &b in raw {
        s1 = (s1 + u32::from(b)) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    out.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
    out
}

/// Builds a minimal PNG of `w x h` with `color_type` (0=Gray, 2=RGB, 4=GrayAlpha, 6=RGBA) and
/// optional extra chunks before `IDAT`.
pub fn minimal_png(w: u32, h: u32, color_type: u8, rows: &[&[u8]], extra: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]);
    out.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    for chunk in extra {
        out.extend_from_slice(chunk);
    }
    let mut raw = Vec::new();
    for row in rows {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    out.extend_from_slice(&png_chunk(b"IDAT", &zlib_stored(&raw)));
    out.extend_from_slice(&png_chunk(b"IEND", &[]));
    out
}

/// Builds a VPK v2 header followed by `tree` and trailing `in_dir` data.
pub fn vpk_v2(tree: &[u8], in_dir: &[u8]) -> Vec<u8> {
    let mut out = words(&[
        0x55AA_1234,
        2,
        tree.len() as u32,
        in_dir.len() as u32,
        0,
        0,
        0,
    ]);
    out.extend_from_slice(tree);
    out.extend_from_slice(in_dir);
    out
}

/// Builds a single-file VPK v1 package storing `files` (`(path, bytes)`) in its data section,
/// returning the package bytes and a map of each file's byte offset to its path.
pub fn vpk_v1<'a>(
    files: &[(&'a str, Vec<u8>)],
) -> (Vec<u8>, std::collections::HashMap<u64, &'a str>) {
    let (mut tree, mut data, mut starts) = (Vec::new(), Vec::new(), Vec::new());
    for (path, bytes) in files {
        let (dir, file) = path.rsplit_once('/').unwrap_or((" ", path));
        let (name, ext) = file.rsplit_once('.').unwrap_or((file, " "));
        for part in [ext, dir, name] {
            tree.extend_from_slice(part.as_bytes());
            tree.push(0);
        }
        tree.extend_from_slice(&crc32fast::hash(bytes).to_le_bytes());
        tree.extend_from_slice(&[0, 0, 0xFF, 0x7F]);
        tree.extend_from_slice(&(data.len() as u32).to_le_bytes());
        tree.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        tree.extend_from_slice(&[0xFF, 0xFF, 0, 0]);
        starts.push((data.len() as u64, *path));
        data.extend_from_slice(bytes);
    }
    tree.push(0);
    let mut out = words(&[0x55AA_1234, 1, tree.len() as u32]);
    let data_start = (12 + tree.len()) as u64;
    out.extend_from_slice(&tree);
    out.extend_from_slice(&data);
    (
        out,
        starts
            .into_iter()
            .map(|(at, p)| (data_start + at, p))
            .collect(),
    )
}
