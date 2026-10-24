//! Renderers: how a system's particles are drawn.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::{Result, malformed};

use super::input::{FloatInput, VectorInput};
use super::models::Models;
use super::read::{MAX_LAYERS, MAX_NAME, Obj, WHAT};

/// A renderer class.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Renderer {
    /// `C_OP_RenderSprites`: a textured card per particle.
    Sprites(Box<Sprites>),
    /// `C_OP_RenderTrails`: a card per particle stretched back along its velocity.
    Trails(Box<Trails>),
    /// `C_OP_RenderRopes`: a ribbon through the particles in order.
    Ropes(Box<Ropes>),
    /// `C_OP_RenderProjected`: a decal per particle, projected onto what is below it.
    Projected(Box<Projected>),
    /// `C_OP_RenderDeferredLight`: a light per particle, of its colour, its radius' reach and
    /// its alpha's intensity.
    DeferredLight(Box<DeferredLight>),
    /// `C_OP_RenderModels`: a model per particle.
    Models(Box<Models>),
    /// `C_OP_RenderScreenVelocityRotate`: draws nothing; turns each particle's roll to the way
    /// it moved on the screen over its last step, for the renderers after it.
    ScreenVelocityRotate {
        /// Added to the roll, in degrees.
        forward_degrees: f32,
    },
    /// A class this crate does not read.
    Unsupported(String),
}

/// How a renderer blends into what is behind (`ParticleOutputBlendMode_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Blend {
    /// Over it, by alpha.
    #[default]
    Alpha,
    /// Added to it.
    Add,
    /// Added, and darkening by alpha.
    BlendAdd,
    /// Half each.
    HalfBlendAdd,
    /// Half added, half subtracted.
    NegHalfBlendAdd,
    /// Times twice the colour.
    Mod2x,
    /// The lighter of the two.
    Lighten,
}

/// How a card faces (`ParticleOrientationChoiceList_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Orientation {
    /// The camera.
    #[default]
    Screen,
    /// The camera, about the world's up axis.
    ScreenZ,
    /// Up, lying flat.
    WorldZ,
    /// Along the particle's normal.
    ParticleNormal,
    /// The camera, about the particle's normal.
    ScreenToNormal,
    /// The particle's own three rotations.
    Full,
}

/// How a texture layer gives a card its colour and alpha (Source's spritecard channel mix):
/// `A` is what the layers before it give (white and opaque before the first), `B` this layer's
/// texel; a luminance is Rec. 709's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum TextureChannels {
    /// Colour from `B`, alpha from `A`.
    Rgb,
    /// Colour and alpha from `B` (the usual).
    Rgba,
    /// Colour from `A`, alpha from `B`.
    A,
    /// Colour from `B`, alpha `A`'s times `B`'s.
    RgbA,
    /// `B`'s colour over `A`'s by `B`'s alpha, `A`'s alpha.
    RgbAlphaMask,
    /// `B`'s colour over `A`'s by `B`'s luminance, `A`'s alpha.
    RgbRgbMask,
    /// Colour from `B`, alpha `B`'s luminance.
    RgbaRgbAlpha,
    /// Colour from `A`, alpha `B`'s luminance.
    ARgbAlpha,
    /// Colour from `A`, alpha `A`'s times `B`'s luminance.
    RgbARgbAlpha,
    /// `B`'s red as colour, alpha from `A`.
    R,
    /// `B`'s green as colour, alpha from `A`.
    G,
    /// `B`'s blue as colour, alpha from `A`.
    B,
    /// `B`'s red as colour and alpha.
    RAlpha,
    /// `B`'s green as colour and alpha.
    GAlpha,
    /// `B`'s blue as colour and alpha.
    BAlpha,
}

/// How a texture layer after the first combines with what the ones before it give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum LayerBlend {
    /// Their product.
    Multiply,
    /// Twice their product: 0.5 in the layer leaves it.
    Mod2x,
    /// The layer alone.
    Replace,
    /// Their sum.
    Add,
    /// The one before less the layer.
    Subtract,
    /// Their mean.
    Average,
    /// What is before scaled by the layer's luminance, with the layer's alpha.
    Luminance,
}

