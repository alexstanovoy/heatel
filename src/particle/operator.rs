//! Operators: what a particle does each step; force generators: what pushes it.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::field::{Field, SetMethod};
use super::initializer::TraceMiss;
use super::input::{FloatInput, VectorInput};
use super::read::{MAX_CONTROL_POINT, Obj};
use super::transform::{ControlPoint, Transform};

/// An operator class.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Operator {
    /// `C_OP_BasicMovement`: Verlet motion under gravity, drag and the system's forces.
    BasicMovement {
        /// Gravity.
        gravity: VectorInput,
        /// The share of the speed lost each thirtieth of a second.
        drag: FloatInput,
    },
    /// `C_OP_Decay`: kills a particle past its lifetime.
    Decay {
        /// Kills a rope's particle only with the two after it.
        rope: bool,
    },
    /// `C_OP_InterpolateRadius`: the radius from a scale of the initial one to another.
    InterpolateRadius {
        /// The window, as shares of the lifetime.
        time: [f32; 2],
        /// The scale at its start.
        start_scale: FloatInput,
        /// The scale at its end.
        end_scale: FloatInput,
        /// Smoothstep rather than the bias.
        ease: bool,
        /// The bias (0.5 linear).
        bias: f32,
    },
    /// `C_OP_PositionLock`: particles follow a transform's moves.
    PositionLock(Box<PositionLock>),
    /// `C_OP_FadeInSimple`: an attribute fades in over a share of the lifetime.
    FadeInSimple {
        /// The share.
        time: f32,
        /// The attribute.
        field: Field,
    },
    /// `C_OP_FadeOutSimple`: an attribute fades out over a share of the lifetime.
    FadeOutSimple {
        /// The share.
        time: f32,
        /// The attribute.
        field: Field,
    },
    /// `C_OP_FadeOut`: the alpha fades out over a random time.
    FadeOut {
        /// The time's low end.
        min: f32,
        /// High end.
        max: f32,
        /// The exponent of the draw.
        exponent: f32,
        /// Times as shares of the lifetime.
        proportional: bool,
        /// Smoothstep.
        ease: bool,
    },
    /// `C_OP_FadeAndKill`: the alpha fades in and out over the lifetime, the particle dies at its
    /// end.
    FadeAndKill {
        /// When the fade in starts and ends.
        fade_in: [f32; 2],
        /// When the fade out starts and ends.
        fade_out: [f32; 2],
        /// The alpha's share at the start.
        start_alpha: f32,
        /// The alpha's share at the end.
        end_alpha: f32,
    },
    /// `C_OP_ColorInterpolate`: the colour moves to another.
    ColorInterpolate {
        /// The colour reached.
        fade: [f32; 3],
        /// The window, as shares of the lifetime.
        time: [f32; 2],
        /// The attribute.
        field: Field,
        /// Smoothstep.
        ease: bool,
    },
    /// `C_OP_RampScalarLinearSimple`: an attribute changes at a rate within a window.
    RampScalarLinearSimple {
        /// Per second.
        rate: f32,
        /// The window, as shares of the lifetime.
        time: [f32; 2],
        /// The attribute.
        field: Field,
    },
    /// `C_OP_RampScalarLinear`: an attribute changes at a random rate within a random window.
    RampScalarLinear {
        /// The rate's range.
        rate: [f32; 2],
        /// The window's start range.
        start: [f32; 2],
        /// The window's end range.
        end: [f32; 2],
        /// The window as shares of the lifetime.
        proportional: bool,
        /// The attribute.
        field: Field,
    },
    /// `C_OP_RampScalarSpline`: an attribute changes at a random rate, biased over a random
    /// window.
    RampScalarSpline {
        /// The rate's range.
        rate: [f32; 2],
        /// The window's start range.
        start: [f32; 2],
        /// The window's end range.
        end: [f32; 2],
        /// The bias over the window (0.5 linear).
        bias: f32,
        /// The window as shares of the lifetime.
        proportional: bool,
        /// The rate rises to the window's middle and falls back.
        ease_out: bool,
        /// The attribute.
        field: Field,
    },
    /// `C_OP_SpinUpdate`: the roll turns at the roll speed.
    SpinUpdate,
    /// `C_OP_Spin`: the roll turns at a rate.
    Spin {
        /// The rate.
        rate_degrees: i32,
        /// The least rate.
        min_rate_degrees: i32,
        /// The share of the lifetime it slows to a stop over (0 never).
        stop_time: f32,
    },
    /// `C_OP_OscillateVector`: a vector attribute oscillates.
    OscillateVector(Box<Oscillate>),
    /// `C_OP_VectorNoise`: a vector attribute from noise at the particle's position.
    VectorNoise {
        /// The attribute.
        field: Field,
        /// The output's low corner.
        min: [f32; 3],
        /// High corner.
        max: [f32; 3],
        /// Noise frequency in space.
        scale: f32,
        /// Noise frequency in time.
        time_scale: f32,
        /// Added per second rather than set.
        additive: bool,
    },
    /// `C_OP_SetFloat`: an attribute from an input each step.
    SetFloat {
        /// The attribute.
        field: Field,
        /// The value.
        value: FloatInput,
        /// How it is written.
        set_method: SetMethod,
        /// How far it moves to it.
        lerp: FloatInput,
    },
    /// `C_OP_SetVec`: a vector attribute from an input each step.
    SetVec {
        /// The attribute.
        field: Field,
        /// The value.
        value: VectorInput,
        /// How it is written.
        set_method: SetMethod,
        /// How far it moves to it.
        lerp: FloatInput,
        /// Normalized.
        normalized: bool,
    },
    /// `C_OP_OrientTo2dDirection`: the roll follows the velocity on the ground plane.
    OrientTo2dDirection {
        /// Added to the angle, in degrees.
        offset_degrees: f32,
        /// How far the roll moves to it.
        strength: f32,
        /// The attribute.
        field: Field,
    },
    /// `C_OP_MaxVelocity`: the speed bounded.
    MaxVelocity {
        /// At most this.
        max: FloatInput,
        /// At least this.
        min: FloatInput,
    },
    /// `C_OP_SetChildControlPoints`: the children's control points from particles.
    SetChildControlPoints(ControlPointsFromParticles),
    /// `C_OP_SetControlPointsToParticle`: the system's own control points from particles.
    SetControlPointsToParticle(ControlPointsFromParticles),
    /// `C_OP_SetPerChildControlPoint`: one control point of each child from one particle each.
    SetPerChildControlPoint {
        /// The children's group.
        group: i32,
        /// The control point.
        control_point: ControlPoint,
        /// How many children.
        count: i32,
        /// The particle of the first child.
        first: FloatInput,
        /// Particles between children.
        increment: FloatInput,
        /// As many children as particles.
        per_particle: bool,
        /// Orientation from the velocity.
        orientation: bool,
    },
    /// `C_OP_CPOffsetToPercentageBetweenCPs`: a control point set off from another by how far
    /// a third has come from a start control point to an end one.
    CpOffsetToPercentage(Box<CpOffset>),
    /// `C_OP_AlphaDecay`: kills a particle whose alphas fall to a least one.
    AlphaDecay {
        /// The least alpha.
        min_alpha: f32,
    },
    /// `C_OP_DampenToCP`: particles slow down near a control point.
    DampenToCp {
        /// The control point.
        control_point: ControlPoint,
        /// The distance within which they slow.
        range: f32,
        /// The power of the falloff.
        scale: f32,
    },
    /// `C_OP_LerpEndCapScalar`: an attribute moves to a value over a time from the end cap on.
    LerpEndCapScalar {
        /// The attribute.
        field: Field,
        /// The value.
        output: f32,
        /// The time.
        lerp_time: f32,
    },
    /// `C_OP_Orient2DRelToCP`: an angle turns to face away from a control point on the ground
    /// plane.
    Orient2dRelToCp {
        /// The control point.
        control_point: ControlPoint,
        /// The attribute.
        field: Field,
        /// Added to the angle, in degrees.
        offset_degrees: f32,
        /// How far it turns each step.
        spin_strength: f32,
    },
    /// `C_OP_ClampScalar`: an attribute held within bounds.
    ClampScalar {
        /// The attribute.
        field: Field,
        /// The least value.
        min: FloatInput,
        /// The largest.
        max: FloatInput,
    },
    /// `C_OP_DistanceToTransform` (`C_OP_DistanceToCP` before `vpcf49`): an attribute from the
    /// distance to a transform.
    DistanceToTransform(Box<DistanceToTransform>),
    /// `C_OP_EndCapTimedDecay`: kills every particle a time into the end cap.
    EndCapTimedDecay {
        /// The time.
        decay_time: FloatInput,
    },
    /// `C_OP_MovementPlaceOnGround`: particles kept on the ground below them.
    MovementPlaceOnGround(Box<GroundMovement>),
    /// `C_OP_InheritFromParentParticles`: an attribute copied from the parent system's
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
    },
    /// `C_OP_MoveToHitbox`: particles move to their place on the model at a control point.
    MoveToHitbox {
        /// The model's control point.
        control_point: ControlPoint,
        /// The window of the move, as ages in seconds (an end of 0 never ends).
        lifetime: [f32; 2],
        /// The share of the move the previous position follows.
        previous_scale: f32,
        /// A constant share each step (`interpolation`) rather than one growing over the window.
        constant: bool,
        /// That share.
        interpolation: FloatInput,
    },
    /// A class this crate does not read.
    Unsupported(String),
}

