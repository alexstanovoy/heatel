//! Renderers: how a system's particles are drawn.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::input::{FloatInput, VectorInput};
use super::read::Obj;

/// Most texture layers a renderer has.
const MAX_LAYERS: usize = 8;

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

/// What every renderer reads: its texture and how its colour reaches the screen.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Look {
    /// The texture (`materials/.../x.vtex`), the first layer's.
    pub texture: Option<String>,
    /// The texture's V scale and offset, then its U scale and offset.
    pub texture_transform: [f32; 4],
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
    pub(super) fn read(class: &str, obj: &Obj<'_>) -> Result<Self> {
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
            other => Self::Unsupported(other.to_owned()),
        })
    }

    /// The shared parameters of a renderer this crate reads.
    pub fn look(&self) -> Option<&Look> {
        match self {
            Self::Sprites(s) => Some(&s.look),
            Self::Trails(t) => Some(&t.look),
            Self::Ropes(r) => Some(&r.look),
            Self::Unsupported(_) => None,
        }
    }
}

impl Look {
    fn read(obj: &Obj<'_>) -> Result<Self> {
        let layers = obj.array("m_vecTexturesInput", MAX_LAYERS)?;
        let first = layers.first().map(Obj::new).transpose()?;
        let texture = match first {
            Some(layer) => layer.str("m_hTexture")?,
            None => obj.str("m_hTexture")?,
        };
        let controls = first
            .map(|layer| layer.object("m_TextureControls"))
            .transpose()?
            .flatten();
        let control = |key: &str, default: f32| -> Result<f32> {
            let Some(controls) = controls else {
                return Ok(default);
            };
            // Per-frame texture animation is not read: its literal value stands.
            Ok(match FloatInput::read(&controls, key, default)? {
                FloatInput::Literal(value) => value,
                FloatInput::Unsupported { literal, .. } => literal,
                _ => default,
            })
        };
        Ok(Self {
            texture: texture.filter(|t| !t.is_empty()).map(str::to_owned),
            texture_transform: [
                control("m_flFinalTextureScaleV", 1.0)?,
                control("m_flFinalTextureOffsetV", 0.0)?,
                control("m_flFinalTextureScaleU", 1.0)?,
                control("m_flFinalTextureOffsetU", 0.0)?,
            ],
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
        })
    }
}
