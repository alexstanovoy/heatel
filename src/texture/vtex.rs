use std::io::Cursor as IoCursor;

use super::convert::Content;
use super::format::{PixelFormat, Stored, VTexFormat};
use super::ktx2::{self, Encoding};
use super::native::{TextureFlags, TextureMeta};
use super::rgba;
use super::sheet::{self, Sheet};
use crate::budget::Budget;
use crate::bytes::Cursor;
use crate::error::{Result, malformed};
use crate::resource::Resource;

pub(super) const WHAT: &str = "vtex";
/// Largest side of a texture; the game's biggest is 4096.
const MAX_SIDE: u32 = 16384;
/// Most memory the decoded levels of one texture may take.
pub(super) const MAX_BYTES: usize = 256 << 20;
/// Most extra data entries a header may list (the game's textures have up to three).
const MAX_EXTRAS: u32 = 16;
/// LZ4 expands a byte at most 255 times, so a level cannot be larger than this per stored byte.
const LZ4_RATIO: usize = 255;

const EXTRA_METADATA: u32 = 3;
const EXTRA_SHEET: u32 = 2;
const EXTRA_COMPRESSED_MIPS: u32 = 4;

/// A 2D texture of a `vtex_c`: its mip levels in the pixel format they are stored in (block
/// formats stay compressed).
#[derive(Clone, Debug)]
pub struct Texture {
    width: u32,
    height: u32,
    format: PixelFormat,
    /// Largest level first; level `i` is `max(1, size >> i)` texels square.
    mips: Vec<Vec<u8>>,
    /// The file's sprite sheet, if it has one.
    sheet: Option<Sheet>,
}