/// `C_OP_DistanceToTransform`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DistanceToTransform {
    /// The attribute.
    pub field: Field,
    /// The distances mapped.
    pub input: [FloatInput; 2],
    /// To these values.
    pub output: [FloatInput; 2],
    /// Where the distance is from.
    pub transform: Transform,
    /// How it is written.
    pub set_method: SetMethod,
    /// Only distances within the input.
    pub active_range: bool,
    /// The value is added to the attribute.
    pub additive: bool,
    /// The distance's scale per axis.
    pub component_scale: VectorInput,
}

/// `C_OP_MovementPlaceOnGround`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GroundMovement {
    /// How far above the ground it stays.
    pub offset: FloatInput,
    /// The longest trace.
    pub max_trace_length: f32,
    /// How far a reference control point moves before the ground is traced again.
    pub tolerance: f32,
    /// Which way the trace goes.
    pub direction: VectorInput,
    /// How far behind the particle the trace starts.
    pub trace_offset: f32,
    /// Over how long a particle moves to the ground found (0 at once).
    pub lerp_rate: f32,
    /// Control points whose moves trace the ground again, if any.
    pub references: [Option<ControlPoint>; 2],
    /// A control point whose move over the tolerance is the share of the move to the ground,
    /// if any.
    pub lerp_control_point: Option<ControlPoint>,
    /// What a trace that misses does.
    pub miss: TraceMiss,
    /// The ground's normal is written, and the offset is along it.
    pub set_normal: bool,
    /// The offset is times the radius.
    pub scale_offset: bool,
    /// A control point whose height below the particle's start is kept as offset, if any.
    pub preserve_offset: Option<ControlPoint>,
}

