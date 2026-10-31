//! Textures as plain RGBA8 pixels: decoding any level to them and writing them as a PNG.

use super::format::PixelFormat;
use crate::error::{Result, malformed};

const WHAT: &str = "texture";

/// The RGBA8 pixels of a level `width` x `height` in `format`: plain RGBA8 as it is, the colour
/// block formats (BC1, BC3, BC4, BC5, BC7) decoded. Other formats are errors.
pub(super) fn decode(
    format: PixelFormat,
    level: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    let decoder = match format {
        PixelFormat::Rgba8 => return Ok(level.to_vec()),
        PixelFormat::Bc1 => texture2ddecoder::decode_bc1,
        PixelFormat::Bc3 => texture2ddecoder::decode_bc3,
        PixelFormat::Bc4 => texture2ddecoder::decode_bc4,
        PixelFormat::Bc5 => texture2ddecoder::decode_bc5,
        PixelFormat::Bc7 => texture2ddecoder::decode_bc7,
        other => return Err(malformed(WHAT, format!("no RGBA8 decoding of {other:?}"))),
    };
    let px = decode_blocks(format, decoder, level, (w, h))?;
    Ok(px
        .iter()
        .flat_map(|p| {
            let [b, g, r, a] = p.to_le_bytes();
            [r, g, b, a]
        })
        .collect())
}

/// texture2ddecoder's block decoders: blocks, width, height, packed pixels out.
pub(super) type BlockDecoder =
    fn(&[u8], usize, usize, &mut [u32]) -> std::result::Result<(), &'static str>;

/// The pixels of a block-compressed level as the decoder makes them, packed as it packs them (a
/// byte each of blue, green, red and alpha from the lowest), with the channels that `fix`
/// rewrites made whole: texture2ddecoder reads the 48 bits of indices of a BC3 alpha or a BC4 or
/// BC5 channel into a `usize`, which keeps 32 of them on a 32-bit target (the browser's wasm), so
/// the last five texels of every block were drawn as the block's first value there.
pub(super) fn decode_blocks(
    format: PixelFormat,
    decoder: BlockDecoder,
    level: &[u8],
    (w, h): (usize, usize),
) -> Result<Vec<u32>> {
    let mut px = vec![0u32; w * h];
    decoder(level, w, h, &mut px)
        .map_err(|e| malformed(WHAT, format!("{format:?} blocks: {e}")))?;
    // (offset of the channel's 8 bytes in a block, the channel's place in the packed pixel)
    let channels: &[(usize, u32)] = match format {
        PixelFormat::Bc3 => &[(0, 3)],
        PixelFormat::Bc4 => &[(0, 2)],
        PixelFormat::Bc5 => &[(0, 2), (8, 1)],
        _ => &[],
    };
    let (bytes, blocks_x) = (format.block().2, w.div_ceil(4));
    for (block, data) in level.chunks_exact(bytes).enumerate() {
        let (bx, by) = (block % blocks_x * 4, block / blocks_x * 4);
        for &(at, channel) in channels {
            for (i, value) in channel_values(&data[at..at + 8]).into_iter().enumerate() {
                let (x, y) = (bx + i % 4, by + i / 4);
                if x < w && y < h {
                    let p = &mut px[y * w + x];
                    *p = *p & !(0xFF << (channel * 8)) | u32::from(value) << (channel * 8);
                }
            }
        }
    }
    Ok(px)
}

/// The 16 values of a block's 8-byte channel (the alpha of BC3, BC4's red, each of BC5's two), in
/// reading order: two end values and sixteen 3-bit steps between them.
fn channel_values(block: &[u8]) -> [u8; 16] {
    let (a, b) = (u16::from(block[0]), u16::from(block[1]));
    let mut steps = [a, b, 0, 0, 0, 0, 0, 255];
    if a > b {
        for (i, step) in steps.iter_mut().enumerate().skip(2) {
            *step = ((8 - i as u16) * a + (i as u16 - 1) * b) / 7;
        }
    } else {
        for (i, step) in steps.iter_mut().enumerate().take(6).skip(2) {
            *step = ((6 - i as u16) * a + (i as u16 - 1) * b) / 5;
        }
        steps[6] = 0;
    }
    let mut bits = u64::from_le_bytes([
        block[2], block[3], block[4], block[5], block[6], block[7], 0, 0,
    ]);
    std::array::from_fn(|_| {
        let value = steps[(bits & 7) as usize] as u8;
        bits >>= 3;
        value
    })
}

/// A PNG file of RGBA8 `pixels`.
pub(super) fn png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let err = |e: png::EncodingError| malformed(WHAT, format!("png: {e}"));
    let mut writer = encoder.write_header().map_err(err)?;
    writer.write_image_data(pixels).map_err(err)?;
    writer.finish().map_err(err)?;
    Ok(out)
}

/// RGBA8 `pixels` of `width` x `height` halved with a 2 x 2 box (a side of 1 stays 1); with
/// `srgb` the colour channels are averaged as light, alpha as it is.
pub(super) fn halve(pixels: &[u8], width: u32, height: u32, srgb: bool) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (hw, hh) = ((w / 2).max(1), (h / 2).max(1));
    let to_light = |v: u8| {
        let c = f32::from(v) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let from_light = |l: f32| {
        let c = if l <= 0.003_130_8 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (c * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let mut out = vec![0u8; hw * hh * 4];
    for y in 0..hh {
        for x in 0..hw {
            let rows = [(2 * y).min(h - 1), (2 * y + 1).min(h - 1)];
            let cols = [(2 * x).min(w - 1), (2 * x + 1).min(w - 1)];
            for c in 0..4 {
                let texels = rows
                    .iter()
                    .flat_map(|r| cols.iter().map(move |col| pixels[(r * w + col) * 4 + c]));
                out[(y * hw + x) * 4 + c] = if srgb && c < 3 {
                    from_light(texels.map(to_light).sum::<f32>() / 4.0)
                } else {
                    (texels.map(u32::from).sum::<u32>() as f32 / 4.0).round() as u8
                };
            }
        }
    }
    out
}