impl Texture {
    /// Reads the texture of a `vtex_c` file: its header, the levels after the resource's own
    /// blocks (the smallest first in the file), LZ4-compressed levels expanded. Cube maps,
    /// volumes and arrays are errors, so are JPEG-coded textures.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::parse_in(bytes, &mut Budget::named(WHAT))
    }

    /// Like [`parse`](Self::parse), charging decoded level and PNG buffers to `budget` before
    /// allocating.
    pub fn parse_in(bytes: &[u8], budget: &mut Budget) -> Result<Self> {
        Self::from_resource_in(&Resource::parse(bytes)?, budget)
    }

    /// Reads the texture from an already-parsed `vtex_c` [`Resource`], charging decoded level and
    /// PNG buffers to `budget` before allocating.
    pub fn from_resource_in(resource: &Resource<'_>, budget: &mut Budget) -> Result<Self> {
        let header = Header::from_resource(resource, budget)?;
        let meta = &header.meta;
        if !meta.flags.is_2d() || meta.depth > 1 {
            return Err(malformed(WHAT, "a cube map, volume or array"));
        }
        let stored = Stored::from_format(meta.format)?;
        let (format, mips) = header.load_levels(resource.trailing(), stored, false, budget)?;
        let mut texture = Self {
            width: meta.width,
            height: meta.height,
            format,
            mips,
            sheet: header
                .meta
                .sheet
                .map(|sheet| sheet.into_legacy(budget))
                .transpose()?,
        };
        if matches!(stored, Stored::Plain(PixelFormat::Bc3)) {
            texture.decode_content(Content::of(resource, budget)?, budget)?;
        }
        Ok(texture)
    }

    /// Decodes DXT5 levels that hold a normal or a YCoCg image to RGBA8.
    fn decode_content(&mut self, content: Content, budget: &mut Budget) -> Result<()> {
        if content == Content::Colour {
            return Ok(());
        }
        // DXT5 takes a byte a texel, RGBA8 four: a texture within the cap as stored may not be.
        let mut total = 0usize;
        for level in 0..self.mips.len() {
            let (w, h) = ((self.width >> level).max(1), (self.height >> level).max(1));
            total = total.saturating_add(rgba8_size(w, h)?);
        }
        if total > MAX_BYTES {
            return Err(malformed(
                WHAT,
                format!("decodes to more than {MAX_BYTES} bytes"),
            ));
        }
        budget.charge(total)?;
        for (level, mip) in self.mips.iter_mut().enumerate() {
            let (w, h) = ((self.width >> level).max(1), (self.height >> level).max(1));
            *mip = content.decode_bc3(mip, w, h);
        }
        self.format = PixelFormat::Rgba8;
        Ok(())
    }

    /// Width of the largest level, in texels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height of the largest level, in texels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// How the texels are stored.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// The mip levels, largest first.
    pub fn mips(&self) -> &[Vec<u8>] {
        &self.mips
    }

    /// Whether the file is a sprite sheet.
    pub fn is_sheet(&self) -> bool {
        self.rect().is_some()
    }

    /// The file's sprite sheet: its sequences and their frames, if it has one.
    pub fn sheet(&self) -> Option<&Sheet> {
        self.sheet.as_ref()
    }

    /// The first frame of the sprite sheet, `[u0, v0, u1, v1]` in 0..1, if it has one.
    fn rect(&self) -> Option<[f32; 4]> {
        self.sheet.as_ref()?.first_rect()
    }

    /// Where the first frame sits in a sprite sheet: the texels `[x0, y0, x1, y1]` of the first
    /// sequence's first frame cropped to its content, widened to whole texels. Source2Viewer
    /// exports exactly these.
    pub fn first_frame_rect(&self) -> Option<[u32; 4]> {
        let rect = self.rect()?;
        let low = |u: f32, side: u32| (u.clamp(0.0, 1.0) * side as f32).floor() as u32;
        let high = |u: f32, side: u32| (u.clamp(0.0, 1.0) * side as f32).ceil() as u32;
        Some([
            low(rect[0], self.width),
            low(rect[1], self.height),
            high(rect[2], self.width),
            high(rect[3], self.height),
        ])
    }

    /// The first frame of a sprite sheet ([`first_frame_rect`](Self::first_frame_rect)), cut out
    /// of every level it lines up with. Block formats stay compressed, so the cut starts at the
    /// block border before the frame (up to three texels more on the left and top); levels the
    /// frame does not line up with are dropped. A texture that is no sheet comes back whole.
    pub fn first_frame(&self) -> Self {
        let Some([fx0, fy0, x1, y1]) = self.first_frame_rect() else {
            return self.clone();
        };
        let (bw, bh, _) = self.format.block();
        let (x0, y0) = (fx0 / bw * bw, fy0 / bh * bh);
        if x1 <= fx0 || y1 <= fy0 {
            return Self {
                sheet: None,
                ..self.clone()
            };
        }
        let mut mips = Vec::new();
        for (level, mip) in self.mips.iter().enumerate() {
            let mask = (1u32 << level) - 1;
            let aligned = [x0, y0, x1, y1].iter().all(|v| v & mask == 0);
            let (lx0, ly0) = (x0 >> level, y0 >> level);
            let (lx1, ly1) = (x1 >> level, y1 >> level);
            if !aligned || lx0 % bw != 0 || ly0 % bh != 0 || lx1 == lx0 || ly1 == ly0 {
                break;
            }
            let side = (self.width >> level).max(1);
            mips.push(self.cut(mip, side, [lx0, ly0, lx1, ly1]));
        }
        Self {
            width: x1 - x0,
            height: y1 - y0,
            format: self.format,
            mips,
            sheet: None,
        }
    }

    /// The blocks of `rect` (`[x0, y0, x1, y1]` texels, `x0` and `y0` on block borders) out of a
    /// level `side` texels wide.
    fn cut(&self, level: &[u8], side: u32, rect: [u32; 4]) -> Vec<u8> {
        let (bw, bh, bytes) = self.format.block();
        let stride = side.div_ceil(bw) as usize * bytes;
        let (from, to) = ((rect[0] / bw) as usize, rect[2].div_ceil(bw) as usize);
        let rows = rect[1] / bh..rect[3].div_ceil(bh);
        let mut out = Vec::with_capacity(rows.len() * (to - from) * bytes);
        for row in rows {
            let start = row as usize * stride;
            out.extend_from_slice(&level[start + from * bytes..start + to * bytes]);
        }
        out
    }

    /// A texture of RGBA8 `pixels` (rows from the top, 4 bytes each) and a single level, for
    /// images made from others: it writes as KTX2 or PNG like a decoded one.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        let size = PixelFormat::Rgba8.level_size(width, height);
        if width == 0
            || height == 0
            || width > MAX_SIDE
            || height > MAX_SIDE
            || size != Some(pixels.len())
        {
            return Err(malformed(
                WHAT,
                format!(
                    "{} bytes are not {width} x {height} RGBA8 pixels",
                    pixels.len()
                ),
            ));
        }
        Ok(Self {
            width,
            height,
            format: PixelFormat::Rgba8,
            mips: vec![pixels],
            sheet: None,
        })
    }

    /// A texture of RGBA8 mip `levels` (largest first, `max(1, size >> i)` texels a side), for
    /// images made level by level from others.
    pub fn from_rgba8_levels(width: u32, height: u32, levels: Vec<Vec<u8>>) -> Result<Self> {
        let mut levels = levels.into_iter();
        let mut texture = Self::from_rgba8(width, height, levels.next().unwrap_or_default())?;
        if levels.len() >= (32 - width.max(height).leading_zeros()) as usize {
            return Err(malformed(WHAT, "too many RGBA8 mip levels"));
        }
        for (level, pixels) in (1..).zip(levels) {
            let (w, h) = ((width >> level).max(1), (height >> level).max(1));
            if PixelFormat::Rgba8.level_size(w, h) != Some(pixels.len()) {
                return Err(malformed(
                    WHAT,
                    format!("level {level} is not {w} x {h} RGBA8"),
                ));
            }
            texture.mips.push(pixels);
        }
        Ok(texture)
    }

    /// Every level as RGBA8 pixels (see [`to_rgba8`](Self::to_rgba8)), largest first.
    pub fn levels_rgba8(&self) -> Result<Vec<Vec<u8>>> {
        rgba8_size(self.width, self.height)?;
        self.mips
            .iter()
            .enumerate()
            .map(|(level, mip)| {
                let (w, h) = ((self.width >> level).max(1), (self.height >> level).max(1));
                rgba::decode(self.format, mip, w, h)
            })
            .collect()
    }

    /// Whether a GPU samples the texture as it is stored: a block format of the desktop's (BC1
    /// to BC7) whose largest level is whole blocks, or plain channels.
    pub fn gpu_ready(&self) -> bool {
        let (bw, bh, _) = self.format.block();
        let blocks_whole = self.width.is_multiple_of(bw) && self.height.is_multiple_of(bh);
        match self.format {
            PixelFormat::Etc2Rgb
            | PixelFormat::Etc2Rgba
            | PixelFormat::EacR11
            | PixelFormat::EacRg11 => false,
            _ => blocks_whole,
        }
    }

    /// The texture with every mip level down to 1 x 1: an RGBA8 texture of a single level (a PNG
    /// the game stores, or an image made from others) gets the rest by halving it with a 2 x 2
    /// box, sRGB colours averaged as light. Others come back as they are.
    pub fn with_mips(mut self, encoding: Encoding) -> Self {
        if self.format != PixelFormat::Rgba8 || self.mips.len() != 1 {
            return self;
        }
        let (mut w, mut h) = (self.width, self.height);
        while w > 1 || h > 1 {
            let half = rgba::halve(
                &self.mips[self.mips.len() - 1],
                w,
                h,
                encoding == Encoding::Srgb,
            );
            (w, h) = ((w / 2).max(1), (h / 2).max(1));
            self.mips.push(half);
        }
        self
    }

    /// An RGBA8 texture's levels block-compressed to `format` (BC1, BC3, BC4 or BC5): images made
    /// from Dota's textures (packed channels, decoded normals) reach the GPU compressed as the
    /// textures Dota stores do. Colours are fitted by their perceived brightness with
    /// `Encoding::Srgb`, every channel alike otherwise. Other textures and formats are errors.
    #[cfg(feature = "encode")]
    pub fn compressed(&self, format: PixelFormat, encoding: Encoding) -> Result<Self> {
        let target = match format {
            PixelFormat::Bc1 => texpresso::Format::Bc1,
            PixelFormat::Bc3 => texpresso::Format::Bc3,
            PixelFormat::Bc4 => texpresso::Format::Bc4,
            PixelFormat::Bc5 => texpresso::Format::Bc5,
            other => return Err(malformed(WHAT, format!("no compression to {other:?}"))),
        };
        if self.format != PixelFormat::Rgba8 {
            return Err(malformed(
                WHAT,
                format!("compressing {:?} texels", self.format),
            ));
        }
        let params = texpresso::Params {
            weights: match encoding {
                Encoding::Srgb => texpresso::COLOUR_WEIGHTS_PERCEPTUAL,
                Encoding::Linear => texpresso::COLOUR_WEIGHTS_UNIFORM,
            },
            ..texpresso::Params::default()
        };
        let mips = self
            .mips
            .iter()
            .enumerate()
            .map(|(level, pixels)| {
                let (w, h) = ((self.width >> level).max(1), (self.height >> level).max(1));
                let mut out = vec![0; target.compressed_size(w as usize, h as usize)];
                target.compress(pixels, w as usize, h as usize, params, &mut out);
                out
            })
            .collect();
        Ok(Self {
            format,
            mips,
            ..self.clone()
        })
    }

    /// A DXT5 texture's colour as BC1, its alpha dropped, block for block and exactly: a DXT5
    /// colour block always holds four colours, which BC1 reads only with its first endpoint the
    /// greater, so a block in the other order has its endpoints and indices swapped, and one of
    /// a single colour points every texel at it.
    pub fn bc3_colour_as_bc1(&self) -> Result<Self> {
        if self.format != PixelFormat::Bc3 {
            return Err(malformed(WHAT, format!("{:?} is not DXT5", self.format)));
        }
        let mips = self
            .mips
            .iter()
            .map(|level| {
                level
                    .as_chunks::<16>()
                    .0
                    .iter()
                    .map(dxt5_colour_as_bc1)
                    .collect::<Vec<_>>()
                    .concat()
            })
            .collect();
        Ok(Self {
            format: PixelFormat::Bc1,
            mips,
            ..self.clone()
        })
    }

    /// The largest level as RGBA8 pixels: plain RGBA8 as it is, BC1, BC3, BC4, BC5 and BC7
    /// decoded; other formats are errors.
    pub fn to_rgba8(&self) -> Result<Vec<u8>> {
        rgba8_size(self.width, self.height)?;
        rgba::decode(self.format, &self.mips[0], self.width, self.height)
    }

    /// A PNG file of the largest level (see [`to_rgba8`](Self::to_rgba8)).
    pub fn to_png(&self) -> Result<Vec<u8>> {
        rgba::png(self.width, self.height, &self.to_rgba8()?)
    }

    /// A PNG file of the first frame of a sprite sheet cut exactly to its
    /// [`first_frame_rect`](Self::first_frame_rect), as Source2Viewer exports it (the whole
    /// texture when it is no sheet).
    pub fn first_frame_png(&self) -> Result<Vec<u8>> {
        let Some([fx0, fy0, x1, y1]) = self.first_frame_rect() else {
            return self.to_png();
        };
        if x1 <= fx0 || y1 <= fy0 {
            return self.to_png();
        }
        let frame = self.first_frame();
        let pixels = frame.to_rgba8()?;
        // The frame is cut on block borders: up to three texels more on the left and top.
        let (bw, bh, _) = self.format.block();
        let (dx, dy) = ((fx0 % bw) as usize, (fy0 % bh) as usize);
        let (w, h) = ((x1 - fx0) as usize, (y1 - fy0) as usize);
        let stride = frame.width as usize * 4;
        let mut out = Vec::with_capacity(w * h * 4);
        for row in pixels.chunks_exact(stride).skip(dy).take(h) {
            out.extend_from_slice(&row[dx * 4..(dx + w) * 4]);
        }
        rgba::png(w as u32, h as u32, &out)
    }

    /// The KTX2 file of this texture. Block formats are written as they are; `encoding` says
    /// whether the colours are sRGB (see [`Encoding`]).
    pub fn to_ktx2(&self, encoding: Encoding) -> Vec<u8> {
        ktx2::write(self.width, self.height, self.format, encoding, &self.mips)
    }
}