/// `C_OP_CPOffsetToPercentageBetweenCPs`' parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CpOffset {
    /// Where the way starts.
    pub start: ControlPoint,
    /// Where it ends.
    pub end: ControlPoint,
    /// What has come along it.
    pub input: ControlPoint,
    /// What the output is set off from.
    pub origin: ControlPoint,
    /// What is set.
    pub output: ControlPoint,
    /// The share of the way mapped to 0..1 (may run backwards).
    pub input_range: [f32; 2],
    /// The bias of that mapping (0.5 linear).
    pub bias: f32,
    /// The share is the distance from the start over the way's length, not the projection on it.
    pub radial: bool,
    /// The offset is in shares of the way's length.
    pub scale_offset: bool,
    /// The offset at a share of 1.
    pub offset: [f32; 3],
}
/// `C_OP_PositionLock`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PositionLock {
    /// What is followed.
    pub transform: Transform,
    /// The window's start, low and high and the draw's exponent, as shares of the lifetime
    /// (1 or more: locked for life).
    pub start: [f32; 3],
    /// The window's end, likewise.
    pub end: [f32; 3],
    /// The distance over which the lock fades (0 none).
    pub range: f32,
    /// The bias of that fade.
    pub range_bias: FloatInput,
    /// A move longer than this is followed whole, as a teleport.
    pub jump_threshold: f32,
    /// The share of the move the previous position follows.
    pub previous_scale: f32,
    /// Rotations are followed too.
    pub lock_rotation: bool,
    /// The move's scale per axis.
    pub scale: VectorInput,
    /// The attribute moved.
    pub output: Field,
    /// Its previous value moved with it.
    pub output_previous: Field,
}

/// `C_OP_OscillateVector`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Oscillate {
    /// The attribute.
    pub field: Field,
    /// The rate's low corner.
    pub rate_min: [f32; 3],
    /// High corner.
    pub rate_max: [f32; 3],
    /// The frequency's low corner.
    pub frequency_min: [f32; 3],
    /// High corner.
    pub frequency_max: [f32; 3],
    /// The phase's multiplier.
    pub multiplier: FloatInput,
    /// The phase's offset.
    pub offset: FloatInput,
    /// The rate's scale.
    pub rate_scale: FloatInput,
    /// The phase follows the share of the lifetime rather than the system's age.
    pub proportional: bool,
    /// The window is a share of the lifetime.
    pub proportional_window: bool,
    /// The window's start range.
    pub start: [f32; 2],
    /// The window's end range.
    pub end: [f32; 2],
    /// A position's oscillation moves its previous position too.
    pub offset_position: bool,
}

/// Which control points take which particles' positions.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ControlPointsFromParticles {
    /// The children's group, for children's control points.
    pub group: i32,
    /// The first control point set.
    pub first_control_point: ControlPoint,
    /// How many.
    pub count: u8,
    /// The particle of the first.
    pub first_source: FloatInput,
    /// Counted from the last particle.
    pub reverse: bool,
    /// Orientation from the velocity.
    pub orientation: bool,
}

