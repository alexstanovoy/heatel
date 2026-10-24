//! Initializers: what a particle starts with.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::{Result, malformed};

use super::field::{Field, SetMethod};
use super::input::{FloatInput, VectorInput};
use super::models::OnModel;
use super::noise::CreationNoise;
use super::path::SequentialPath;
use super::read::{MAX_CONTROL_POINT, MAX_NAME, Obj, WHAT};
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
    /// `C_INIT_InitVec`: a vector attribute from an input.
    InitVec {
        /// The attribute.
        field: Field,
        /// The value.
        value: VectorInput,
        /// How it is written.
        set_method: SetMethod,
        /// Normalized.
        normalized: bool,
        /// A position written to the previous position too.
        write_previous: bool,
    },
    /// `C_INIT_PositionPlaceOnGround`: the position moved along a trace to the ground.
    PositionPlaceOnGround(Box<PlaceOnGround>),
    /// `C_INIT_PositionWarp`: the position scaled in a control point's frame.
    PositionWarp(Box<Warp>),
    /// `C_INIT_VelocityFromCP`: a velocity a control point gives.
    VelocityFromCp {
        /// The velocity.
        velocity: VectorInput,
        /// Whose orientation turns it.
        transform: Transform,
        /// Times this.
        scale: f32,
        /// Only its direction.
        direction_only: bool,
    },
    /// `C_INIT_DistanceToCPInit`: an attribute from the distance to a control point.
    DistanceToCp(Box<DistanceToCp>),
    /// `C_INIT_Orient2DRelToCP`: an angle facing away from a control point on the ground
    /// plane.
    Orient2dRelToCp {
        /// The control point.
        control_point: ControlPoint,
        /// The attribute.
        field: Field,
        /// Added to the angle, in degrees.
        offset_degrees: f32,
    },
    /// `C_INIT_CreateFromParentParticles`: the position of one of the parent system's
    /// particles.
    CreateFromParentParticles {
        /// The share of the parent's velocity kept.
        velocity_scale: f32,
        /// Parent particles between one particle and the next.
        increment: f32,
        /// A parent particle at random rather than in order.
        random: bool,
        /// The seed of the random choice (0 the system's).
        random_seed: i32,
        /// The position at the particle's moment within the step rather than at its end.
        sub_frame: bool,
    },
    /// `C_INIT_SetHitboxToModel`: a place on the model at a control point.
    SetHitboxToModel(Box<Hitbox>),
    /// `C_INIT_CreateSequentialPath`: positions one after another along a path.
    CreateSequentialPath(Box<SequentialPath>),
    /// `C_INIT_CreateWithinBox`: a position in a box about a control point.
    CreateWithinBox(Box<WithinBox>),
    /// `C_INIT_CreateInEpitrochoid`: a position on an epitrochoid (the curve a point of a
    /// circle rolling around another draws) by the particle's moment or number.
    CreateInEpitrochoid(Box<Epitrochoid>),
    /// `C_INIT_CreationNoise`: an attribute from noise at the particle's place and moment.
    CreationNoise(Box<CreationNoise>),
    /// `C_INIT_RandomSecondSequence`: a second sprite sheet sequence at random.
    RandomSecondSequence {
        /// The lowest.
        min: i32,
        /// The highest.
        max: i32,
    },
    /// `C_INIT_RemapTransformToVector` (`C_INIT_RemapCPtoVector` before `vpcf46`): a vector
    /// attribute from a transform's position, remapped per axis.
    RemapTransformToVector(Box<TransformToVector>),
    /// `C_INIT_NormalAlignToCP`: the normal along an axis of a transform.
    NormalAlignToCp {
        /// The transform.
        transform: Transform,
        /// Its axis: 0..2 X, Y, Z, 3..5 the same negated.
        axis: u8,
    },
    /// `C_INIT_InheritFromParentParticles`: an attribute from one of the parent system's
    /// particles.
    InheritFromParentParticles {
        /// The copy's scale.
        scale: f32,
        /// The attribute.
        field: Field,
        /// Parent particles between one particle and the next.
        increment: i32,
        /// A parent particle at random rather than in order.
        random: bool,
        /// The seed of the random choice (0 the system's).
        random_seed: i32,
        /// The parent particle's index and number are written (behaviour version 9 on).
        write_parent: bool,
    },
    /// `C_INIT_RemapInitialDirectionToTransformToVector`
    /// (`C_INIT_RemapInitialDirectionToCPToVector` before `vpcf47`): a vector attribute from
    /// the direction from a transform to the particle, turned about an axis.
    RemapInitialDirectionToTransformToVector {
        /// The transform.
        transform: Transform,
        /// The attribute.
        field: Field,
        /// Times this.
        scale: f32,
        /// The turn, in degrees.
        offset_rotation_degrees: f32,
        /// The axis of the turn.
        offset_axis: [f32; 3],
        /// Of unit length.
        normalize: bool,
    },
    /// `C_INIT_CreateOnModel`: a position in the hitboxes of the model a control point carries.
    CreateOnModel(Box<OnModel>),
    /// A class this crate does not read.
    Unsupported(String),
}