/// The mip sizes a header lists as stored, and whether they are LZ4-compressed.
struct Compressed {
    lz4: bool,
    sizes: Vec<u32>,
}

struct MipSize {
    unpacked: usize,
    content: usize,
}

pub(super) struct Header {
    pub(super) meta: TextureMeta,
    compressed: Option<Compressed>,
}

impl Header {
    pub(super) fn from_resource(resource: &Resource<'_>, budget: &mut Budget) -> Result<Self> {
        let data = resource
            .block(b"DATA")
            .ok_or_else(|| malformed(WHAT, "no DATA block"))?;
        Self::parse(data, budget)
    }

    fn parse(data: &[u8], budget: &mut Budget) -> Result<Self> {
        let mut c = Cursor::new(data, WHAT);
        let version = c.u16()?;
        if version != 1 {
            return Err(malformed(WHAT, format!("version {version}, expected 1")));
        }
        let flags = TextureFlags(c.u16()?);
        c.take(16)?; // reflectivity
        let (width, height, depth) = (
            u32::from(c.u16()?),
            u32::from(c.u16()?),
            u32::from(c.u16()?),
        );
        let (format, mip_count) = (VTexFormat::from_id(c.u8()?), c.u8()?);
        c.u32()?;
        let extras_at = c.pos() as u64;
        let extras_offset = c.u32()?;
        let extras = c.u32()?;
        let longest = width
            .max(height)
            .max(if flags.is_volume() { depth } else { 1 });
        if width == 0
            || height == 0
            || depth > MAX_SIDE
            || longest > MAX_SIDE
            || mip_count == 0
            || u32::from(mip_count) > 32 - longest.leading_zeros()
        {
            return Err(malformed(
                WHAT,
                format!("{mip_count} levels of {width} x {height} x {depth} texels"),
            ));
        }
        let mut header = Self {
            meta: TextureMeta {
                width,
                height,
                depth,
                mip_count,
                format,
                flags,
                display_rect: None,
                sheet: None,
            },
            compressed: None,
        };
        header.read_extras(data, extras_at + u64::from(extras_offset), extras, budget)?;
        Ok(header)
    }