/// A force generator class.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Force {
    /// `C_OP_AttractToControlPoint`: a pull to a transform falling off with distance.
    AttractToControlPoint {
        /// What pulls.
        transform: Transform,
        /// The force.
        amount: FloatInput,
        /// The falloff's power.
        falloff: f32,
        /// Closer than this the force is that at distance 1.
        min_distance: FloatInput,
        /// The force per axis.
        component_scale: [f32; 3],
    },
    /// `C_OP_RandomForce`: a random push each step.
    RandomForce {
        /// Low corner.
        min: [f32; 3],
        /// High corner.
        max: [f32; 3],
    },
    /// `C_OP_TwistAroundAxis`: a push around an axis through a control point.
    TwistAroundAxis {
        /// The push.
        amount: f32,
        /// The axis.
        axis: [f32; 3],
        /// The axis is in a control point's frame.
        local: bool,
        /// The control point the axis goes through.
        control_point: ControlPoint,
        /// The control point whose frame a local axis is in (0 below behaviour version 3).
        axis_control_point: ControlPoint,
    },
    /// A class this crate does not read.
    Unsupported(String),
}

impl Operator {
    pub(super) fn read(class: &str, obj: &Obj<'_>) -> Result<Self> {
        if let Some(op) = Self::read_life(class, obj)? {
            return Ok(op);
        }
        if let Some(op) = Self::read_motion(class, obj)? {
            return Ok(op);
        }
        if let Some(op) = Self::read_end(class, obj)? {
            return Ok(op);
        }
        if let Some(op) = Self::read_follow(class, obj)? {
            return Ok(op);
        }
        Ok(match class {
            "C_OP_SetFloat" => Self::SetFloat {
                field: obj.field("m_nOutputField", Field::RADIUS)?,
                value: FloatInput::read(obj, "m_InputValue", 0.0)?,
                set_method: SetMethod::from_index(obj.choice(
                    "m_nSetMethod",
                    &SetMethod::NAMES,
                    0,
                )?),
                lerp: FloatInput::read(obj, "m_Lerp", 1.0)?,
            },
            "C_OP_SetVec" => Self::SetVec {
                field: obj.field("m_nOutputField", Field::COLOR)?,
                value: VectorInput::read(obj, "m_InputValue", [0.0; 3])?,
                set_method: SetMethod::from_index(obj.choice(
                    "m_nSetMethod",
                    &SetMethod::NAMES,
                    0,
                )?),
                lerp: FloatInput::read(obj, "m_Lerp", 1.0)?,
                normalized: obj.bool("m_bNormalizedOutput", false)?,
            },
            "C_OP_VectorNoise" => Self::VectorNoise {
                field: obj.field("m_nFieldOutput", Field::COLOR)?,
                min: obj.vec3("m_vecOutputMin", [0.0; 3])?,
                max: obj.vec3("m_vecOutputMax", [1.0; 3])?,
                scale: obj.float("m_fl4NoiseScale", 0.1)?,
                time_scale: obj.float("m_flNoiseAnimationTimeScale", 0.0)?,
                additive: obj.bool("m_bAdditive", false)?,
            },
            "C_OP_OscillateVector" => Self::OscillateVector(Box::new(oscillate(obj)?)),
            "C_OP_SetChildControlPoints" => Self::SetChildControlPoints(from_particles(obj)?),
            "C_OP_SetControlPointsToParticle" if !obj.has("m_nSetParent") => {
                Self::SetControlPointsToParticle(from_particles(obj)?)
            }
            "C_OP_SetPerChildControlPoint" => per_child(obj)?,
            "C_OP_CPOffsetToPercentageBetweenCPs" => {
                Self::CpOffsetToPercentage(Box::new(CpOffset {
                    start: obj.control_point("m_nStartCP", 0)?,
                    end: obj.control_point("m_nEndCP", 1)?,
                    input: obj.control_point("m_nInputCP", 3)?,
                    origin: obj.control_point("m_nOffsetCP", 2)?,
                    output: obj.control_point("m_nOuputCP", 4)?,
                    input_range: [
                        obj.float("m_flInputMin", 0.0)?,
                        obj.float("m_flInputMax", 1.0)?,
                    ],
                    bias: obj.float("m_flInputBias", 0.5)?,
                    radial: obj.bool("m_bRadialCheck", true)?,
                    scale_offset: obj.bool("m_bScaleOffset", false)?,
                    offset: obj.vec3("m_vecOffset", [0.0; 3])?,
                }))
            }
            other => Self::Unsupported(other.to_owned()),
        })
    }