/// `C_INIT_CreateWithinBox`' parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WithinBox {
    /// The low corner.
    pub min: VectorInput,
    /// The high corner.
    pub max: VectorInput,
    /// The control point it is about.
    pub control_point: ControlPoint,
    /// In the control point's frame.
    pub local: bool,
    /// Spread evenly (a low-discrepancy sequence keyed by the particle's number and this seed;
    /// a negative seed is offset by the system's) rather than at random, if so.
    pub even: Option<i32>,
}

/// `C_INIT_CreateInEpitrochoid`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Epitrochoid {
    /// The two axes the curve lies in, 0..2 (none: that axis stays 0).
    pub components: [Option<u8>; 2],
    /// Its centre.
    pub transform: Transform,
    /// Radians of the curve per second of creation time (or per particle).
    pub density: FloatInput,
    /// The drawing point's distance from the rolling circle's centre.
    pub offset: FloatInput,
    /// The fixed circle's radius.
    pub radius1: FloatInput,
    /// The rolling circle's radius.
    pub radius2: FloatInput,
    /// By the particle's number rather than its creation time.
    pub use_count: bool,
    /// In the transform's frame.
    pub local: bool,
    /// Added to the position rather than set.
    pub offset_existing: bool,
}

/// `C_INIT_RemapTransformToVector`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TransformToVector {
    /// The attribute.
    pub field: Field,
    /// The positions mapped, per axis.
    pub input: [[f32; 3]; 2],
    /// To these values, clamped.
    pub output: [[f32; 3]; 2],
    /// The transform read.
    pub transform: Transform,
    /// A transform whose orientation turns the output, if any.
    pub local: Option<Transform>,
    /// Only particles created within this window of the system's age, if any.
    pub window: Option<[f32; 2]>,
    /// How it is written.
    pub set_method: SetMethod,
    /// Added to the attribute (and the previous position).
    pub offset: bool,
    /// As a velocity: times the step.
    pub accelerate: bool,
    /// A bias of the output (0.5 none).
    pub bias: f32,
}

/// What a trace that does not reach the ground does (`ParticleTraceMissBehavior_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TraceMiss {
    /// Nothing changes.
    Nothing,
    /// The particle dies.
    Kill,
    /// The particle goes to the trace's end.
    #[default]
    TraceEnd,
}

impl TraceMiss {
    const NAMES: [&str; 3] = [
        "PARTICLE_TRACE_MISS_BEHAVIOR_NONE",
        "PARTICLE_TRACE_MISS_BEHAVIOR_KILL",
        "PARTICLE_TRACE_MISS_BEHAVIOR_TRACE_END",
    ];

    pub(super) fn read(obj: &Obj<'_>) -> Result<Self> {
        Ok([Self::Nothing, Self::Kill, Self::TraceEnd]
            [obj.choice("m_nTraceMissBehavior", &Self::NAMES, 2)?])
    }
}

/// `C_INIT_PositionPlaceOnGround`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PlaceOnGround {
    /// How far above the ground it stays.
    pub offset: FloatInput,
    /// The longest trace.
    pub max_trace_length: FloatInput,
    /// Which way the trace goes.
    pub direction: VectorInput,
    /// What a trace that misses does.
    pub miss: TraceMiss,
    /// The attribute moved.
    pub field: Field,
    /// Only the previous position moves.
    pub previous_only: bool,
    /// The ground's normal is written.
    pub set_normal: bool,
    /// Where the normal goes.
    pub normal: Field,
    /// The radius times this is added to the offset.
    pub offset_by_radius: f32,
    /// A control point whose height above the start is kept as offset, if any.
    pub preserve_offset: Option<ControlPoint>,
}

