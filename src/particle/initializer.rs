//! Initializers: what a particle starts with.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::field::{Field, SetMethod};
use super::input::{FloatInput, VectorInput};
use super::read::{MAX_CONTROL_POINT, Obj};
use super::transform::{ControlPoint, Transform};

/// The format that turned the local speed of `C_INIT_CreateWithinSphereTransform` around its Y
/// axis (`vpcf62`): older files have it mirrored.
const LOCAL_SPEED_FLIP: u32 = 62;

/// An initializer class.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Initializer {
    /// `C_INIT_InitFloat`: an attribute from an input.
    InitFloat {
        /// The attribute.
        field: Field,
        /// The value.
        value: FloatInput,
        /// How it is written.
        set_method: SetMethod,
        /// How far the attribute moves to it, 0..1.
        strength: FloatInput,
    },
    /// `C_INIT_RandomColor`: a colour between two.
    RandomColor {
        /// One end.
        min: [f32; 3],
        /// The other end.
        max: [f32; 3],
        /// The attribute.
        field: Field,
    },
    /// `C_INIT_RandomSequence`: a sprite sheet sequence.
    RandomSequence {
        /// The lowest.
        min: i32,
        /// The highest.
        max: i32,
        /// In a shuffled order rather than at random.
        shuffle: bool,
        /// In order rather than at random.
        linear: bool,
    },
    /// `C_INIT_RandomYawFlip`: the yaw turned half around for a share of the particles.
    RandomYawFlip {
        /// The share.
        percent: f32,
    },
    /// `C_INIT_CreateWithinSphere(Transform)`: a position in a shell about a transform and a
    /// velocity out of it.
    CreateWithinSphere(Box<Sphere>),
    /// `C_INIT_PositionOffset`: the position moved by a random offset.
    PositionOffset {
        /// The offset's low corner.
        min: VectorInput,
        /// The offset's high corner.
        max: VectorInput,
        /// Whose orientation turns it.
        transform: Transform,
        /// In the transform's frame.
        local: bool,
        /// Times the radius.
        proportional: bool,
    },
    /// `C_INIT_InitialVelocityNoise`: a velocity from noise at the particle's place and time.
    InitialVelocityNoise {
        /// The velocity's low corner.
        min: VectorInput,
        /// The velocity's high corner.
        max: VectorInput,
        /// Noise frequency in time.
        scale: FloatInput,
        /// Noise frequency in space.
        scale_location: FloatInput,
        /// Time offset.
        offset: FloatInput,
        /// Place offset.
        offset_location: VectorInput,
        /// Components taken as absolute values.
        absolute: [bool; 3],
        /// Components taken as inverted absolute values.
        absolute_inverted: [bool; 3],
        /// Whose orientation turns it.
        transform: Transform,
        /// A velocity per step rather than per second.
        ignore_dt: bool,
    },
    /// `C_INIT_VelocityRandom`: a random velocity in a control point's frame plus a random speed.
    VelocityRandom {
        /// The local velocity's low corner.
        local_min: VectorInput,
        /// The local velocity's high corner.
        local_max: VectorInput,
        /// The speed in a random direction, low end.
        speed_min: FloatInput,
        /// High end.
        speed_max: FloatInput,
        /// The frame's control point.
        control_point: ControlPoint,
        /// A velocity per step rather than per second.
        ignore_dt: bool,
    },
    /// `C_INIT_RingWave`: positions on a ring about a transform, moving out of it.
    RingWave(Box<Ring>),
    /// `C_INIT_RemapParticleCountToScalar`: an attribute from the particle's number.
    RemapParticleCountToScalar {
        /// The attribute.
        field: Field,
        /// The numbers mapped.
        input: [i64; 2],
        /// To these values.
        output: [f32; 2],
        /// Only numbers within the input.
        active_range: bool,
        /// Counted from the end.
        invert: bool,
        /// The count starts over past the input's end.
        wrap: bool,
        /// A bias of the output (0.5 none).
        bias: f32,
        /// How it is written.
        set_method: SetMethod,
    },
    /// `C_INIT_AgeNoise`: the particle starts aged, by noise.
    AgeNoise {
        /// Absolute noise.
        absolute: bool,
        /// Inverted absolute noise.
        absolute_inverted: bool,
        /// Time offset.
        offset: f32,
        /// The share of its life, low end.
        age_min: f32,
        /// High end.
        age_max: f32,
        /// Noise frequency in time.
        scale: f32,
        /// Noise frequency in space.
        scale_location: f32,
        /// Place offset.
        offset_location: [f32; 3],
    },
    /// `C_INIT_InheritVelocity`: the velocity of a control point.
    InheritVelocity {
        /// The control point.
        control_point: ControlPoint,
        /// Times this.
        scale: f32,
    },
    /// A class this crate does not read.
    Unsupported(String),
}