/// A texture layer after a renderer's first.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TextureLayer {
    /// The texture (`materials/.../x.vtex`).
    pub texture: String,
    /// How it gives colour and alpha.
    pub channels: TextureChannels,
    /// How it combines with the layers before it.
    pub blend: LayerBlend,
    /// Its V scale and offset, then its U scale and offset.
    pub transform: [f32; 4],
}

const CHANNELS: [&str; 15] = [
    "SPRITECARD_TEXTURE_CHANNEL_MIX_RGB",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_RGBA",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_A",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_RGB_A",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_RGB_ALPHAMASK",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_RGB_RGBMASK",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_RGBA_RGBALPHA",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_A_RGBALPHA",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_RGB_A_RGBALPHA",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_R",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_G",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_B",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_RALPHA",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_GALPHA",
    "SPRITECARD_TEXTURE_CHANNEL_MIX_BALPHA",
];

const LAYER_BLENDS: [&str; 7] = [
    "SPRITECARD_TEXTURE_BLEND_MULTIPLY",
    "SPRITECARD_TEXTURE_BLEND_MOD2X",
    "SPRITECARD_TEXTURE_BLEND_REPLACE",
    "SPRITECARD_TEXTURE_BLEND_ADD",
    "SPRITECARD_TEXTURE_BLEND_SUBTRACT",
    "SPRITECARD_TEXTURE_BLEND_AVERAGE",
    "SPRITECARD_TEXTURE_BLEND_LUMINANCE",
];

impl TextureChannels {
    fn read(layer: &Obj<'_>) -> Result<Self> {
        Ok([
            Self::Rgb,
            Self::Rgba,
            Self::A,
            Self::RgbA,
            Self::RgbAlphaMask,
            Self::RgbRgbMask,
            Self::RgbaRgbAlpha,
            Self::ARgbAlpha,
            Self::RgbARgbAlpha,
            Self::R,
            Self::G,
            Self::B,
            Self::RAlpha,
            Self::GAlpha,
            Self::BAlpha,
        ][layer.choice("m_nTextureChannels", &CHANNELS, 1)?])
    }
}

impl LayerBlend {
    fn read(layer: &Obj<'_>) -> Result<Self> {
        Ok([
            Self::Multiply,
            Self::Mod2x,
            Self::Replace,
            Self::Add,
            Self::Subtract,
            Self::Average,
            Self::Luminance,
        ][layer.choice("m_nTextureBlendMode", &LAYER_BLENDS, 0)?])
    }
}

/// What every renderer reads: its texture and how its colour reaches the screen.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Look {
    /// The texture (`materials/.../x.vtex`), the first layer's.
    pub texture: Option<String>,
    /// The texture's V scale and offset, then its U scale and offset.
    pub texture_transform: [f32; 4],
    /// How the first layer's texture gives colour and alpha.
    pub channels: TextureChannels,
    /// The texture layers after the first, in order.
    pub layers: Vec<TextureLayer>,
    /// The blend.
    pub blend: Blend,
    /// The colour's scale before blending.
    pub overbright: FloatInput,
    /// Extra colour over an alpha blend (the colour times 1 plus this).
    pub add_self: FloatInput,
    /// The colour clamped to 1 before blending.
    pub saturate: bool,
    /// The particles' colours are in gamma space.
    pub gamma_vertex_colors: bool,
    /// The alpha's scale.
    pub alpha_scale: FloatInput,
    /// The radius' scale.
    pub radius_scale: FloatInput,
    /// The colour's scale.
    pub color_scale: VectorInput,
    /// Drawn over everything, not hidden by what is in front.
    pub no_depth_test: bool,
    /// The sprite sheet's frames a second (or per lifetime, see `animate_in_fps`).
    pub animation_rate: f32,
    /// The rate is in frames a second rather than over the sequence a second.
    pub animate_in_fps: bool,
    /// Frames blend into the next one.
    pub blend_frames: bool,
    /// How the card faces.
    pub orientation: Orientation,
    /// The texture is a normal map that bends the view of what is behind (a heat haze), not a
    /// colour.
    pub refract: bool,
}