    pub(super) fn load_levels(
        &self,
        raw: &[u8],
        stored: Stored,
        retain_png_depth: bool,
        budget: &mut Budget,
    ) -> Result<(PixelFormat, Vec<Vec<u8>>)> {
        if matches!(stored, Stored::Png) {
            let meta = &self.meta;
            if !meta.flags.is_2d() || meta.depth > 1 || meta.mip_count != 1 {
                return Err(malformed(WHAT, "PNG must have one image and mip"));
            }
            let (format, pixels) =
                decode_png(raw, (meta.width, meta.height), retain_png_depth, budget)?;
            return Ok((format, vec![pixels]));
        }
        let format = stored.decoded();
        let sizes = self.level_sizes(format)?;
        let mut levels = self.read_levels(raw, &sizes, budget)?;
        if matches!(stored, Stored::Bgra8) {
            for px in levels
                .iter_mut()
                .flat_map(|mip| mip.as_chunks_mut::<4>().0.iter_mut())
            {
                px.swap(0, 2);
            }
        }
        Ok((format, levels))
    }

    /// Reads the list of `count` extra data entries at `at`: (type, offset from the offset field,
    /// size).
    fn read_extras(&mut self, data: &[u8], at: u64, count: u32, budget: &mut Budget) -> Result<()> {
        if count > MAX_EXTRAS {
            return Err(malformed(WHAT, format!("{count} extra data entries")));
        }
        let start = usize::try_from(at).map_err(|_| malformed(WHAT, "extra data offset"))?;
        let mut list = Cursor::new(data.get(start..).unwrap_or_default(), WHAT);
        for _ in 0..count {
            let (kind, pos) = (list.u32()?, start + list.pos());
            let (offset, size) = (list.u32()? as usize, list.u32()? as usize);
            let payload = pos
                .checked_add(offset)
                .and_then(|from| data.get(from..from.checked_add(size)?))
                .ok_or_else(|| malformed(WHAT, "extra data runs past the end"))?;
            match kind {
                EXTRA_SHEET => self.meta.sheet = sheet::read(payload, budget)?,
                EXTRA_METADATA => {
                    let mut metadata = Cursor::new(payload, WHAT);
                    metadata.u16()?;
                    let [w, h] = [u32::from(metadata.u16()?), u32::from(metadata.u16()?)];
                    if w > 0 && w <= self.meta.width && h > 0 && h <= self.meta.height {
                        self.meta.display_rect = Some([w, h]);
                    }
                }
                EXTRA_COMPRESSED_MIPS => self.compressed = Some(self.compressed_mips(payload)?),
                _ => {}
            }
        }
        Ok(())
    }