/// `C_INIT_CreateWithinSphereTransform`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Sphere {
    /// The centre.
    pub transform: Transform,
    /// The shell's inner radius.
    pub radius_min: FloatInput,
    /// Its outer radius.
    pub radius_max: FloatInput,
    /// The speed out of the centre, low end.
    pub speed_min: FloatInput,
    /// High end.
    pub speed_max: FloatInput,
    /// The exponent of the speed's draw.
    pub speed_exponent: f32,
    /// A velocity in the transform's frame, low corner.
    pub local_speed_min: VectorInput,
    /// High corner.
    pub local_speed_max: VectorInput,
    /// Scales the direction per axis.
    pub distance_bias: VectorInput,
    /// Axes the direction is made positive on.
    pub distance_bias_absolute: [bool; 3],
    /// The bias in the transform's frame.
    pub local: bool,
    /// The attribute the position goes to.
    pub output: Field,
    /// The attribute the velocity goes to (`POSITION_PREVIOUS` as a velocity).
    pub velocity: Field,
}

/// `C_INIT_RingWave`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Ring {
    /// The centre.
    pub transform: Transform,
    /// Particles spaced evenly rather than at random.
    pub even: bool,
    /// Particles per turn when even (-1 the particle count).
    pub per_orbit: FloatInput,
    /// The radius.
    pub radius: FloatInput,
    /// A random offset of up to this.
    pub thickness: FloatInput,
    /// The speed out, low end.
    pub speed_min: FloatInput,
    /// High end.
    pub speed_max: FloatInput,
    /// The ring's pitch, yaw and roll in degrees.
    pub angles: [FloatInput; 3],
    /// The speed out stays level.
    pub xy_velocity_only: bool,
}