/// `C_OP_RenderSprites`' parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Sprites {
    /// The shared parameters.
    pub look: Look,
    /// The least radius, as a share of the distance to the camera.
    pub min_size: FloatInput,
    /// The largest, likewise.
    pub max_size: FloatInput,
    /// A card larger than this share of the distance starts to fade.
    pub start_fade_size: FloatInput,
    /// And is gone at this one.
    pub end_fade_size: FloatInput,
}

/// `C_OP_RenderTrails`' parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Trails {
    /// The shared parameters.
    pub look: Look,
    /// The longest trail.
    pub max_length: f32,
    /// The shortest.
    pub min_length: f32,
    /// The length's scale.
    pub length_scale: f32,
    /// The seconds a trail grows to its length over.
    pub length_fade_in: f32,
    /// The length ignores the step's duration.
    pub ignore_dt: bool,
    /// The radius at most this times the length.
    pub radius_to_length: f32,
    /// The tail's alpha scale.
    pub tail_alpha_scale: FloatInput,
    /// The tail's radius scale.
    pub tail_radius_scale: FloatInput,
    /// The head's radius scale.
    pub head_radius_scale: FloatInput,
    /// The head moved forward by this share of the length.
    pub forward_shift: f32,
}

/// `C_OP_RenderRopes`' parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Ropes {
    /// The shared parameters.
    pub look: Look,
    /// The least pieces per span.
    pub min_tessellation: u8,
    /// The most.
    pub max_tessellation: u8,
    /// World units one texture repeat covers along the rope.
    pub texture_v_world_size: FloatInput,
    /// World units a second the texture scrolls.
    pub texture_v_scroll_rate: FloatInput,
    /// World units the texture starts offset.
    pub texture_v_offset: FloatInput,
}

/// `C_OP_RenderProjected`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Projected {
    /// The shared parameters; the materials, not these, name the texture and the blend.
    pub look: Look,
    /// The materials (`materials/.../x.vmat`) a particle picks one of.
    pub materials: Vec<String>,
    /// Which material, by index (clamped to the list).
    pub material_selection: FloatInput,
    /// The texture mirrored left to right.
    pub flip_horizontal: bool,
    /// The decal faces along the particle's normal rather than down.
    pub orient_to_normal: bool,
    /// How far below and above the particle the decal reaches, when set rather than its width.
    pub projection_depth: Option<[f32; 2]>,
}

/// `C_OP_RenderDeferredLight`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeferredLight {
    /// The reach's scale (the reach is the radius times it).
    pub radius_scale: FloatInput,
    /// The intensity's scale (the intensity is the alpha times it).
    pub alpha_scale: FloatInput,
    /// The colour's scale, combined with the particle's colour by `color_blend`.
    pub color_scale: VectorInput,
    /// How the colour scale combines with the particle's colour.
    pub color_blend: ColorBlend,
    /// The share of the reach past which the light starts to fade out.
    pub start_falloff: f32,
    /// A spot light's reach along the particle's normal (0: a point light).
    pub light_distance: f32,
    /// A spot light's field of view, in degrees.
    pub spot_fov: f32,
    /// The texture it names (`materials/.../x.vtex`), which a deferred light does not draw.
    pub texture: Option<String>,
}

/// How a renderer's colour scale combines with a particle's colour
/// (`ParticleColorBlendType_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum ColorBlend {
    /// The colour times the scale.
    #[default]
    Multiply,
    /// The colour times twice the scale.
    Multiply2x,
    /// The colour over the scale.
    Divide,
    /// The colour plus the scale.
    Add,
    /// The colour less the scale.
    Subtract,
    /// The colour times twice the scale (as `Multiply2x`).
    Mod2x,
    /// The colour screened by the scale.
    Screen,
    /// The larger of each channel.
    Max,
    /// The smaller of each channel.
    Min,
    /// The scale in place of the colour.
    Replace,
    /// The mean of the two.
    Average,
    /// The colour times the scale, inverted.
    Negate,
    /// The colour times the scale's luminance.
    Luminance,
}