    /// The payload of the compressed mip sizes entry: whether they are compressed, the offset of
    /// the sizes (always 8), their count and the sizes, one per level.
    fn compressed_mips(&self, payload: &[u8]) -> Result<Compressed> {
        let mut c = Cursor::new(payload, WHAT);
        let lz4 = match c.u32()? {
            0 => false,
            1 => true,
            other => return Err(malformed(WHAT, format!("compressed mips flag {other}"))),
        };
        let offset = c.u32()?;
        let count = c.u32()? as usize;
        if offset != 8 || count < usize::from(self.meta.mip_count) || c.remaining() / 4 < count {
            return Err(malformed(WHAT, "compressed mip sizes"));
        }
        let sizes = (0..self.meta.mip_count)
            .map(|_| c.u32())
            .collect::<Result<_>>()?;
        Ok(Compressed { lz4, sizes })
    }

    /// Unpacked sizes, including Source 2's minimum of four images for multi-slice BC mips.
    fn level_sizes(&self, format: PixelFormat) -> Result<Vec<MipSize>> {
        let over_cap = || malformed(WHAT, format!("decodes to more than {MAX_BYTES} bytes"));
        let mut total = 0usize;
        (0..self.meta.mip_count)
            .map(|level| {
                let [w, h, depth] = self.meta.mip_dimensions(level);
                let images = depth * self.meta.flags.face_count();
                let padded = if format.block().0 > 1 && (2..4).contains(&images) {
                    4
                } else {
                    images
                };
                let image = format.level_size(w, h).ok_or_else(over_cap)?;
                let unpacked = image.checked_mul(padded as usize).ok_or_else(over_cap)?;
                total = total
                    .checked_add(unpacked)
                    .filter(|&size| size <= MAX_BYTES)
                    .ok_or_else(over_cap)?;
                Ok(MipSize {
                    unpacked,
                    content: image * images as usize,
                })
            })
            .collect()
    }