impl Initializer {
    pub(super) fn read(class: &str, obj: &Obj<'_>, format: u32) -> Result<Self> {
        Ok(match class {
            "C_INIT_InitFloat" => Self::InitFloat {
                field: obj.field("m_nOutputField", Field::RADIUS)?,
                value: FloatInput::read(obj, "m_InputValue", 0.0)?,
                set_method: SetMethod::from_index(obj.choice(
                    "m_nSetMethod",
                    &SetMethod::NAMES,
                    0,
                )?),
                strength: FloatInput::read(obj, "m_InputStrength", 1.0)?,
            },
            "C_INIT_RandomColor" => Self::RandomColor {
                min: obj.color("m_ColorMin", [1.0; 3])?,
                max: obj.color("m_ColorMax", [1.0; 3])?,
                field: obj.field("m_nFieldOutput", Field::COLOR)?,
            },
            "C_INIT_RandomSequence" if !obj.has("m_WeightedList") => Self::RandomSequence {
                min: obj.int_in("m_nSequenceMin", 0, 0, 1 << 16)? as i32,
                max: obj.int_in("m_nSequenceMax", 0, 0, 1 << 16)? as i32,
                shuffle: obj.bool("m_bShuffle", false)?,
                linear: obj.bool("m_bLinear", false)?,
            },
            "C_INIT_RandomYawFlip" => Self::RandomYawFlip {
                percent: obj.float("m_flPercent", 0.5)?,
            },
            "C_INIT_CreateWithinSphere" | "C_INIT_CreateWithinSphereTransform" => {
                Self::CreateWithinSphere(Box::new(sphere(obj, format)?))
            }
            "C_INIT_PositionOffset" => Self::PositionOffset {
                min: VectorInput::read(obj, "m_OffsetMin", [0.0; 3])?,
                max: VectorInput::read(obj, "m_OffsetMax", [0.0; 3])?,
                transform: Transform::read(obj, "m_nControlPointNumber", true)?,
                local: obj.bool("m_bLocalCoords", false)?,
                proportional: obj.bool("m_bProportional", false)?,
            },
            "C_INIT_InitialVelocityNoise" => velocity_noise(obj)?,
            "C_INIT_VelocityRandom" => Self::VelocityRandom {
                local_min: VectorInput::read(obj, "m_LocalCoordinateSystemSpeedMin", [0.0; 3])?,
                local_max: VectorInput::read(obj, "m_LocalCoordinateSystemSpeedMax", [0.0; 3])?,
                speed_min: FloatInput::read(obj, "m_fSpeedMin", 0.0)?,
                speed_max: FloatInput::read(obj, "m_fSpeedMax", 0.0)?,
                control_point: obj.control_point("m_nControlPointNumber", 0)?,
                ignore_dt: obj.bool("m_bIgnoreDT", false)?,
            },
            "C_INIT_RingWave" => Self::RingWave(Box::new(ring(obj)?)),
            "C_INIT_RemapParticleCountToScalar" if !obj.has("m_nScaleControlPoint") => {
                count_to_scalar(obj)?
            }
            "C_INIT_AgeNoise" => age_noise(obj)?,
            "C_INIT_InheritVelocity" => Self::InheritVelocity {
                control_point: obj.control_point("m_nControlPointNumber", 0)?,
                scale: obj.float("m_flVelocityScale", 1.0)?,
            },
            other => Self::Unsupported(other.to_owned()),
        })
    }
}

fn sphere(obj: &Obj<'_>, format: u32) -> Result<Sphere> {
    let mut transform = Transform::read(obj, "m_nControlPointNumber", true)?;
    // Before `vpcf50` a range of control points was spelled out beside the first one.
    if let (Transform::ControlPoint { range_max, .. }, false) =
        (&mut transform, obj.has("m_TransformInput"))
    {
        let highest = obj.bool("m_bUseHighestEndCP", false)?;
        if highest || obj.bool("m_bDistributeInCPRange", false)? {
            let max = obj.int_in(
                "m_nControlPointNumberMax",
                0,
                -1,
                i64::from(MAX_CONTROL_POINT),
            )?;
            *range_max = ControlPoint::new(if highest {
                MAX_CONTROL_POINT
            } else {
                max.max(0) as u8
            });
        }
    }
    let mut local_speed_min = VectorInput::read(obj, "m_LocalCoordinateSystemSpeedMin", [0.0; 3])?;
    let mut local_speed_max = VectorInput::read(obj, "m_LocalCoordinateSystemSpeedMax", [0.0; 3])?;
    if format < LOCAL_SPEED_FLIP {
        local_speed_min = local_speed_min.negated_y();
        local_speed_max = local_speed_max.negated_y();
    }
    let absolute = obj.vec3("m_vecDistanceBiasAbs", [0.0; 3])?;
    Ok(Sphere {
        transform,
        radius_min: FloatInput::read(obj, "m_fRadiusMin", 0.0)?,
        radius_max: FloatInput::read(obj, "m_fRadiusMax", 0.0)?,
        speed_min: FloatInput::read(obj, "m_fSpeedMin", 0.0)?,
        speed_max: FloatInput::read(obj, "m_fSpeedMax", 0.0)?,
        speed_exponent: obj.float("m_fSpeedRandExp", 1.0)?.clamp(-255.0, 255.0),
        local_speed_min,
        local_speed_max,
        distance_bias: VectorInput::read(obj, "m_vecDistanceBias", [1.0; 3])?,
        distance_bias_absolute: absolute.map(|a| a != 0.0),
        local: obj.bool("m_bLocalCoords", false)?,
        output: obj.field("m_nFieldOutput", Field::POSITION)?,
        velocity: obj.field("m_nFieldVelocity", Field::POSITION_PREVIOUS)?,
    })
}