const COLOR_BLENDS: [&str; 13] = [
    "PARTICLE_COLOR_BLEND_MULTIPLY",
    "PARTICLE_COLOR_BLEND_MULTIPLY2X",
    "PARTICLE_COLOR_BLEND_DIVIDE",
    "PARTICLE_COLOR_BLEND_ADD",
    "PARTICLE_COLOR_BLEND_SUBTRACT",
    "PARTICLE_COLOR_BLEND_MOD2X",
    "PARTICLE_COLOR_BLEND_SCREEN",
    "PARTICLE_COLOR_BLEND_MAX",
    "PARTICLE_COLOR_BLEND_MIN",
    "PARTICLE_COLOR_BLEND_REPLACE",
    "PARTICLE_COLOR_BLEND_AVERAGE",
    "PARTICLE_COLOR_BLEND_NEGATE",
    "PARTICLE_COLOR_BLEND_LUMINANCE",
];

/// The material a projected renderer has when its file names none.
const BASE_PROJECTED: &str = "materials/particle/base_projected.vmat";
/// The format that put a projected renderer's material into a list (`vpcf59`).
const PROJECTED_LIST: u32 = 59;

const BLENDS: [&str; 7] = [
    "PARTICLE_OUTPUT_BLEND_MODE_ALPHA",
    "PARTICLE_OUTPUT_BLEND_MODE_ADD",
    "PARTICLE_OUTPUT_BLEND_MODE_BLEND_ADD",
    "PARTICLE_OUTPUT_BLEND_MODE_HALF_BLEND_ADD",
    "PARTICLE_OUTPUT_BLEND_MODE_NEG_HALF_BLEND_ADD",
    "PARTICLE_OUTPUT_BLEND_MODE_MOD2X",
    "PARTICLE_OUTPUT_BLEND_MODE_LIGHTEN",
];

const ORIENTATIONS: [&str; 6] = [
    "PARTICLE_ORIENTATION_SCREEN_ALIGNED",
    "PARTICLE_ORIENTATION_SCREEN_Z_ALIGNED",
    "PARTICLE_ORIENTATION_WORLD_Z_ALIGNED",
    "PARTICLE_ORIENTATION_ALIGN_TO_PARTICLE_NORMAL",
    "PARTICLE_ORIENTATION_SCREENALIGN_TO_PARTICLE_NORMAL",
    "PARTICLE_ORIENTATION_FULL_3AXIS_ROTATION",
];

impl Renderer {
    pub(super) fn read(class: &str, obj: &Obj<'_>, format: u32) -> Result<Self> {
        Ok(match class {
            "C_OP_RenderSprites" => Self::Sprites(Box::new(Sprites {
                look: Look::read(obj)?,
                min_size: FloatInput::read(obj, "m_flMinSize", 0.0)?,
                max_size: FloatInput::read(obj, "m_flMaxSize", 5000.0)?,
                start_fade_size: FloatInput::read(obj, "m_flStartFadeSize", 1e8)?,
                end_fade_size: FloatInput::read(obj, "m_flEndFadeSize", 2e8)?,
            })),
            "C_OP_RenderTrails" => Self::Trails(Box::new(Trails {
                look: Look::read(obj)?,
                max_length: obj.float("m_flMaxLength", 2000.0)?,
                min_length: obj.float("m_flMinLength", 0.0)?,
                length_scale: obj.float("m_flLengthScale", 1.0)?,
                length_fade_in: obj.float("m_flLengthFadeInTime", 0.0)?,
                ignore_dt: obj.bool("m_bIgnoreDT", false)?,
                radius_to_length: obj.float("m_flConstrainRadiusToLengthRatio", 1.0)?,
                tail_alpha_scale: FloatInput::read(obj, "m_flTailAlphaScale", 1.0)?,
                tail_radius_scale: FloatInput::read(obj, "m_flRadiusTaper", 1.0)?,
                head_radius_scale: FloatInput::read(obj, "m_flRadiusHeadTaper", 1.0)?,
                forward_shift: obj.float("m_flForwardShift", 0.0)?,
            })),
            "C_OP_RenderRopes" => Self::Ropes(Box::new(Ropes {
                look: Look::read(obj)?,
                min_tessellation: obj.int_clamped("m_nMinTesselation", 1, 0, 128)? as u8,
                max_tessellation: obj.int_clamped("m_nMaxTesselation", 128, 0, 128)? as u8,
                texture_v_world_size: FloatInput::read(obj, "m_flTextureVWorldSize", 10.0)?,
                texture_v_scroll_rate: FloatInput::read(obj, "m_flTextureVScrollRate", 0.0)?,
                texture_v_offset: FloatInput::read(obj, "m_flTextureVOffset", 0.0)?,
            })),
            "C_OP_RenderProjected" => Self::Projected(Box::new(projected(obj, format)?)),
            "C_OP_RenderDeferredLight" => Self::DeferredLight(Box::new(deferred_light(obj)?)),
            "C_OP_RenderModels" => Self::Models(Box::new(Models::read(obj, format)?)),
            "C_OP_RenderScreenVelocityRotate" => Self::ScreenVelocityRotate {
                forward_degrees: obj.float("m_flForwardDegrees", -90.0)?,
            },
            other => Self::Unsupported(other.to_owned()),
        })
    }