    /// The operators of a particle's life: its fades, ramps and death.
    fn read_life(class: &str, obj: &Obj<'_>) -> Result<Option<Self>> {
        Ok(Some(match class {
            "C_OP_Decay" => Self::Decay {
                rope: obj.bool("m_bRopeDecay", false)?,
            },
            "C_OP_InterpolateRadius" => Self::InterpolateRadius {
                time: [
                    obj.float("m_flStartTime", 0.0)?,
                    obj.float("m_flEndTime", 1.0)?,
                ],
                start_scale: FloatInput::read(obj, "m_flStartScale", 1.0)?,
                end_scale: FloatInput::read(obj, "m_flEndScale", 1.0)?,
                ease: obj.bool("m_bEaseInAndOut", false)?,
                bias: Some(obj.float("m_flBias", 0.5)?)
                    .filter(|b| *b != 0.0)
                    .unwrap_or(0.5),
            },
            "C_OP_FadeInSimple" => Self::FadeInSimple {
                time: obj.float("m_flFadeInTime", 0.25)?,
                field: obj.field("m_nFieldOutput", Field::ALPHA)?,
            },
            "C_OP_FadeOutSimple" => Self::FadeOutSimple {
                time: obj.float("m_flFadeOutTime", 0.25)?,
                field: obj.field("m_nFieldOutput", Field::ALPHA)?,
            },
            "C_OP_FadeOut" => fade_out(obj)?,
            "C_OP_FadeAndKill" => fade_and_kill(obj)?,
            "C_OP_ColorInterpolate" => Self::ColorInterpolate {
                fade: obj.color("m_ColorFade", [1.0; 3])?,
                time: [
                    obj.float("m_flFadeStartTime", 0.0)?,
                    obj.float("m_flFadeEndTime", 1.0)?,
                ],
                field: obj.field("m_nFieldOutput", Field::COLOR)?,
                ease: obj.bool("m_bEaseInOut", true)?,
            },
            "C_OP_RampScalarLinearSimple" => Self::RampScalarLinearSimple {
                rate: obj.float("m_Rate", 0.0)?,
                time: [
                    obj.float("m_flStartTime", 0.0)?,
                    obj.float("m_flEndTime", 1.0)?,
                ],
                field: obj.field("m_nField", Field::RADIUS)?,
            },
            "C_OP_RampScalarLinear" => Self::RampScalarLinear {
                rate: [obj.float("m_RateMin", 0.0)?, obj.float("m_RateMax", 0.0)?],
                start: [
                    obj.float("m_flStartTime_min", 0.0)?,
                    obj.float("m_flStartTime_max", 0.0)?,
                ],
                end: [
                    obj.float("m_flEndTime_min", 1.0)?,
                    obj.float("m_flEndTime_max", 1.0)?,
                ],
                proportional: obj.bool("m_bProportionalOp", true)?,
                field: obj.field("m_nField", Field::RADIUS)?,
            },
            _ => return Ok(None),
        }))
    }

    /// The operators of a particle's motion and turn.
    fn read_motion(class: &str, obj: &Obj<'_>) -> Result<Option<Self>> {
        Ok(Some(match class {
            "C_OP_BasicMovement" => Self::BasicMovement {
                gravity: VectorInput::read(obj, "m_Gravity", [0.0; 3])?,
                drag: FloatInput::read(obj, "m_fDrag", 0.0)?,
            },
            "C_OP_PositionLock" => Self::PositionLock(Box::new(position_lock(obj)?)),
            "C_OP_SpinUpdate" => Self::SpinUpdate,
            "C_OP_Spin" => Self::Spin {
                rate_degrees: obj.int_in("m_nSpinRateDegrees", 0, -(1 << 20), 1 << 20)? as i32,
                min_rate_degrees: obj.int_in("m_nSpinRateMinDegrees", 0, -(1 << 20), 1 << 20)?
                    as i32,
                stop_time: obj.float("m_fSpinRateStopTime", 0.0)?,
            },
            "C_OP_OrientTo2dDirection" if !obj.has("m_vecInput") => Self::OrientTo2dDirection {
                offset_degrees: obj.float("m_flRotOffset", 0.0)?,
                strength: obj.float("m_flSpinStrength", 1.0)?,
                field: obj.field("m_nFieldOutput", Field::ROLL)?,
            },
            "C_OP_MaxVelocity" => max_velocity(obj)?,
            _ => return Ok(None),
        }))
    }