/// `C_INIT_PositionWarp`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Warp {
    /// The scale at the warp's start, per axis.
    pub min: VectorInput,
    /// At its end.
    pub max: VectorInput,
    /// The frame's control point.
    pub control_point: ControlPoint,
    /// A control point whose position scales the warp, if any.
    pub scale_control_point: Option<ControlPoint>,
    /// The warp's component that scales the radius, if any.
    pub radius_component: Option<u8>,
    /// Over how long the warp goes from its start to its end (0: a random warp between them).
    pub time: f32,
    /// When it starts.
    pub start_time: f32,
    /// The previous position's warp is times this.
    pub previous_scale: f32,
    /// From end to start.
    pub invert: bool,
    /// The particles made so far count as the time.
    pub use_count: bool,
}

/// `C_INIT_DistanceToCPInit`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DistanceToCp {
    /// The attribute.
    pub field: Field,
    /// The distances mapped.
    pub input: [FloatInput; 2],
    /// To these values.
    pub output: [FloatInput; 2],
    /// The control point.
    pub control_point: ControlPoint,
    /// How it is written.
    pub set_method: SetMethod,
    /// Only distances within the input.
    pub active_range: bool,
    /// The distance's scale per axis.
    pub distance_scale: [f32; 3],
    /// A bias of the output (0.5 none).
    pub bias: f32,
}

/// `C_INIT_SetHitboxToModel`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Hitbox {
    /// The model's control point.
    pub control_point: ControlPoint,
    /// The hitbox set.
    pub set: String,
    /// One hitbox rather than any (-1 any).
    pub desired: i32,
    /// The hitboxes' scale.
    pub scale: VectorInput,
    /// A bias of the place within them.
    pub direction_bias: [f32; 3],
    /// The share of the hitbox's depth the place is within, from its surface.
    pub shell_size: FloatInput,
    /// The bones rather than the hitboxes.
    pub use_bones: bool,
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
    pub(super) fn read(
        class: &str,
        obj: &Obj<'_>,
        format: u32,
        behavior_version: i32,
    ) -> Result<Self> {
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
            other => return Self::read_more(other, obj, behavior_version),
        })
    }

    /// The initializers of shadows, souls and the ground.
    fn read_more(class: &str, obj: &Obj<'_>, behavior_version: i32) -> Result<Self> {
        Ok(match class {
            "C_INIT_InitVec" => Self::InitVec {
                field: obj.field("m_nOutputField", Field::COLOR)?,
                value: VectorInput::read(obj, "m_InputValue", [0.0; 3])?,
                set_method: SetMethod::from_index(obj.choice(
                    "m_nSetMethod",
                    &SetMethod::NAMES,
                    0,
                )?),
                normalized: obj.bool("m_bNormalizedOutput", false)?,
                write_previous: obj.bool("m_bWritePreviousPosition", false)?,
            },
            "C_INIT_PositionPlaceOnGround" => Self::PositionPlaceOnGround(Box::new(ground(obj)?)),
            "C_INIT_PositionWarp" => Self::PositionWarp(Box::new(warp(obj)?)),
            "C_INIT_VelocityFromCP" => velocity_from_cp(obj)?,
            "C_INIT_DistanceToCPInit" if !obj.bool("m_bLOS", false)? => {
                Self::DistanceToCp(Box::new(distance_to_cp(obj)?))
            }
            "C_INIT_Orient2DRelToCP" => Self::Orient2dRelToCp {
                control_point: obj.control_point("m_nCP", 0)?,
                field: obj.field("m_nFieldOutput", Field::ROLL)?,
                offset_degrees: obj.float("m_flRotOffset", 0.0)?,
            },
            "C_INIT_CreateFromParentParticles" => Self::CreateFromParentParticles {
                velocity_scale: obj.float("m_flVelocityScale", 0.0)?,
                increment: obj.float("m_flIncrement", 1.0)?.clamp(-1e4, 1e4),
                random: obj.bool("m_bRandomDistribution", false)?,
                random_seed: obj.int_in("m_nRandomSeed", 0, -(1 << 31), (1 << 31) - 1)? as i32,
                sub_frame: obj.bool("m_bSubFrame", true)?,
            },
            "C_INIT_SetHitboxToModel" => Self::SetHitboxToModel(Box::new(hitbox(obj)?)),
            other => return Self::read_shapes(other, obj, behavior_version),
        })
    }

    /// The initializers of paths, boxes, curves and noise, the town portal's and the missiles'.
    fn read_shapes(class: &str, obj: &Obj<'_>, behavior_version: i32) -> Result<Self> {
        Ok(match class {
            "C_INIT_CreateSequentialPath" => {
                Self::CreateSequentialPath(Box::new(SequentialPath::read(obj)?))
            }
            "C_INIT_CreateWithinBox" => Self::CreateWithinBox(Box::new(within_box(obj)?)),
            "C_INIT_CreateInEpitrochoid" => Self::CreateInEpitrochoid(Box::new(epitrochoid(obj)?)),
            "C_INIT_CreationNoise" => Self::CreationNoise(Box::new(CreationNoise::read(obj)?)),
            "C_INIT_RandomSecondSequence" => Self::RandomSecondSequence {
                min: obj.int_in("m_nSequenceMin", 0, 0, 1 << 16)? as i32,
                max: obj.int_in("m_nSequenceMax", 0, 0, 1 << 16)? as i32,
            },
            "C_INIT_RemapTransformToVector" | "C_INIT_RemapCPtoVector" => {
                Self::RemapTransformToVector(Box::new(transform_to_vector(obj)?))
            }
            // Before `vpcf57` the transform was the control point `m_nControlPointNumber`.
            "C_INIT_NormalAlignToCP" => Self::NormalAlignToCp {
                transform: Transform::read_member(
                    obj,
                    "m_transformInput",
                    "m_nControlPointNumber",
                    true,
                )?,
                axis: obj.choice("m_nControlPointAxis", &AXES, 0)? as u8,
            },
            "C_INIT_InheritFromParentParticles" => Self::InheritFromParentParticles {
                scale: obj.float("m_flScale", 1.0)?,
                field: obj.field("m_nFieldOutput", Field::RADIUS)?,
                increment: obj.int_in("m_nIncrement", 1, -(1 << 16), 1 << 16)? as i32,
                random: obj.bool("m_bRandomDistribution", false)?,
                random_seed: obj.int_in("m_nRandomSeed", 0, -(1 << 31), (1 << 31) - 1)? as i32,
                write_parent: behavior_version >= 9,
            },
            // Before `vpcf47` the transform was the control point `m_nCP`.
            "C_INIT_RemapInitialDirectionToTransformToVector"
            | "C_INIT_RemapInitialDirectionToCPToVector" => {
                Self::RemapInitialDirectionToTransformToVector {
                    transform: Transform::read_member(obj, "m_TransformInput", "m_nCP", true)?,
                    field: obj.field("m_nFieldOutput", Field::POSITION)?,
                    scale: obj.float("m_flScale", 1.0)?,
                    offset_rotation_degrees: obj.float("m_flOffsetRot", 0.0)?,
                    offset_axis: obj.vec3("m_vecOffsetAxis", [0.0; 3])?,
                    normalize: obj.bool("m_bNormalize", false)?,
                }
            }
            "C_INIT_CreateOnModel" => match OnModel::read(obj)? {
                Some(on_model) => Self::CreateOnModel(Box::new(on_model)),
                None => Self::Unsupported(class.to_owned()),
            },
            other => Self::Unsupported(other.to_owned()),
        })
    }
}