    /// The shared parameters of a renderer this crate reads.
    pub fn look(&self) -> Option<&Look> {
        match self {
            Self::Sprites(s) => Some(&s.look),
            Self::Trails(t) => Some(&t.look),
            Self::Ropes(r) => Some(&r.look),
            Self::Projected(p) => Some(&p.look),
            Self::DeferredLight(_)
            | Self::Models(_)
            | Self::ScreenVelocityRotate { .. }
            | Self::Unsupported(_) => None,
        }
    }
}

fn deferred_light(obj: &Obj<'_>) -> Result<DeferredLight> {
    let layers = obj.array("m_vecTexturesInput", MAX_LAYERS)?;
    let texture = match layers.first().map(Obj::new).transpose()? {
        Some(layer) => layer.str("m_hTexture")?,
        None => obj.str("m_hTexture")?,
    };
    if texture.is_some_and(|t| t.len() > MAX_NAME) {
        return Err(malformed(
            WHAT,
            "a deferred light's texture path is too long",
        ));
    }
    let blends = [
        ColorBlend::Multiply,
        ColorBlend::Multiply2x,
        ColorBlend::Divide,
        ColorBlend::Add,
        ColorBlend::Subtract,
        ColorBlend::Mod2x,
        ColorBlend::Screen,
        ColorBlend::Max,
        ColorBlend::Min,
        ColorBlend::Replace,
        ColorBlend::Average,
        ColorBlend::Negate,
        ColorBlend::Luminance,
    ];
    Ok(DeferredLight {
        radius_scale: FloatInput::read(obj, "m_flRadiusScale", 1.0)?,
        alpha_scale: FloatInput::read(obj, "m_flAlphaScale", 1.0)?,
        color_scale: VectorInput::read(obj, "m_vecColorScale", [1.0; 3])?,
        color_blend: blends[obj.choice("m_nColorBlendType", &COLOR_BLENDS, 0)?],
        start_falloff: obj.float("m_flStartFalloff", 0.25)?,
        light_distance: obj.float("m_flLightDistance", 0.0)?,
        spot_fov: obj.float("m_flSpotFoV", 60.0)?,
        texture: texture.filter(|t| !t.is_empty()).map(str::to_owned),
    })
}

fn projected(obj: &Obj<'_>, format: u32) -> Result<Projected> {
    let mut materials = Vec::new();
    for entry in obj.array("m_vecProjectedMaterials", MAX_LAYERS)? {
        let path = Obj::new(entry)?.str("m_hMaterial")?.unwrap_or_default();
        materials.push(path.to_owned());
    }
    // Before `vpcf59` the one material was a member of its own, appended to the list.
    if format < PROJECTED_LIST && materials.len() < MAX_LAYERS {
        let path = obj.str("m_hProjectedMaterial")?.unwrap_or(BASE_PROJECTED);
        materials.push(path.to_owned());
    }
    if materials.iter().any(|m| m.len() > MAX_NAME) {
        return Err(malformed(WHAT, "a projected material's path is too long"));
    }
    let depth = obj
        .bool("m_bEnableProjectedDepthControls", false)?
        .then(|| -> Result<[f32; 2]> {
            Ok([
                obj.float("m_flMinProjectionDepth", -128.0)?,
                obj.float("m_flMaxProjectionDepth", 128.0)?,
            ])
        })
        .transpose()?;
    Ok(Projected {
        look: Look::read(obj)?,
        materials,
        material_selection: FloatInput::read(obj, "m_flMaterialSelection", 0.0)?,
        flip_horizontal: obj.bool("m_bFlipHorizontal", false)?,
        orient_to_normal: obj.bool("m_bOrientToNormal", false)?,
        projection_depth: depth,
    })
}