    /// The operators of a particle's end and bounds: its decays, clamps and spline ramps.
    fn read_end(class: &str, obj: &Obj<'_>) -> Result<Option<Self>> {
        Ok(Some(match class {
            "C_OP_RampScalarSpline" => Self::RampScalarSpline {
                rate: [obj.float("m_RateMin", 0.0)?, obj.float("m_RateMax", 0.0)?],
                start: [
                    obj.float("m_flStartTime_min", 0.0)?,
                    obj.float("m_flStartTime_max", 0.0)?,
                ],
                end: [
                    obj.float("m_flEndTime_min", 1.0)?,
                    obj.float("m_flEndTime_max", 1.0)?,
                ],
                bias: obj.float("m_flBias", 0.5)?,
                proportional: obj.bool("m_bProportionalOp", true)?,
                ease_out: obj.bool("m_bEaseOut", false)?,
                field: obj.field("m_nField", Field::RADIUS)?,
            },
            "C_OP_AlphaDecay" => Self::AlphaDecay {
                min_alpha: obj.float("m_flMinAlpha", 0.0)?.max(0.0),
            },
            "C_OP_LerpEndCapScalar" => Self::LerpEndCapScalar {
                field: obj.field("m_nFieldOutput", Field::RADIUS)?,
                output: obj.float("m_flOutput", 1.0)?,
                lerp_time: obj.float("m_flLerpTime", 1.0)?,
            },
            "C_OP_ClampScalar" => Self::ClampScalar {
                field: obj.field("m_nFieldOutput", Field::RADIUS)?,
                min: FloatInput::read(obj, "m_flOutputMin", 0.0)?,
                max: FloatInput::read(obj, "m_flOutputMax", 1.0)?,
            },
            "C_OP_EndCapTimedDecay" => Self::EndCapTimedDecay {
                decay_time: FloatInput::read(obj, "m_flDecayTime", 0.0)?,
            },
            _ => return Ok(None),
        }))
    }

    /// The operators that follow a control point, the ground, the parent or the body.
    fn read_follow(class: &str, obj: &Obj<'_>) -> Result<Option<Self>> {
        Ok(Some(match class {
            "C_OP_DampenToCP" => Self::DampenToCp {
                control_point: obj.control_point("m_nControlPointNumber", 0)?,
                range: obj.float("m_flRange", 100.0)?,
                scale: obj.float("m_flScale", 1.0)?,
            },
            "C_OP_Orient2DRelToCP" => Self::Orient2dRelToCp {
                control_point: obj.control_point("m_nCP", 0)?,
                field: obj.field("m_nFieldOutput", Field::ROLL)?,
                offset_degrees: obj.float("m_flRotOffset", 0.0)?,
                spin_strength: obj.float("m_flSpinStrength", 1.0)?,
            },
            "C_OP_DistanceToCP" | "C_OP_DistanceToTransform" if !obj.bool("m_bLOS", false)? => {
                Self::DistanceToTransform(Box::new(distance_to_transform(obj)?))
            }
            "C_OP_MovementPlaceOnGround" => {
                Self::MovementPlaceOnGround(Box::new(ground_movement(obj)?))
            }
            "C_OP_InheritFromParentParticles" => Self::InheritFromParentParticles {
                scale: obj.float("m_flScale", 1.0)?,
                field: obj.field("m_nFieldOutput", Field::RADIUS)?,
                increment: obj.int_in("m_nIncrement", 1, -(1 << 16), 1 << 16)? as i32,
                random: obj.bool("m_bRandomDistribution", false)?,
            },
            "C_OP_MoveToHitbox" => move_to_hitbox(obj)?,
            _ => return Ok(None),
        }))
    }
}

fn per_child(obj: &Obj<'_>) -> Result<Operator> {
    Ok(Operator::SetPerChildControlPoint {
        group: obj.int_in("m_nChildGroupID", 0, -1, 1 << 16)? as i32,
        control_point: obj.control_point("m_nFirstControlPoint", 0)?,
        count: obj.int_clamped("m_nNumControlPoints", 1, 0, 64)? as i32,
        first: FloatInput::read(obj, "m_nFirstSourcePoint", 0.0)?,
        increment: FloatInput::read(obj, "m_nParticleIncrement", 1.0)?,
        per_particle: obj.bool("m_bNumBasedOnParticleCount", false)?,
        orientation: obj.bool("m_bSetOrientation", false)?,
    })
}

fn fade_out(obj: &Obj<'_>) -> Result<Operator> {
    let (mut min, mut max) = (
        obj.float("m_flFadeOutTimeMin", 0.25)?,
        obj.float("m_flFadeOutTimeMax", 0.25)?,
    );
    // A random time from zero is an instant fade.
    if min != max && min == 0.0 {
        (min, max) = (f32::EPSILON, f32::EPSILON);
    }
    Ok(Operator::FadeOut {
        min,
        max,
        exponent: obj.float("m_flFadeOutTimeExp", 1.0)?,
        proportional: obj.bool("m_bProportional", true)?,
        ease: obj.bool("m_bEaseInAndOut", true)?,
    })
}

fn fade_and_kill(obj: &Obj<'_>) -> Result<Operator> {
    let mut start_in = obj.float("m_flStartFadeInTime", 0.0)?;
    let mut end_in = obj.float("m_flEndFadeInTime", 0.5)?;
    let mut start_out = obj.float("m_flStartFadeOutTime", 0.5)?;
    let mut end_out = obj.float("m_flEndFadeOutTime", 1.0)?;
    end_in = end_in.max(start_in);
    end_out = end_out.max(start_out);
    if start_in > start_out {
        (start_in, start_out) = (start_out, start_in);
    }
    if end_in > end_out {
        (end_in, end_out) = (end_out, end_in);
    }
    Ok(Operator::FadeAndKill {
        fade_in: [start_in, end_in],
        fade_out: [start_out, end_out],
        start_alpha: obj.float("m_flStartAlpha", 1.0)?,
        end_alpha: obj.float("m_flEndAlpha", 0.0)?,
    })
}

