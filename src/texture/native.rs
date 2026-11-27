//! Typed Source 2 texture headers and native mip images.

use super::convert::Content;
use super::format::{PixelFormat, Stored, VTexFormat};
use super::ktx2::Encoding;
use super::rgba::{copy_blocks, decode_blocks, linear_to_srgb, srgb_to_linear, unorm8};
use super::sheet::VTexSheet;
use super::vtex::{Header, WHAT, rgba8_size};
use super::{bc6, eac};
use crate::{Budget, Resource, Result, error::malformed};

/// The raw Source 2 texture flags, including any unrecognized bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureFlags(pub u16);

impl TextureFlags {
    /// The resource is a cubemap.
    pub fn is_cube(self) -> bool {
        self.0 & 0x10 != 0
    }

    /// Mip levels can shrink along the depth axis.
    pub fn is_volume(self) -> bool {
        self.0 & 0x20 != 0
    }

    /// Mip levels retain the same array layer count.
    pub fn is_array(self) -> bool {
        self.0 & 0x40 != 0
    }

    /// A single 2D image per mip.
    pub fn is_2d(self) -> bool {
        self.0 & 0x70 == 0
    }

    /// The compiler requested a linear API texture.
    pub fn linear_api_texture(self) -> bool {
        self.0 & 0x200 != 0
    }

    /// Number of cubemap faces per layer, or one for other textures.
    pub fn face_count(self) -> u32 {
        if self.is_cube() { 6 } else { 1 }
    }
}

/// Authored metadata of a compiled Source 2 texture.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TextureMeta {
    /// Stored width of mip zero in texels.
    pub width: u32,
    /// Stored height of mip zero in texels.
    pub height: u32,
    /// Header depth or array layer count, independent of the number of cube faces.
    pub depth: u32,
    /// Number of authored mip levels.
    pub mip_count: u8,
    /// The Source 2 format of the file, prior to decoding PNG, swapping BGRA, etc.
    pub format: VTexFormat,
    /// Raw header flags.
    pub flags: TextureFlags,
    /// Valid display rectangle dimensions, when present in the metadata extra.
    pub display_rect: Option<[u32; 2]>,
    /// The sprite sheet, including each frame's full set of UV rectangles.
    pub sheet: Option<VTexSheet>,
}

impl TextureMeta {
    /// Reads the header and extras without copying or decoding the mip payloads.
    /// Formats outside the decoder's support still have readable metadata.
    pub fn read(bytes: &[u8]) -> Result<Self> {
        Self::from_resource_in(&Resource::parse(bytes)?, &mut Budget::named(WHAT))
    }

    /// Reads metadata from an already parsed resource, charging sheet allocation to `budget`.
    pub fn from_resource_in(resource: &Resource<'_>, budget: &mut Budget) -> Result<Self> {
        Ok(Header::from_resource(resource, budget)?.meta)
    }

    /// Known linear interpretation, or `None` when the renderer needs the material's sampler
    /// or its own interpretation. Absence of the linear API flag does not imply sRGB.
    pub fn encoding_hint(&self) -> Option<Encoding> {
        use VTexFormat as V;
        (self.flags.linear_api_texture()
            || matches!(
                self.format,
                V::R16
                    | V::Rg1616
                    | V::Rgba16161616
                    | V::R16f
                    | V::Rg1616f
                    | V::Rgba16161616f
                    | V::R32f
                    | V::Rg3232f
                    | V::Rgb323232f
                    | V::Rgba32323232f
                    | V::Bc6h
                    | V::Ati1n
                    | V::Ati2n
                    | V::R11Eac
                    | V::Rg11Eac
                    | V::A8
                    | V::R8Unorm
                    | V::R32Uint
            ))
        .then_some(Encoding::Linear)
    }

    pub(super) fn mip_dimensions(&self, level: u8) -> [u32; 3] {
        let size = |side: u32| (side >> level).max(1);
        let depth = if self.flags.is_volume() {
            size(self.depth)
        } else {
            self.depth.max(1)
        };
        [size(self.width), size(self.height), depth]
    }
}