    /// The levels out of the bytes after the resource, which hold the smallest level first. Each
    /// is stored as listed in the compressed sizes entry, if any: LZ4 where that is smaller than
    /// the level itself.
    fn read_levels(
        &self,
        raw: &[u8],
        sizes: &[MipSize],
        budget: &mut Budget,
    ) -> Result<Vec<Vec<u8>>> {
        budget.charge(sizes.iter().map(|size| size.unpacked).sum())?;
        let mut levels = vec![Vec::new(); sizes.len()];
        let mut at = 0usize;
        for level in (0..sizes.len()).rev() {
            let MipSize {
                unpacked: size,
                content,
            } = sizes[level];
            let len = self
                .compressed
                .as_ref()
                .map_or(size, |entry| entry.sizes[level] as usize);
            let part = at
                .checked_add(len)
                .and_then(|end| raw.get(at..end))
                .ok_or_else(|| malformed(WHAT, "level data runs past the end"))?;
            at += len;
            levels[level] = if len >= size {
                part[..content].to_vec()
            } else if self.compressed.as_ref().is_some_and(|c| c.lz4) && size / LZ4_RATIO <= len {
                let mut out = vec![0; size];
                let decoded = lz4_flex::block::decompress_into(part, &mut out)
                    .map_err(|e| malformed(WHAT, format!("level {level}: {e}")))?;
                if decoded != size {
                    return Err(malformed(
                        WHAT,
                        format!("level {level} expands to {decoded} of {size} bytes"),
                    ));
                }
                out.truncate(content);
                out
            } else {
                return Err(malformed(
                    WHAT,
                    format!("level {level} has {len} of its {size} bytes"),
                ));
            };
        }
        Ok(levels)
    }
}

/// The bytes of a level `width` x `height` as RGBA8, refused past [`MAX_BYTES`].
pub(super) fn rgba8_size(width: u32, height: u32) -> Result<usize> {
    PixelFormat::Rgba8
        .level_size(width, height)
        .filter(|&size| size <= MAX_BYTES)
        .ok_or_else(|| malformed(WHAT, format!("{width} x {height} RGBA8 is over the cap")))
}