fn velocity_noise(obj: &Obj<'_>) -> Result<Initializer> {
    // Before `vpcf53` the frame was a control point's only when `m_bLocalSpace` said so.
    let transform = if obj.has("m_TransformInput") || obj.bool("m_bLocalSpace", false)? {
        Transform::read(obj, "m_nControlPointNumber", true)?
    } else {
        Transform::Identity
    };
    let flags = |key: &str| -> Result<[bool; 3]> { Ok(obj.vec3(key, [0.0; 3])?.map(|v| v != 0.0)) };
    Ok(Initializer::InitialVelocityNoise {
        min: VectorInput::read(obj, "m_vecOutputMin", [0.0; 3])?,
        max: VectorInput::read(obj, "m_vecOutputMax", [1.0; 3])?,
        scale: FloatInput::read(obj, "m_flNoiseScale", 1.0)?,
        scale_location: FloatInput::read(obj, "m_flNoiseScaleLoc", 0.01)?,
        offset: FloatInput::read(obj, "m_flOffset", 0.0)?,
        offset_location: VectorInput::read(obj, "m_vecOffsetLoc", [0.0; 3])?,
        absolute: flags("m_vecAbsVal")?,
        absolute_inverted: flags("m_vecAbsValInv")?,
        transform,
        ignore_dt: obj.bool("m_bIgnoreDt", false)?,
    })
}

fn ring(obj: &Obj<'_>) -> Result<Ring> {
    Ok(Ring {
        transform: Transform::read(obj, "m_nControlPointNumber", true)?,
        even: obj.bool("m_bEvenDistribution", false)?,
        per_orbit: FloatInput::read(obj, "m_flParticlesPerOrbit", -1.0)?,
        radius: FloatInput::read(obj, "m_flInitialRadius", 0.0)?,
        thickness: FloatInput::read(obj, "m_flThickness", 0.0)?,
        speed_min: FloatInput::read(obj, "m_flInitialSpeedMin", 0.0)?,
        speed_max: FloatInput::read(obj, "m_flInitialSpeedMax", 0.0)?,
        angles: [
            FloatInput::read(obj, "m_flPitch", 0.0)?,
            FloatInput::read(obj, "m_flYaw", 0.0)?,
            FloatInput::read(obj, "m_flRoll", 0.0)?,
        ],
        xy_velocity_only: obj.bool("m_bXYVelocityOnly", true)?,
    })
}

fn count_to_scalar(obj: &Obj<'_>) -> Result<Initializer> {
    Ok(Initializer::RemapParticleCountToScalar {
        field: obj.field("m_nFieldOutput", Field::RADIUS)?,
        input: [obj.int("m_nInputMin", 0)?, obj.int("m_nInputMax", 10)?],
        output: [
            obj.float("m_flOutputMin", 0.0)?,
            obj.float("m_flOutputMax", 1.0)?,
        ],
        active_range: obj.bool("m_bActiveRange", false)?,
        invert: obj.bool("m_bInvert", false)?,
        wrap: obj.bool("m_bWrap", false)?,
        bias: obj.float("m_flRemapBias", 0.5)?,
        set_method: SetMethod::from_index(obj.choice("m_nSetMethod", &SetMethod::NAMES, 0)?),
    })
}

fn age_noise(obj: &Obj<'_>) -> Result<Initializer> {
    Ok(Initializer::AgeNoise {
        absolute: obj.bool("m_bAbsVal", false)?,
        absolute_inverted: obj.bool("m_bAbsValInv", false)?,
        offset: obj.float("m_flOffset", 0.0)?,
        age_min: obj.float("m_flAgeMin", 0.0)?,
        age_max: obj.float("m_flAgeMax", 1.0)?,
        scale: obj.float("m_flNoiseScale", 1.0)?,
        scale_location: obj.float("m_flNoiseScaleLoc", 1.0)?,
        offset_location: obj.vec3("m_vecOffsetLoc", [0.0; 3])?,
    })
}
