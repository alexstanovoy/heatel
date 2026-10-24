use crate::error::{Result, malformed};

/// How a texture's texels are stored: a block-compressed format of the GPU, or plain channels.
/// Compressed blocks pass through from the `vtex_c` untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PixelFormat {
    /// BC1 (DXT1): RGB, 1-bit alpha.
    Bc1,
    /// BC3 (DXT5): RGBA.
    Bc3,
    /// BC4: one channel.
    Bc4,
    /// BC5: two channels.
    Bc5,
    /// BC6H: HDR RGB.
    Bc6h,
    /// BC7: RGBA.
    Bc7,
    /// ETC2 RGB.
    Etc2Rgb,
    /// ETC2 RGBA.
    Etc2Rgba,
    /// EAC R11: one channel.
    EacR11,
    /// EAC RG11: two channels.
    EacRg11,
    /// 8-bit red.
    R8,
    /// 8-bit red and green.
    Rg8,
    /// 8-bit RGBA.
    Rgba8,
    /// 16-bit red.
    R16,
    /// 16-bit red and green.
    Rg16,
    /// 16-bit RGBA.
    Rgba16,
    /// Half-float red.
    R16f,
    /// Half-float red and green.
    Rg16f,
    /// Half-float RGBA.
    Rgba16f,
    /// Float red.
    R32f,
    /// Float red and green.
    Rg32f,
    /// Float RGBA.
    Rgba32f,
}

/// What a `vtex_c` format id stores, and what has to happen to it on the way to a
/// [`PixelFormat`].
#[derive(Clone, Copy)]
pub(crate) enum Stored {
    /// Texels laid out as the pixel format has them.
    Plain(PixelFormat),
    /// Blue, green, red, alpha: swapped to RGBA.
    Bgra8,
    /// The first mip is a PNG file, decoded to RGBA8.
    Png,
}

impl Stored {
    /// The format ids of `vtex_c` (Valve's `VTexFormat`). The JPEG ids and the three-channel
    /// 32-bit float have no counterpart in the GPU formats a KTX2 can name, and no texture of
    /// the game uses them.
    pub(crate) fn from_id(id: u8) -> Result<Self> {
        use PixelFormat as P;
        Ok(match id {
            1 => Self::Plain(P::Bc1),
            2 => Self::Plain(P::Bc3),
            3 => Self::Plain(P::R8),
            4 => Self::Plain(P::Rgba8),
            5 => Self::Plain(P::R16),
            6 => Self::Plain(P::Rg16),
            7 => Self::Plain(P::Rgba16),
            8 => Self::Plain(P::R16f),
            9 => Self::Plain(P::Rg16f),
            10 => Self::Plain(P::Rgba16f),
            11 => Self::Plain(P::R32f),
            12 => Self::Plain(P::Rg32f),
            14 => Self::Plain(P::Rgba32f),
            16 | 18 => Self::Png,
            19 => Self::Plain(P::Bc6h),
            20 => Self::Plain(P::Bc7),
            21 => Self::Plain(P::Bc5),
            22 => Self::Plain(P::Rg8),
            23 => Self::Plain(P::Etc2Rgb),
            24 => Self::Plain(P::Etc2Rgba),
            25 => Self::Plain(P::EacR11),
            26 => Self::Plain(P::EacRg11),
            27 => Self::Plain(P::Bc4),
            28 => Self::Bgra8,
            _ => return Err(malformed("vtex", format!("unsupported format id {id}"))),
        })
    }

    /// The format the decoded texels have (and the stored ones, but for the PNG).
    pub(crate) fn decoded(self) -> PixelFormat {
        match self {
            Self::Plain(format) => format,
            Self::Bgra8 | Self::Png => PixelFormat::Rgba8,
        }
    }
}

impl PixelFormat {
    /// Texels per block along x and y, and bytes per block (1 x 1 for plain channels).
    pub fn block(self) -> (u32, u32, usize) {
        match self {
            Self::Bc1 | Self::Bc4 | Self::Etc2Rgb | Self::EacR11 => (4, 4, 8),
            Self::Bc3 | Self::Bc5 | Self::Bc6h | Self::Bc7 | Self::Etc2Rgba | Self::EacRg11 => {
                (4, 4, 16)
            }
            Self::R8 => (1, 1, 1),
            Self::Rg8 | Self::R16 | Self::R16f => (1, 1, 2),
            Self::Rgba8 | Self::Rg16 | Self::Rg16f | Self::R32f => (1, 1, 4),
            Self::Rgba16 | Self::Rgba16f | Self::Rg32f => (1, 1, 8),
            Self::Rgba32f => (1, 1, 16),
        }
    }

    /// The bytes of one mip level `width` x `height` texels, none when they overflow.
    pub fn level_size(self, width: u32, height: u32) -> Option<usize> {
        let (bw, bh, bytes) = self.block();
        let blocks = (width.div_ceil(bw) as usize).checked_mul(height.div_ceil(bh) as usize)?;
        blocks.checked_mul(bytes)
    }
}
