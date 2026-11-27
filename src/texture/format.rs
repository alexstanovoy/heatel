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

macro_rules! vtex_formats {
    ($($(#[$doc:meta])* $name:ident = $id:literal),+ $(,)?) => {
        /// The exact Source 2 format identifier, before any unpacking or channel conversion.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum VTexFormat {
            $($(#[$doc])* $name,)+
            /// An unrecognized Source 2 format identifier.
            Unrecognized(u8),
        }

        impl VTexFormat {
            /// Interprets a header's format byte.
            pub const fn from_id(id: u8) -> Self {
                match id {
                    $($id => Self::$name,)+
                    other => Self::Unrecognized(other),
                }
            }

            /// Byte value in the Source 2 header.
            pub const fn id(self) -> u8 {
                match self {
                    $(Self::$name => $id,)+
                    Self::Unrecognized(id) => id,
                }
            }
        }
    };
}

vtex_formats! {
    /// Unknown Valve format.
    Unknown = 0,
    /// DXT1 (BC1).
    Dxt1 = 1,
    /// DXT5 (BC3).
    Dxt5 = 2,
    /// 8-bit intensity.
    I8 = 3,
    /// 8-bit RGBA.
    Rgba8888 = 4,
    /// 16-bit unsigned normalized red.
    R16 = 5,
    /// 16-bit unsigned normalized red and green.
    Rg1616 = 6,
    /// 16-bit unsigned normalized RGBA.
    Rgba16161616 = 7,
    /// Half-float red.
    R16f = 8,
    /// Half-float red and green.
    Rg1616f = 9,
    /// Half-float RGBA.
    Rgba16161616f = 10,
    /// Float red.
    R32f = 11,
    /// Float red and green.
    Rg3232f = 12,
    /// Float RGB.
    Rgb323232f = 13,
    /// Float RGBA.
    Rgba32323232f = 14,
    /// JPEG in an RGBA8888 wrapper.
    JpegRgba8888 = 15,
    /// PNG in an RGBA8888 wrapper.
    PngRgba8888 = 16,
    /// JPEG in a DXT5 wrapper.
    JpegDxt5 = 17,
    /// PNG in a DXT5 wrapper.
    PngDxt5 = 18,
    /// Unsigned HDR BC6H.
    Bc6h = 19,
    /// BC7.
    Bc7 = 20,
    /// ATI2N (BC5).
    Ati2n = 21,
    /// 8-bit intensity and alpha.
    Ia88 = 22,
    /// ETC2 RGB.
    Etc2 = 23,
    /// ETC2 RGBA.
    Etc2Eac = 24,
    /// EAC red.
    R11Eac = 25,
    /// EAC red and green.
    Rg11Eac = 26,
    /// ATI1N (BC4).
    Ati1n = 27,
    /// 8-bit BGRA.
    Bgra8888 = 28,
    /// WebP in an RGBA8888 wrapper.
    WebpRgba8888 = 29,
    /// WebP in a DXT5 wrapper.
    WebpDxt5 = 30,
    /// 32-bit unsigned integer red.
    R32Uint = 31,
    /// 8-bit alpha.
    A8 = 32,
    /// 8-bit unsigned normalized red.
    R8Unorm = 33,
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
    /// Maps a Source 2 format to its GPU storage or pixel conversion.
    pub(crate) fn from_format(format: VTexFormat) -> Result<Self> {
        use PixelFormat as P;
        use VTexFormat as V;
        Ok(match format {
            V::Dxt1 => Self::Plain(P::Bc1),
            V::Dxt5 => Self::Plain(P::Bc3),
            V::I8 | V::R8Unorm | V::A8 => Self::Plain(P::R8),
            V::Rgba8888 => Self::Plain(P::Rgba8),
            V::R16 => Self::Plain(P::R16),
            V::Rg1616 => Self::Plain(P::Rg16),
            V::Rgba16161616 => Self::Plain(P::Rgba16),
            V::R16f => Self::Plain(P::R16f),
            V::Rg1616f => Self::Plain(P::Rg16f),
            V::Rgba16161616f => Self::Plain(P::Rgba16f),
            V::R32f => Self::Plain(P::R32f),
            V::Rg3232f => Self::Plain(P::Rg32f),
            V::Rgba32323232f => Self::Plain(P::Rgba32f),
            V::Bc6h => Self::Plain(P::Bc6h),
            V::Bc7 => Self::Plain(P::Bc7),
            V::Ati2n => Self::Plain(P::Bc5),
            V::Ia88 => Self::Plain(P::Rg8),
            V::Etc2 => Self::Plain(P::Etc2Rgb),
            V::Etc2Eac => Self::Plain(P::Etc2Rgba),
            V::R11Eac => Self::Plain(P::EacR11),
            V::Rg11Eac => Self::Plain(P::EacRg11),
            V::Ati1n => Self::Plain(P::Bc4),
            V::PngRgba8888 | V::PngDxt5 => Self::Png,
            V::Bgra8888 => Self::Bgra8,
            _ => {
                return Err(malformed(
                    "vtex",
                    format!("unsupported format id {}", format.id()),
                ));
            }
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