/// The pixels of a PNG-coded level, which must be the texture's size.
fn decode_png(
    png: &[u8],
    (width, height): (u32, u32),
    retain_depth: bool,
    budget: &mut Budget,
) -> Result<(PixelFormat, Vec<u8>)> {
    let png_err = |e: png::DecodingError| malformed(WHAT, format!("png: {e}"));
    let limits = png::Limits {
        bytes: budget.left().min(MAX_BYTES),
    };
    let mut decoder = png::Decoder::new_with_limits(IoCursor::new(png), limits);
    decoder.set_transformations(if retain_depth {
        png::Transformations::EXPAND
    } else {
        png::Transformations::normalize_to_color8()
    });
    let mut reader = decoder.read_info().map_err(png_err)?;
    let info = reader.info();
    if (info.width, info.height) != (width, height) {
        return Err(malformed(
            WHAT,
            format!(
                "png of {} x {} in a {width} x {height} texture",
                info.width, info.height
            ),
        ));
    }
    let wide = reader.output_color_type().1 == png::BitDepth::Sixteen;
    let format = if wide {
        PixelFormat::Rgba16
    } else {
        PixelFormat::Rgba8
    };
    let alloc = format
        .level_size(width, height)
        .filter(|&size| size <= MAX_BYTES)
        .ok_or_else(|| {
            malformed(
                WHAT,
                format!("{width} x {height} PNG output is over the cap"),
            )
        })?;
    let size = reader
        .output_buffer_size()
        .filter(|&size| size <= alloc)
        .ok_or_else(|| malformed(WHAT, "png buffer size"))?;
    budget.charge(alloc)?;
    let mut pixels = vec![0; alloc];
    let frame = reader.next_frame(&mut pixels[..size]).map_err(png_err)?;
    let (channels, decoded) = (frame.color_type.samples(), frame.buffer_size());
    if frame.color_type == png::ColorType::Indexed || decoded != alloc / 4 * channels {
        return Err(malformed(WHAT, "png pixels do not fill the texture"));
    }
    if wide {
        let samples = pixels.as_chunks_mut::<2>().0;
        for sample in &mut samples[..decoded / 2] {
            sample.swap(0, 1);
        }
        expand_png(samples, channels);
    } else {
        expand_png(pixels.as_chunks_mut::<1>().0, channels);
    }
    Ok((format, pixels))
}

/// Expand grayscale and RGB PNGs back to front so input and output can share a buffer.
fn expand_png<const N: usize>(samples: &mut [[u8; N]], channels: usize) {
    if channels == 4 {
        return;
    }
    for pixel in (0..samples.len() / 4).rev() {
        let src = pixel * channels;
        let dst = pixel * 4;
        let red = samples[src];
        let rgb = if channels == 3 {
            [red, samples[src + 1], samples[src + 2]]
        } else {
            [red; 3]
        };
        let alpha = if channels == 2 {
            samples[src + 1]
        } else {
            [255; N]
        };
        samples[dst..dst + 3].copy_from_slice(&rgb);
        samples[dst + 3] = alpha;
    }
}

/// The colour half of a DXT5 block as a BC1 block of the same four colours (see
/// [`Texture::bc3_colour_as_bc1`]).
fn dxt5_colour_as_bc1(block: &[u8; 16]) -> [u8; 8] {
    let colour = &block[8..];
    let (c0, c1) = (
        u16::from_le_bytes([colour[0], colour[1]]),
        u16::from_le_bytes([colour[2], colour[3]]),
    );
    let indices = u32::from_le_bytes([colour[4], colour[5], colour[6], colour[7]]);
    let (c0, c1, indices) = match c0.cmp(&c1) {
        std::cmp::Ordering::Greater => (c0, c1, indices),
        // Endpoints swapped: 0 and 1 trade places, so do the thirds (2 and 3).
        std::cmp::Ordering::Less => (c1, c0, indices ^ 0x5555_5555),
        // One colour: BC1 would read its fourth index as transparent black.
        std::cmp::Ordering::Equal => (c0, c1, 0),
    };
    let mut out = [0u8; 8];
    out[..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..].copy_from_slice(&indices.to_le_bytes());
    out
}