fn position_lock(obj: &Obj<'_>) -> Result<PositionLock> {
    // The exponents count in quarter steps.
    let quarter = |v: f32| (v.clamp(-255.0, 255.0) * 4.0).trunc() * 0.25;
    let threshold = obj.float("m_flJumpThreshold", 512.0)?;
    Ok(PositionLock {
        transform: Transform::read(obj, "m_nControlPointNumber", true)?,
        start: [
            obj.float("m_flStartTime_min", 1.0)?,
            obj.float("m_flStartTime_max", 1.0)?,
            quarter(obj.float("m_flStartTime_exp", 1.0)?),
        ],
        end: [
            obj.float("m_flEndTime_min", 1.0)?,
            obj.float("m_flEndTime_max", 1.0)?,
            quarter(obj.float("m_flEndTime_exp", 1.0)?),
        ],
        range: obj.float("m_flRange", 0.0)?,
        range_bias: FloatInput::read(obj, "m_flRangeBias", 0.2)?,
        jump_threshold: threshold,
        previous_scale: obj.float("m_flPrevPosScale", 1.0)?,
        lock_rotation: obj.bool("m_bLockRot", false)?,
        scale: VectorInput::read(obj, "m_vecScale", [1.0; 3])?,
        output: obj.field("m_nFieldOutput", Field::POSITION)?,
        output_previous: obj.field("m_nFieldOutputPrev", Field::POSITION_PREVIOUS)?,
    })
}

fn oscillate(obj: &Obj<'_>) -> Result<Oscillate> {
    let field = obj.field("m_nField", Field::POSITION)?;
    Ok(Oscillate {
        field,
        rate_min: obj.vec3("m_RateMin", [0.0; 3])?,
        rate_max: obj.vec3("m_RateMax", [0.0; 3])?,
        frequency_min: obj.vec3("m_FrequencyMin", [1.0; 3])?,
        frequency_max: obj.vec3("m_FrequencyMax", [1.0; 3])?,
        multiplier: FloatInput::read(obj, "m_flOscMult", 2.0)?,
        offset: FloatInput::read(obj, "m_flOscAdd", 0.5)?,
        rate_scale: FloatInput::read(obj, "m_flRateScale", 1.0)?,
        proportional: obj.bool("m_bProportional", true)?,
        proportional_window: obj.bool("m_bProportionalOp", true)?,
        start: [
            obj.float("m_flStartTime_min", 0.0)?,
            obj.float("m_flStartTime_max", 0.0)?,
        ],
        end: [
            obj.float("m_flEndTime_min", 1.0)?,
            obj.float("m_flEndTime_max", 1.0)?,
        ],
        offset_position: obj.bool("m_bOffset", false)? && field == Field::POSITION,
    })
}

fn max_velocity(obj: &Obj<'_>) -> Result<Operator> {
    let mut max = FloatInput::read(obj, "m_flMaxVelocity", 0.0)?;
    // Before `vpcf66` a control point's component could stand in for the maximum.
    if obj.int("m_nOverrideCP", -1)? >= 0 {
        max = FloatInput::ControlPointComponent {
            control_point: obj.control_point("m_nOverrideCP", 0)?,
            component: obj.int_in("m_nOverrideCPField", 0, 0, 2)? as u8,
            mapping: super::input::Mapping::Direct,
        };
    }
    Ok(Operator::MaxVelocity {
        max,
        min: FloatInput::read(obj, "m_flMinVelocity", 0.0)?,
    })
}

fn distance_to_transform(obj: &Obj<'_>) -> Result<DistanceToTransform> {
    Ok(DistanceToTransform {
        field: obj.field("m_nFieldOutput", Field::RADIUS)?,
        input: [
            FloatInput::read(obj, "m_flInputMin", 0.0)?,
            FloatInput::read(obj, "m_flInputMax", 128.0)?,
        ],
        output: [
            FloatInput::read(obj, "m_flOutputMin", 0.0)?,
            FloatInput::read(obj, "m_flOutputMax", 1.0)?,
        ],
        // Before `vpcf49` the transform was the control point `m_nStartCP`.
        transform: Transform::read_member(obj, "m_TransformStart", "m_nStartCP", true)?,
        set_method: SetMethod::from_index(obj.choice("m_nSetMethod", &SetMethod::NAMES, 0)?),
        active_range: obj.bool("m_bActiveRange", false)?,
        additive: obj.bool("m_bAdditive", false)?,
        component_scale: VectorInput::read(obj, "m_vecComponentScale", [1.0; 3])?,
    })
}