/// The original stream of a Source 2 texture wrapped around an ordinary image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EmbeddedImage<'a> {
    /// The complete PNG stream.
    Png(&'a [u8]),
    /// The complete JPEG stream, for example for browser image decoding.
    Jpeg(&'a [u8]),
    /// The complete WebP stream.
    Webp(&'a [u8]),
}

/// Borrows an embedded PNG, JPEG or WebP stream. A native mip format returns None.
pub fn embedded_image(bytes: &[u8]) -> Result<Option<EmbeddedImage<'_>>> {
    let resource = Resource::parse(bytes)?;
    let meta = TextureMeta::from_resource_in(&resource, &mut Budget::named(WHAT))?;
    let data = resource.trailing();
    use VTexFormat as V;
    let (image, valid) = match meta.format {
        V::PngRgba8888 | V::PngDxt5 => (
            EmbeddedImage::Png(data),
            data.starts_with(b"\x89PNG\r\n\x1a\n"),
        ),
        V::JpegRgba8888 | V::JpegDxt5 => (EmbeddedImage::Jpeg(data), data.starts_with(b"\xff\xd8")),
        V::WebpRgba8888 | V::WebpDxt5 => (
            EmbeddedImage::Webp(data),
            data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"),
        ),
        _ => return Ok(None),
    };
    if !valid {
        return Err(malformed(WHAT, "invalid embedded image signature"));
    }
    Ok(Some(image))
}

/// A compiled texture, with every mip at its native precision. BC/ETC/EAC blocks are retained,
/// PNG wrappers are expanded to RGBA8 or RGBA16, and BGRA is reordered to RGBA.
#[derive(Clone, Debug)]
pub struct VTex {
    meta: TextureMeta,
    format: PixelFormat,
    content: Content,
    levels: Vec<Vec<u8>>,
}

impl VTex {
    /// Reads a compiled texture, including cube faces, volume slices and array layers.
    /// JPEG and WebP streams are available through [`embedded_image`].
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::from_resource_in(&Resource::parse(bytes)?, &mut Budget::named(WHAT))
    }

    /// Reads a texture from an already parsed resource with bounded allocations.
    pub fn from_resource_in(resource: &Resource<'_>, budget: &mut Budget) -> Result<Self> {
        let header = Header::from_resource(resource, budget)?;
        let stored = Stored::from_format(header.meta.format)?;
        let (format, levels) = header.load_levels(resource.trailing(), stored, true, budget)?;
        let content = match stored {
            Stored::Plain(PixelFormat::Bc3) => Content::of(resource, budget)?,
            _ => Content::Colour,
        };
        Ok(Self {
            meta: header.meta,
            format,
            content,
            levels,
        })
    }

    /// The metadata from the Source 2 header and extras.
    pub fn meta(&self) -> &TextureMeta {
        &self.meta
    }

    /// The representation of the mip byte data. See the Source 2 format for special channels,
    /// such as alpha in an A8 resource or intensity and alpha in IA88.
    pub fn pixel_format(&self) -> PixelFormat {
        self.format
    }

    /// Borrows authored mip `level`, where zero is the largest.
    pub fn mip(&self, level: u8) -> Option<TextureMip<'_>> {
        Some(TextureMip {
            texture: self,
            data: self.levels.get(usize::from(level))?,
            dimensions: self.meta.mip_dimensions(level),
        })
    }
}

/// Controls the RGBA8 fallback's transfer function and two-channel normal decoding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba8Options {
    /// Override the source encoding using the material's texture role. Default uses known
    /// texture metadata when a conversion is requested.
    pub source_encoding: Option<Encoding>,
    /// Desired encoding. `None` preserves the input transfer function.
    pub output_encoding: Option<Encoding>,
    /// Synthesize positive tangent-space Z from BC5 red and green. Keep this off for masks
    /// and other two-channel data.
    pub reconstruct_normal_z: bool,
}

/// An authored mip level as native bytes, with all images in file order.
#[derive(Clone, Copy, Debug)]
pub struct TextureMip<'a> {
    texture: &'a VTex,
    data: &'a [u8],
    dimensions: [u32; 3],
}

impl<'a> TextureMip<'a> {
    /// Width, height, and the number of volume slices or array layers at this level.
    pub fn dimensions(&self) -> [u32; 3] {
        self.dimensions
    }