/// A control point's axes, in `ParticleControlPointAxis_t` order.
const AXES: [&str; 6] = [
    "PARTICLE_CP_AXIS_X",
    "PARTICLE_CP_AXIS_Y",
    "PARTICLE_CP_AXIS_Z",
    "PARTICLE_CP_AXIS_NEGATIVE_X",
    "PARTICLE_CP_AXIS_NEGATIVE_Y",
    "PARTICLE_CP_AXIS_NEGATIVE_Z",
];

fn within_box(obj: &Obj<'_>) -> Result<WithinBox> {
    let even = match obj.object("m_randomnessParameters")? {
        Some(r) if r.bool("m_bDistributeEvenly", false)? => {
            Some(r.int_in("m_nSeed", 0, -(1 << 31), (1 << 31) - 1)? as i32)
        }
        _ => None,
    };
    Ok(WithinBox {
        min: VectorInput::read(obj, "m_vecMin", [0.0; 3])?,
        max: VectorInput::read(obj, "m_vecMax", [0.0; 3])?,
        control_point: obj.control_point("m_nControlPointNumber", 0)?,
        local: obj.bool("m_bLocalSpace", false)?,
        even,
    })
}

fn epitrochoid(obj: &Obj<'_>) -> Result<Epitrochoid> {
    let component = |key: &str, default: i64| -> Result<Option<u8>> {
        Ok(u8::try_from(obj.int_clamped(key, default, -1, 2)?).ok())
    };
    Ok(Epitrochoid {
        components: [
            component("m_nComponent1", 0)?,
            component("m_nComponent2", 1)?,
        ],
        // Before `vpcf55` the centre was the control point `m_nControlPointNumber`.
        transform: Transform::read(obj, "m_nControlPointNumber", true)?,
        density: FloatInput::read(obj, "m_flParticleDensity", 10.0)?,
        offset: FloatInput::read(obj, "m_flOffset", 4.0)?,
        radius1: FloatInput::read(obj, "m_flRadius1", 40.0)?,
        radius2: FloatInput::read(obj, "m_flRadius2", 24.0)?,
        use_count: obj.bool("m_bUseCount", false)?,
        local: obj.bool("m_bUseLocalCoords", false)?,
        offset_existing: obj.bool("m_bOffsetExistingPos", false)?,
    })
}

