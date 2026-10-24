//! The functions driven by noise: an emission rate, a new particle's attribute and a force.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::field::Field;
use super::input::{FloatInput, VectorInput};
use super::read::{MAX_CONTROL_POINT, Obj};
use super::transform::ControlPoint;

/// `C_OP_NoiseEmitter`'s parameters: particles a second from noise over the system's age.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NoiseEmitter {
    /// How long it emits (0 forever).
    pub duration: FloatInput,
    /// When, after the system starts.
    pub start_time: FloatInput,
    /// Noise frequency in time.
    pub scale: FloatInput,
    /// The rate at the noise's low end.
    pub output_min: FloatInput,
    /// At its high end.
    pub output_max: FloatInput,
    /// Added to the age before the noise is sampled.
    pub offset: f32,
    /// Absolute noise (0..1, the whole range) rather than -1..1 over the range's middle.
    pub absolute: bool,
    /// The noise turned over (one less it).
    pub absolute_inverted: bool,
    /// Below behaviour version 2, the rate is also times this times the system's highest
    /// control point (0 none, and always from version 2 on).
    pub index_scale: f32,
    /// A control point component the rate is times, if any.
    pub scale_control_point: Option<(ControlPoint, u8)>,
    /// A control point whose position, offset and scaled, the noise is sampled at, if any.
    pub world_noise_point: Option<ControlPoint>,
    /// That position's scale.
    pub world_noise_scale: f32,
    /// That position's offset.
    pub offset_location: [f32; 3],
    /// The world's time in milliseconds times this is added to the noise's sample.
    pub world_time_scale: f32,
}

/// `C_INIT_CreationNoise`'s parameters: an attribute from noise at a new particle's place and
/// moment.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CreationNoise {
    /// The attribute; angles in degrees.
    pub field: Field,
    /// Absolute noise.
    pub absolute: bool,
    /// The noise turned over.
    pub absolute_inverted: bool,
    /// Added to the creation time.
    pub offset: f32,
    /// The value at the noise's low end.
    pub output_min: f32,
    /// At its high end.
    pub output_max: f32,
    /// Noise frequency in time.
    pub scale: f32,
    /// Noise frequency in space.
    pub scale_location: f32,
    /// The world's time in milliseconds times this is added to the time.
    pub world_time_scale: f32,
    /// Added to the place.
    pub offset_location: [f32; 3],
}

/// Which noise a noise force follows (`ParticleDirectionNoiseType_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum DirectionNoise {
    /// A vector of value noise.
    #[default]
    Value,
    /// The curl of value noise: a flow without sources or sinks.
    Curl,
    /// The offset to the nearest point of a jittered cell grid.
    Worley,
}

/// `C_OP_CurlNoiseForce`'s parameters: a push from noise at the particle's place.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CurlNoise {
    /// The noise.
    pub kind: DirectionNoise,
    /// Noise frequency per axis.
    pub frequency: VectorInput,
    /// The push per axis.
    pub scale: VectorInput,
    /// Added to the sample.
    pub offset: VectorInput,
    /// Added to the sample per second of the system's age.
    pub offset_rate: VectorInput,
    /// The cell grid's seed.
    pub worley_seed: FloatInput,
    /// How far a cell's point strays from its corner, 0..1.
    pub worley_jitter: FloatInput,
}

impl NoiseEmitter {
    pub(super) fn read(obj: &Obj<'_>, behavior_version: i32) -> Result<Self> {
        let point = |key: &str| -> Result<Option<ControlPoint>> {
            let index = obj.int_clamped(key, -1, -1, i64::from(MAX_CONTROL_POINT))?;
            Ok(u8::try_from(index).ok().and_then(ControlPoint::new))
        };
        let scale_field = obj.int_clamped("m_nScaleControlPointField", 0, -1, 2)?;
        Ok(Self {
            duration: FloatInput::read(obj, "m_flEmissionDuration", 0.0)?,
            start_time: FloatInput::read(obj, "m_flStartTime", 0.0)?,
            scale: FloatInput::read(obj, "m_flNoiseScale", 0.1)?,
            output_min: FloatInput::read(obj, "m_flOutputMin", 0.0)?,
            output_max: FloatInput::read(obj, "m_flOutputMax", 100.0)?,
            offset: obj.float("m_flOffset", 0.0)?,
            absolute: obj.bool("m_bAbsVal", false)?,
            absolute_inverted: obj.bool("m_bAbsValInv", false)?,
            index_scale: if behavior_version < 2 {
                obj.float("m_flEmissionScale", 0.0)?
            } else {
                0.0
            },
            scale_control_point: point("m_nScaleControlPoint")?.zip(u8::try_from(scale_field).ok()),
            world_noise_point: point("m_nWorldNoisePoint")?,
            world_noise_scale: obj.float("m_flWorldNoiseScale", 0.001)?,
            offset_location: obj.vec3("m_vecOffsetLoc", [0.0; 3])?,
            world_time_scale: obj.float("m_flWorldTimeScale", 0.0)?,
        })
    }
}

impl CreationNoise {
    pub(super) fn read(obj: &Obj<'_>) -> Result<Self> {
        Ok(Self {
            field: obj.field("m_nFieldOutput", Field::RADIUS)?,
            absolute: obj.bool("m_bAbsVal", false)?,
            absolute_inverted: obj.bool("m_bAbsValInv", false)?,
            offset: obj.float("m_flOffset", 0.0)?,
            output_min: obj.float("m_flOutputMin", 0.0)?,
            output_max: obj.float("m_flOutputMax", 1.0)?,
            scale: obj.float("m_flNoiseScale", 0.1)?,
            scale_location: obj.float("m_flNoiseScaleLoc", 0.001)?,
            world_time_scale: obj.float("m_flWorldTimeScale", 0.0)?,
            offset_location: obj.vec3("m_vecOffsetLoc", [0.0; 3])?,
        })
    }
}

impl CurlNoise {
    pub(super) fn read(obj: &Obj<'_>) -> Result<Self> {
        let kinds = [
            "PARTICLE_DIR_NOISE_PERLIN",
            "PARTICLE_DIR_NOISE_CURL",
            "PARTICLE_DIR_NOISE_WORLEY_BASIC",
        ];
        Ok(Self {
            kind: [
                DirectionNoise::Value,
                DirectionNoise::Curl,
                DirectionNoise::Worley,
            ][obj.choice("m_nNoiseType", &kinds, 0)?],
            frequency: VectorInput::read(obj, "m_vecNoiseFreq", [0.02; 3])?,
            scale: VectorInput::read(obj, "m_vecNoiseScale", [1000.0; 3])?,
            offset: VectorInput::read(obj, "m_vecOffset", [0.0; 3])?,
            offset_rate: VectorInput::read(obj, "m_vecOffsetRate", [0.0; 3])?,
            worley_seed: FloatInput::read(obj, "m_flWorleySeed", 0.0)?,
            worley_jitter: FloatInput::read(obj, "m_flWorleyJitter", 0.875)?,
        })
    }
}