/// A texture layer's V scale and offset, then its U scale and offset (`m_TextureControls`).
fn texture_transform(layer: Option<&Obj<'_>>) -> Result<[f32; 4]> {
    let controls = layer
        .map(|layer| layer.object("m_TextureControls"))
        .transpose()?
        .flatten();
    let control = |key: &str, default: f32| -> Result<f32> {
        let Some(controls) = &controls else {
            return Ok(default);
        };
        // Per-frame texture animation is not read: its literal value stands.
        Ok(match FloatInput::read(controls, key, default)? {
            FloatInput::Literal(value) => value,
            FloatInput::Unsupported { literal, .. } => literal,
            _ => default,
        })
    };
    Ok([
        control("m_flFinalTextureScaleV", 1.0)?,
        control("m_flFinalTextureOffsetV", 0.0)?,
        control("m_flFinalTextureScaleU", 1.0)?,
        control("m_flFinalTextureOffsetU", 0.0)?,
    ])
}

impl Look {
    fn read(obj: &Obj<'_>) -> Result<Self> {
        let entries = obj.array("m_vecTexturesInput", MAX_LAYERS)?;
        let mut entries = entries.iter().map(Obj::new);
        let first = entries.next().transpose()?;
        let texture = match &first {
            Some(layer) => layer.str("m_hTexture")?,
            None => obj.str("m_hTexture")?,
        };
        let mut layers = Vec::new();
        for layer in entries {
            let layer = layer?;
            let Some(texture) = layer.str("m_hTexture")?.filter(|t| !t.is_empty()) else {
                continue;
            };
            if texture.len() > MAX_NAME {
                return Err(malformed(WHAT, "a texture layer's path is too long"));
            }
            layers.push(TextureLayer {
                texture: texture.to_owned(),
                channels: TextureChannels::read(&layer)?,
                blend: LayerBlend::read(&layer)?,
                transform: texture_transform(Some(&layer))?,
            });
        }
        Ok(Self {
            texture: texture.filter(|t| !t.is_empty()).map(str::to_owned),
            texture_transform: texture_transform(first.as_ref())?,
            channels: first
                .as_ref()
                .map_or(Ok(TextureChannels::Rgba), TextureChannels::read)?,
            layers,
            blend: [
                Blend::Alpha,
                Blend::Add,
                Blend::BlendAdd,
                Blend::HalfBlendAdd,
                Blend::NegHalfBlendAdd,
                Blend::Mod2x,
                Blend::Lighten,
            ][obj.choice("m_nOutputBlendMode", &BLENDS, 0)?],
            overbright: FloatInput::read(obj, "m_flOverbrightFactor", 1.0)?,
            add_self: FloatInput::read(obj, "m_flAddSelfAmount", 0.0)?,
            saturate: obj.bool("m_bSaturateColorPreAlphaBlend", true)?,
            gamma_vertex_colors: obj.bool("m_bGammaCorrectVertexColors", true)?,
            alpha_scale: FloatInput::read(obj, "m_flAlphaScale", 1.0)?,
            radius_scale: FloatInput::read(obj, "m_flRadiusScale", 1.0)?,
            color_scale: VectorInput::read(obj, "m_vecColorScale", [1.0; 3])?,
            no_depth_test: obj.bool("m_bDisableZBuffering", false)?,
            animation_rate: obj.float("m_flAnimationRate", 0.1)?,
            animate_in_fps: obj.bool("m_bAnimateInFPS", false)?,
            blend_frames: obj.bool("m_bBlendFramesSeq0", true)?,
            orientation: [
                Orientation::Screen,
                Orientation::ScreenZ,
                Orientation::WorldZ,
                Orientation::ParticleNormal,
                Orientation::ScreenToNormal,
                Orientation::Full,
            ][obj.choice("m_nOrientationType", &ORIENTATIONS, 0)?],
            refract: obj.bool("m_bRefract", false)?,
        })
    }
}