    /// All images, layer major and face minor. Useful as a single 3D or array upload.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// One image's native bytes. For 2D textures use `(0, 0)`. Cubemap face indices are
    /// +X, −X, +Y, −Y, +Z, −Z in source order.
    pub fn image(&self, layer: u32, face: u32) -> Option<&'a [u8]> {
        let [width, height, depth] = self.dimensions;
        let faces = self.texture.meta.flags.face_count();
        if layer >= depth || face >= faces {
            return None;
        }
        let bytes = self.texture.format.level_size(width, height)?;
        let start = (layer as usize * faces as usize + face as usize).checked_mul(bytes)?;
        self.data.get(start..start.checked_add(bytes)?)
    }

    /// Decodes an image to tightly packed RGBA8 rows. Numeric channels outside [0, 1] clip.
    /// Float, 16-bit and HDR data lose precision here. Alpha is unaffected by gamma.
    /// Use the linear float decoder or native bytes to retain HDR data.
    pub fn decode_rgba8(&self, layer: u32, face: u32, options: Rgba8Options) -> Result<Vec<u8>> {
        let data = self.image_or_error(layer, face)?;
        let [w, h, _] = self.dimensions;
        let size = rgba8_size(w, h)?;
        let transfer = self.transfer(options.source_encoding, options.output_encoding)?;
        let format = self.texture.format;
        if let Some(decode) = float_blocks(format) {
            let mut pixels = vec![0; size];
            copy_blocks(
                data,
                (w, h),
                format.block().2,
                pixels.as_chunks_mut::<4>().0,
                decode,
                |[r, g, b]| [transfer(r), transfer(g), transfer(b), 1.0].map(unorm8),
            );
            return Ok(pixels);
        }
        if format == PixelFormat::Rgba8 || format.block().0 > 1 {
            let lookup: [u8; 256] = std::array::from_fn(|i| unorm8(transfer(i as f32 / 255.0)));
            if format == PixelFormat::Rgba8 {
                let mut pixels = data.to_vec();
                for pixel in pixels.as_chunks_mut::<4>().0 {
                    for channel in &mut pixel[..3] {
                        *channel = lookup[*channel as usize];
                    }
                }
                return Ok(pixels);
            }
            let convert = |pixel: [u8; 4]| {
                let [r, g, mut b, a] = self.texture.content.unpack(pixel);
                if options.reconstruct_normal_z && format == PixelFormat::Bc5 {
                    let axis = |v| 2.0 * (f32::from(v) / 255.0) - 1.0;
                    let (x, y) = (axis(r), axis(g));
                    b = unorm8(0.5 + 0.5 * (1.0 - x * x - y * y).max(0.0).sqrt());
                }
                [
                    lookup[r as usize],
                    lookup[g as usize],
                    lookup[b as usize],
                    a,
                ]
            };
            return Ok(decode_blocks(format, data, w, h, convert));
        }
        let convert =
            |[r, g, b, a]: [f32; 4]| [transfer(r), transfer(g), transfer(b), a].map(unorm8);
        Ok(self.plain_pixels(data).flat_map(convert).collect())
    }

    /// Decodes plain, unsigned BC6H or EAC texels to linear floats. Half-floats expand exactly and
    /// HDR values are preserved. Supply the material's encoding for ambiguous headers.
    pub fn decode_linear_rgba32f(
        &self,
        layer: u32,
        face: u32,
        source_encoding: Option<Encoding>,
    ) -> Result<Vec<[f32; 4]>> {
        let data = self.image_or_error(layer, face)?;
        let [w, h, _] = self.dimensions;
        if PixelFormat::Rgba32f
            .level_size(w, h)
            .is_none_or(|size| size > super::vtex::MAX_BYTES)
        {
            return Err(malformed(WHAT, "linear float image is over the size cap"));
        }
        let transfer = self.transfer(source_encoding, Some(Encoding::Linear))?;
        let format = self.texture.format;
        if let Some(decode) = float_blocks(format) {
            let mut pixels = vec![[0.0; 4]; w as usize * h as usize];
            copy_blocks(
                data,
                (w, h),
                format.block().2,
                &mut pixels,
                decode,
                |[r, g, b]| [transfer(r), transfer(g), transfer(b), 1.0],
            );
            return Ok(pixels);
        }
        if format.block().0 > 1 {
            return Err(malformed(
                WHAT,
                "linear float decoding requires plain texels, BC6H or EAC",
            ));
        }
        Ok(self
            .plain_pixels(data)
            .map(|[r, g, b, a]| [transfer(r), transfer(g), transfer(b), a])
            .collect())
    }

    fn image_or_error(&self, layer: u32, face: u32) -> Result<&'a [u8]> {
        self.image(layer, face)
            .ok_or_else(|| malformed(WHAT, format!("no image at layer {layer}, face {face}")))
    }

    fn transfer(
        &self,
        source: Option<Encoding>,
        output: Option<Encoding>,
    ) -> Result<fn(f32) -> f32> {
        let Some(output) = output else {
            return Ok(|v| v);
        };
        let source = source.or_else(|| {
            if self.texture.content == Content::Normal {
                Some(Encoding::Linear)
            } else {
                self.texture.meta.encoding_hint()
            }
        });
        match (source, output) {
            (None, _) => Err(malformed(WHAT, "texture source encoding is unspecified")),
            (Some(Encoding::Srgb), Encoding::Linear) => Ok(srgb_to_linear),
            (Some(Encoding::Linear), Encoding::Srgb) => Ok(linear_to_srgb),
            _ => Ok(|v| v),
        }
    }

    fn plain_pixels(&self, data: &'a [u8]) -> impl Iterator<Item = [f32; 4]> {
        let format = self.texture.format;
        let source = self.texture.meta.format;
        data.chunks_exact(format.block().2)
            .map(move |pixel| plain_pixel(format, source, pixel))
    }
}