fn transform_to_vector(obj: &Obj<'_>) -> Result<TransformToVector> {
    // Before `vpcf46` the transform was the control point `m_nCPInput`, and a local frame the
    // control point `m_nLocalSpaceCP` when set.
    let local = if obj.has("m_LocalSpaceTransform") {
        match Transform::read_member(obj, "m_LocalSpaceTransform", "", true)? {
            Transform::Identity => None,
            transform => Some(transform),
        }
    } else {
        let cp = obj.int_in("m_nLocalSpaceCP", -1, -1, i64::from(MAX_CONTROL_POINT))?;
        u8::try_from(cp)
            .ok()
            .and_then(ControlPoint::new)
            .map(Transform::point)
    };
    let (start, end) = (
        obj.float("m_flStartTime", -1.0)?,
        obj.float("m_flEndTime", -1.0)?,
    );
    Ok(TransformToVector {
        field: obj.field("m_nFieldOutput", Field::POSITION)?,
        input: [
            obj.vec3("m_vInputMin", [0.0; 3])?,
            obj.vec3("m_vInputMax", [0.0; 3])?,
        ],
        output: [
            obj.vec3("m_vOutputMin", [0.0; 3])?,
            obj.vec3("m_vOutputMax", [0.0; 3])?,
        ],
        transform: Transform::read(obj, "m_nCPInput", true)?,
        local,
        window: (start != -1.0 && end != -1.0).then_some([start, end]),
        set_method: SetMethod::from_index(obj.choice("m_nSetMethod", &SetMethod::NAMES, 0)?),
        offset: obj.bool("m_bOffset", false)?,
        accelerate: obj.bool("m_bAccelerate", false)?,
        bias: obj.float("m_flRemapBias", 0.5)?,
    })
}

fn ground(obj: &Obj<'_>) -> Result<PlaceOnGround> {
    let set_normal = obj.bool("m_bSetNormal", false)?;
    let mut normal = obj.field("m_nGroundNormalAttribute", Field::NORMAL)?;
    let mut direction = VectorInput::read(obj, "m_vecTraceDir", [0.0, 0.0, -1.0])?;
    // Before `vpcf64` a trace along an attribute was a flag and that attribute.
    if obj.bool("m_bTraceAlongNormal", false)? {
        let along = obj.field("m_nTraceDirectionAttribute", Field::NORMAL)?;
        if set_normal {
            normal = along;
        }
        direction = VectorInput::ParticleVector {
            field: along,
            initial: false,
            scale: [1.0; 3],
        };
    }
    let preserve = obj.int_in("m_nPreserveOffsetCP", -1, -1, i64::from(MAX_CONTROL_POINT))?;
    Ok(PlaceOnGround {
        offset: FloatInput::read(obj, "m_flOffset", 0.0)?,
        max_trace_length: FloatInput::read(obj, "m_flMaxTraceLength", 128.0)?,
        direction,
        miss: TraceMiss::read(obj)?,
        field: obj.field("m_nAttribute", Field::POSITION)?,
        previous_only: obj.bool("m_bSetPXYZOnly", false)?,
        set_normal,
        normal,
        offset_by_radius: obj.float("m_flOffsetByRadiusFactor", 0.0)?,
        preserve_offset: u8::try_from(preserve).ok().and_then(ControlPoint::new),
    })
}

