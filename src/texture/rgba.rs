//! Textures as plain RGBA8 pixels: decoding any level to them and writing them as a PNG.

use super::format::PixelFormat;
use crate::error::{Result, malformed};

const WHAT: &str = "texture";

/// RGBA8 decoding used by the existing 2D export API.
pub(super) fn decode(
    format: PixelFormat,
    level: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    match format {
        PixelFormat::Rgba8 => Ok(level.to_vec()),
        PixelFormat::Bc1
        | PixelFormat::Bc3
        | PixelFormat::Bc4
        | PixelFormat::Bc5
        | PixelFormat::Bc7 => Ok(decode_blocks(format, level, width, height, |pixel| pixel)),
        other => Err(malformed(WHAT, format!("no RGBA8 decoding of {other:?}"))),
    }
}

/// Decode a block format with only one block of scratch. All callers supply one validated image.
pub(super) fn decode_blocks(
    format: PixelFormat,
    level: &[u8],
    width: u32,
    height: u32,
    convert: impl Fn([u8; 4]) -> [u8; 4],
) -> Vec<u8> {
    let mut out = vec![0; width as usize * height as usize * 4];
    copy_blocks(
        level,
        (width, height),
        format.block().2,
        out.as_chunks_mut::<4>().0,
        |data| block_pixels(format, data),
        convert,
    );
    out
}

pub(super) fn copy_blocks<P: Copy, T>(
    level: &[u8],
    (width, height): (u32, u32),
    block_size: usize,
    out: &mut [T],
    decode: impl Fn(&[u8]) -> [P; 16],
    convert: impl Fn(P) -> T,
) {
    let (w, h) = (width as usize, height as usize);
    let blocks_x = w.div_ceil(4);
    for (index, data) in level.chunks_exact(block_size).enumerate() {
        let (bx, by) = (index % blocks_x * 4, index / blocks_x * 4);
        let block = decode(data);
        for (y, row) in block.as_chunks::<4>().0.iter().take(h - by).enumerate() {
            let at = (by + y) * w + bx;
            for (target, &pixel) in out[at..at + (w - bx).min(4)].iter_mut().zip(row) {
                *target = convert(pixel);
            }
        }
    }
}

fn block_pixels(format: PixelFormat, data: &[u8]) -> [[u8; 4]; 16] {
    let mut packed = [0; 16];
    match format {
        PixelFormat::Bc1 => return bc_colour(data, false),
        PixelFormat::Bc3 => {
            let mut pixels = bc_colour(&data[8..], true);
            for (pixel, alpha) in pixels.iter_mut().zip(channel_values(&data[..8])) {
                pixel[3] = alpha;
            }
            return pixels;
        }
        PixelFormat::Bc4 => return channel_values(data).map(|r| [r, 0, 0, 255]),
        PixelFormat::Bc5 => {
            let r = channel_values(&data[..8]);
            let g = channel_values(&data[8..]);
            return std::array::from_fn(|i| [r[i], g[i], 0, 255]);
        }
        PixelFormat::Etc2Rgb => texture2ddecoder::decode_etc2_rgb_block(data, &mut packed),
        PixelFormat::Etc2Rgba => {
            texture2ddecoder::decode_etc2_rgb_block(&data[8..], &mut packed);
            for (pixel, alpha) in packed.iter_mut().zip(super::eac::alpha(&data[..8])) {
                *pixel = (*pixel & 0x00FF_FFFF) | u32::from(alpha) << 24;
            }
        }
        PixelFormat::Bc7 => texture2ddecoder::decode_bc7_block(data, &mut packed),
        _ => unreachable!("8-bit block format"),
    }
    packed.map(|p| {
        let [b, g, r, a] = p.to_le_bytes();
        [r, g, b, a]
    })
}

/// DXT5 always uses the four-color palette, even when the endpoints are in ascending order.
fn bc_colour(data: &[u8], four_colors: bool) -> [[u8; 4]; 16] {
    let a = u16::from_le_bytes([data[0], data[1]]);
    let b = u16::from_le_bytes([data[2], data[3]]);
    let expand = |n: u16| -> [u16; 4] {
        let (r, g, b) = (n >> 11, (n >> 5) & 63, n & 31);
        [r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2, 255]
    };
    let (first, second) = (expand(a), expand(b));
    let mix =
        |x, y, denominator| std::array::from_fn(|i| (x * first[i] + y * second[i]) / denominator);
    let (third, fourth) = if four_colors || a > b {
        (mix(2, 1, 3), mix(1, 2, 3))
    } else {
        (mix(1, 1, 2), [0; 4])
    };
    let palette = [first, second, third, fourth].map(|pixel| pixel.map(|v| v as u8));
    let indices = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    std::array::from_fn(|i| palette[((indices >> (2 * i)) & 3) as usize])
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
                    unorm8(linear_to_srgb(
                        texels
                            .map(|v| srgb_to_linear(f32::from(v) / 255.0))
                            .sum::<f32>()
                            / 4.0,
                    ))
                } else {
                    (texels.map(u32::from).sum::<u32>() as f32 / 4.0).round() as u8
                };
            }
        }
    }
    out
}

pub(super) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub(super) fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

pub(super) fn unorm8(c: f32) -> u8 {
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}