type FloatBlockDecoder = fn(&[u8]) -> [[f32; 3]; 16];

fn float_blocks(format: PixelFormat) -> Option<FloatBlockDecoder> {
    Some(match format {
        PixelFormat::Bc6h => |data| bc6::block(data).map(|rgb| rgb.map(half)),
        PixelFormat::EacR11 => eac::red,
        PixelFormat::EacRg11 => eac::red_green,
        _ => return None,
    })
}

fn plain_pixel(format: PixelFormat, source: VTexFormat, bytes: &[u8]) -> [f32; 4] {
    use PixelFormat as P;
    let byte = |i| f32::from(bytes[i]) / 255.0;
    match source {
        VTexFormat::I8 => return [byte(0), byte(0), byte(0), 1.0],
        VTexFormat::Ia88 => return [byte(0), byte(0), byte(0), byte(1)],
        VTexFormat::A8 => return [0.0, 0.0, 0.0, byte(0)],
        _ => {}
    }
    let word = |i: usize| u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]);
    let wide = |i: usize| f32::from_le_bytes(std::array::from_fn(|j| bytes[4 * i + j]));
    let (channels, sample): (usize, &dyn Fn(usize) -> f32) = match format {
        P::R8 => (1, &byte),
        P::Rg8 => (2, &byte),
        P::Rgba8 => (4, &byte),
        P::R16 => (1, &|i| f32::from(word(i)) / 65535.0),
        P::Rg16 => (2, &|i| f32::from(word(i)) / 65535.0),
        P::Rgba16 => (4, &|i| f32::from(word(i)) / 65535.0),
        P::R16f => (1, &|i| half(word(i))),
        P::Rg16f => (2, &|i| half(word(i))),
        P::Rgba16f => (4, &|i| half(word(i))),
        P::R32f => (1, &wide),
        P::Rg32f => (2, &wide),
        P::Rgba32f => (4, &wide),
        _ => unreachable!("plain format"),
    };
    let mut pixel = [0.0, 0.0, 0.0, 1.0];
    for (i, channel) in pixel[..channels].iter_mut().enumerate() {
        *channel = sample(i);
    }
    pixel
}

fn half(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 31);
    let mantissa = u32::from(bits & 1023);
    match exponent {
        0 if mantissa == 0 => f32::from_bits(sign),
        0 => f32::from_bits(sign | (103 << 23)) * mantissa as f32,
        31 => f32::from_bits(sign | 0x7F80_0000 | mantissa << 13),
        _ => f32::from_bits(sign | (exponent + 112) << 23 | mantissa << 13),
    }
}
