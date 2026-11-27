//! The two DXT5 textures that are not colours as stored, which the resource says in its
//! "special dependencies" (what the texture compiler was asked to do): scaled YCoCg images and
//! tangent-space normals. Both are decoded to RGBA8, as the game's tools show them.

use super::format::PixelFormat;
use crate::budget::Budget;
use crate::error::Result;
use crate::resource::Resource;

const YCOCG: &str = "Texture Compiler Version Image YCoCg Conversion";
const NORMALS: &str = "Texture Compiler Version Image NormalizeNormals";

/// What a DXT5 texture holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Content {
    /// Red, green, blue and alpha.
    Colour,
    /// Colour as luma in alpha, chroma in red and green, a scale in blue.
    YCoCg,
    /// A normal's x in alpha and y in green.
    Normal,
}

impl Content {
    /// What the texture compiler was told to do with the image, read from the resource's edit
    /// info: the old `REDI` block lists its special dependencies as plain strings, the new
    /// `RED2` as a KV3 list.
    pub(super) fn of(resource: &Resource<'_>, budget: &mut Budget) -> Result<Self> {
        let legacy = resource.block(b"REDI");
        let edit = resource.kv3_in(b"RED2", budget).transpose()?;
        let dependencies = edit
            .as_ref()
            .and_then(|doc| doc.root.get("m_SpecialDependencies"))
            .and_then(|values| values.as_array())
            .unwrap_or_default();
        let says = |text: &str| {
            legacy.is_some_and(|bytes| {
                bytes
                    .windows(text.len())
                    .any(|word| word == text.as_bytes())
            }) || dependencies.iter().any(|dependency| {
                dependency.get("m_String").and_then(|value| value.as_str()) == Some(text)
            })
        };
        Ok(if says(YCOCG) {
            Self::YCoCg
        } else if says(NORMALS) {
            Self::Normal
        } else {
            Self::Colour
        })
    }

    pub(super) fn decode_bc3(self, level: &[u8], width: u32, height: u32) -> Vec<u8> {
        super::rgba::decode_blocks(PixelFormat::Bc3, level, width, height, |pixel| {
            self.unpack(pixel)
        })
    }

    pub(super) fn unpack(self, [r, g, b, a]: [u8; 4]) -> [u8; 4] {
        match self {
            Self::Colour => [r, g, b, a],
            Self::YCoCg => ycocg(r, g, b, a),
            Self::Normal => normal(r, g, a),
        }
    }
}

fn byte(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// Scaled YCoCg-DXT5: luma in alpha, chroma in red and green around 128 divided by a scale kept
/// in blue's top five bits.
fn ycocg(r: u8, g: u8, b: u8, y: u8) -> [u8; 4] {
    let scale = f32::from((b >> 3) + 1);
    let co = (f32::from(r) - 128.0) / scale;
    let cg = (f32::from(g) - 128.0) / scale;
    let y = f32::from(y);
    [byte(y + co - cg), byte(y + cg), byte(y - co - cg), 255]
}

/// A DXT5nm normal: x in alpha, y in green, z from the unit length (none when the pair is
/// longer than one, as the blocks round), in whole numbers as Source2Viewer reconstructs it; the
/// unused red is kept in alpha, where its swap of red and alpha leaves it.
fn normal(r: u8, g: u8, a: u8) -> [u8; 4] {
    let x = 2 * i32::from(a) - 255;
    let y = 2 * i32::from(g) - 255;
    // A negative square makes NaN, which becomes 0.
    let z = ((255 * 255 - x * x - y * y) as f32).sqrt() as i32;
    let unit = |v: i32| (v / 2 + 128).clamp(0, 255) as u8;
    [unit(x), unit(y), unit(z), r]
}