fn warp(obj: &Obj<'_>) -> Result<Warp> {
    let scale = obj.int_in(
        "m_nScaleControlPointNumber",
        -1,
        -1,
        i64::from(MAX_CONTROL_POINT),
    )?;
    let radius = obj.int_clamped("m_nRadiusComponent", -1, -1, 2)?;
    Ok(Warp {
        min: VectorInput::read(obj, "m_vecWarpMin", [1.0; 3])?,
        max: VectorInput::read(obj, "m_vecWarpMax", [1.0; 3])?,
        control_point: obj.control_point("m_nControlPointNumber", 0)?,
        scale_control_point: u8::try_from(scale).ok().and_then(ControlPoint::new),
        radius_component: u8::try_from(radius).ok(),
        time: obj.float("m_flWarpTime", 0.0)?,
        start_time: obj.float("m_flWarpStartTime", 0.0)?,
        previous_scale: obj.float("m_flPrevPosScale", 1.0)?,
        invert: obj.bool("m_bInvertWarp", false)?,
        use_count: obj.bool("m_bUseCount", false)?,
    })
}

fn velocity_from_cp(obj: &Obj<'_>) -> Result<Initializer> {
    // Before `vpcf57` the velocity was a control point's value, or its difference from a
    // second one, in the frame of a third.
    let (velocity, transform) = if obj.has("m_velocityInput") {
        let transform = if obj.has("m_transformInput") {
            Transform::read_member(obj, "m_transformInput", "m_nControlPointLocal", true)?
        } else {
            Transform::Identity
        };
        (
            VectorInput::read(obj, "m_velocityInput", [0.0; 3])?,
            transform,
        )
    } else {
        let point = |key: &str, default: i64| -> Result<Option<ControlPoint>> {
            let index = obj.int(key, default)?;
            Ok(u8::try_from(index).ok().and_then(ControlPoint::new))
        };
        let velocity = match (
            point("m_nControlPoint", 0)?,
            point("m_nControlPointCompare", -1)?,
        ) {
            (Some(control_point), None) => VectorInput::ControlPointValue {
                control_point,
                scale: [1.0; 3],
            },
            (Some(control_point), Some(delta)) => VectorInput::ControlPointDelta {
                control_point,
                delta,
            },
            (None, _) => VectorInput::Literal([0.0; 3]),
        };
        let transform =
            point("m_nControlPointLocal", -1)?.map_or(Transform::Identity, Transform::point);
        (velocity, transform)
    };
    Ok(Initializer::VelocityFromCp {
        velocity,
        transform,
        scale: obj.float("m_flVelocityScale", 1.0)?,
        direction_only: obj.bool("m_bDirectionOnly", false)?,
    })
}

fn distance_to_cp(obj: &Obj<'_>) -> Result<DistanceToCp> {
    Ok(DistanceToCp {
        field: obj.field("m_nFieldOutput", Field::RADIUS)?,
        input: [
            FloatInput::read(obj, "m_flInputMin", 0.0)?,
            FloatInput::read(obj, "m_flInputMax", 128.0)?,
        ],
        output: [
            FloatInput::read(obj, "m_flOutputMin", 0.0)?,
            FloatInput::read(obj, "m_flOutputMax", 1.0)?,
        ],
        control_point: obj.control_point("m_nStartCP", 0)?,
        set_method: SetMethod::from_index(obj.choice("m_nSetMethod", &SetMethod::NAMES, 0)?),
        active_range: obj.bool("m_bActiveRange", false)?,
        distance_scale: obj.vec3("m_vecDistanceScale", [1.0; 3])?,
        bias: obj.float("m_flRemapBias", 0.5)?,
    })
}

fn hitbox(obj: &Obj<'_>) -> Result<Hitbox> {
    let set = obj.str("m_HitboxSetName")?.unwrap_or("default");
    if set.len() > MAX_NAME {
        return Err(malformed(WHAT, "a hitbox set's name is too long"));
    }
    Ok(Hitbox {
        control_point: obj.control_point("m_nControlPointNumber", 0)?,
        set: set.to_owned(),
        desired: obj.int_in("m_nDesiredHitbox", -1, -1, 1 << 16)? as i32,
        scale: VectorInput::read(obj, "m_vecHitBoxScale", [1.0; 3])?,
        direction_bias: obj.vec3("m_vecDirectionBias", [0.0; 3])?,
        shell_size: FloatInput::read(obj, "m_flShellSize", 1.0)?,
        use_bones: obj.bool("m_bUseBones", false)?,
    })
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