fn ground_movement(obj: &Obj<'_>) -> Result<GroundMovement> {
    let point = |key: &str| -> Result<Option<ControlPoint>> {
        let index = obj.int_in(key, -1, -1, i64::from(MAX_CONTROL_POINT))?;
        Ok(u8::try_from(index).ok().and_then(ControlPoint::new))
    };
    Ok(GroundMovement {
        offset: FloatInput::read(obj, "m_flOffset", 0.0)?,
        max_trace_length: obj.float("m_flMaxTraceLength", 128.0)?,
        tolerance: obj.float("m_flTolerance", 32.0)?,
        direction: VectorInput::read(obj, "m_vecTraceDir", [0.0, 0.0, -1.0])?,
        trace_offset: obj.float("m_flTraceOffset", 64.0)?,
        lerp_rate: obj.float("m_flLerpRate", 0.0)?,
        references: [point("m_nRefCP1")?, point("m_nRefCP2")?],
        lerp_control_point: point("m_nLerpCP")?,
        miss: TraceMiss::read(obj)?,
        set_normal: obj.bool("m_bSetNormal", false)?,
        scale_offset: obj.bool("m_bScaleOffset", false)?,
        preserve_offset: point("m_nPreserveOffsetCP")?,
    })
}

fn move_to_hitbox(obj: &Obj<'_>) -> Result<Operator> {
    // Before `vpcf57` the model was the control point `m_nControlPointNumber`.
    let control_point = match obj.object("m_modelInput")? {
        None => obj.control_point("m_nControlPointNumber", 0)?,
        Some(model)
            if model
                .str("m_nType")?
                .is_none_or(|t| t == "PM_TYPE_CONTROL_POINT") =>
        {
            model.control_point("m_nControlPoint", 0)?
        }
        Some(_) => return Ok(Operator::Unsupported("C_OP_MoveToHitbox".to_owned())),
    };
    let types = ["HITBOX_LERP_LIFETIME", "HITBOX_LERP_CONSTANT"];
    Ok(Operator::MoveToHitbox {
        control_point,
        lifetime: [
            obj.float("m_flLifeTimeLerpStart", 0.0)?,
            obj.float("m_flLifeTimeLerpEnd", 1.0)?,
        ],
        previous_scale: obj.float("m_flPrevPosScale", 1.0)?,
        constant: obj.choice("m_nLerpType", &types, 0)? == 1,
        interpolation: FloatInput::read(obj, "m_flInterpolation", 1.0)?,
    })
}

fn from_particles(obj: &Obj<'_>) -> Result<ControlPointsFromParticles> {
    let first_control_point = obj.control_point("m_nFirstControlPoint", 0)?;
    let count = obj.int_clamped("m_nNumControlPoints", 1, 0, 64)?;
    Ok(ControlPointsFromParticles {
        group: obj.int_in("m_nChildGroupID", 0, -1, 1 << 16)? as i32,
        first_control_point,
        count: count.min(64 - first_control_point.index() as i64) as u8,
        first_source: FloatInput::read(obj, "m_nFirstSourcePoint", 0.0)?,
        reverse: obj.bool("m_bReverse", false)?,
        orientation: obj.bool("m_bSetOrientation", false)?,
    })
}

impl Force {
    pub(super) fn read(class: &str, obj: &Obj<'_>, behavior_version: i32) -> Result<Self> {
        Ok(match class {
            "C_OP_AttractToControlPoint" if !obj.bool("m_bApplyMinForce", false)? => {
                Self::AttractToControlPoint {
                    transform: Transform::read(
                        obj,
                        "m_nControlPointNumber",
                        obj.bool("m_bScaleLocal", false)?,
                    )?,
                    amount: FloatInput::read(obj, "m_fForceAmount", 100.0)?,
                    falloff: obj.float("m_fFalloffPower", 2.0)?,
                    min_distance: FloatInput::read(obj, "m_fMinimumDistance", 0.0)?,
                    component_scale: obj.vec3("m_vecComponentScale", [1.0; 3])?,
                }
            }
            "C_OP_RandomForce" => Self::RandomForce {
                min: obj.vec3("m_MinForce", [0.0; 3])?,
                max: obj.vec3("m_MaxForce", [0.0; 3])?,
            },
            "C_OP_TwistAroundAxis" => {
                let control_point = obj.control_point("m_nControlPointNumber", 0)?;
                Self::TwistAroundAxis {
                    amount: obj.float("m_fForceAmount", 0.0)?,
                    axis: obj.vec3("m_TwistAxis", [0.0, 0.0, 1.0])?,
                    local: obj.bool("m_bLocalSpace", false)?,
                    control_point,
                    axis_control_point: if behavior_version >= 3 {
                        control_point
                    } else {
                        ControlPoint::default()
                    },
                }
            }
            other => Self::Unsupported(other.to_owned()),
        })
    }
}
