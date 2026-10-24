//! Pre-emission operators: what a system does to its control points and itself each step,
//! before it emits.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::input::{FloatInput, Mapping, VectorInput};
use super::read::Obj;
use super::transform::{ControlPoint, Transform};

/// A pre-emission operator class.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum PreEmission {
    /// `C_OP_SetControlPointPositions`: four control points placed in a head control point's
    /// frame (or the world's).
    SetControlPointPositions(Box<ControlPointPositions>),
    /// `C_OP_StopAfterCPDuration`: the system stops emitting once it is older than a duration.
    StopAfterDuration {
        /// The duration.
        duration: FloatInput,
        /// Its particles go at once.
        destroy: bool,
        /// Its end cap plays.
        play_end_cap: bool,
    },
    /// `C_OP_HSVShiftToCP`: the hue, saturation and value shift of a colour control point from
    /// the system's own colour, into a control point.
    HsvShiftToCp {
        /// The control point of the colour (0..255 per channel).
        color: ControlPoint,
        /// The control point whose X enables the shift.
        enable: ControlPoint,
        /// The control point the shift goes to.
        output: ControlPoint,
        /// The system's own colour, 0..1.
        default_color: [f32; 3],
    },
    /// `C_OP_SetSingleControlPointPosition`: a control point placed in a transform's frame.
    SetSingleControlPointPosition {
        /// The control point placed.
        control_point: ControlPoint,
        /// Where, in the transform's frame.
        position: VectorInput,
        /// The frame.
        transform: Transform,
        /// Only in the first step.
        set_once: bool,
    },
    /// `C_OP_SetControlPointFieldToScalarExpression`: a component of a control point from two
    /// numbers and an expression.
    SetControlPointFieldToScalarExpression(Box<ScalarExpressionToCp>),
    /// `C_OP_DistanceBetweenCPsToCP`: a component of a control point from the distance between
    /// two others.
    DistanceBetweenCpsToCp(Box<DistanceBetweenCps>),
    /// `C_OP_SetParentControlPointsToChildCP`: a control point of each child of a group from one
    /// of the system's control points each, in order.
    SetParentControlPointsToChildCp {
        /// The children's group.
        group: i32,
        /// The control point set on each child.
        child_control_point: ControlPoint,
        /// How many children at most.
        count: u8,
        /// The system's control point the first child takes (the next takes the one after).
        first_source: ControlPoint,
        /// The orientation is taken too.
        orientation: bool,
    },
    /// `C_OP_SetCPOrientationToPointAtCP`: a control point turned to face another.
    SetCpOrientationToPointAtCp {
        /// The control point faced.
        input: ControlPoint,
        /// The control point turned.
        output: ControlPoint,
        /// How far it turns each step, 0..1.
        interpolation: FloatInput,
        /// Only on the ground plane.
        flat: bool,
        /// Its side axes from a perpendicular that holds when it faces straight up or down.
        avoid_singularity: bool,
        /// Away from the control point rather than at it.
        point_away: bool,
    },
    /// A class this crate does not read.
    Unsupported(String),
}

/// How two numbers make one (`ScalarExpressionType_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum ScalarExpression {
    /// None: 0.
    Uninitialized,
    /// The first plus the second.
    #[default]
    Add,
    /// The first less the second.
    Subtract,
    /// Their product.
    Multiply,
    /// The first over the second.
    Divide,
    /// The first.
    Input1,
    /// The smaller.
    Min,
    /// The larger.
    Max,
    /// The first modulo the second.
    Mod,
    /// 1 when they are equal, else 0.
    Equal,
    /// 1 when the first is the larger, else 0.
    Greater,
    /// 1 when the first is the smaller, else 0.
    Less,
}

impl ScalarExpression {
    const NAMES: [&str; 12] = [
        "SCALAR_EXPRESSION_UNINITIALIZED",
        "SCALAR_EXPRESSION_ADD",
        "SCALAR_EXPRESSION_SUBTRACT",
        "SCALAR_EXPRESSION_MUL",
        "SCALAR_EXPRESSION_DIVIDE",
        "SCALAR_EXPRESSION_INPUT_1",
        "SCALAR_EXPRESSION_MIN",
        "SCALAR_EXPRESSION_MAX",
        "SCALAR_EXPRESSION_MOD",
        "SCALAR_EXPRESSION_EQUAL",
        "SCALAR_EXPRESSION_GT",
        "SCALAR_EXPRESSION_LT",
    ];

    fn read(obj: &Obj<'_>) -> Result<Self> {
        let all = [
            Self::Uninitialized,
            Self::Add,
            Self::Subtract,
            Self::Multiply,
            Self::Divide,
            Self::Input1,
            Self::Min,
            Self::Max,
            Self::Mod,
            Self::Equal,
            Self::Greater,
            Self::Less,
        ];
        // The numeric form counts from -1 (uninitialized).
        Ok(match obj.get("m_nExpression").and_then(|v| v.as_i64()) {
            Some(n) => all[(n + 1).clamp(0, 11) as usize],
            None => all[obj.choice("m_nExpression", &Self::NAMES, 1)?],
        })
    }

    /// The expression of `a` and `b`; a quotient or remainder by 0 is 0.
    pub fn evaluate(self, a: f32, b: f32) -> f32 {
        let flag = |c: bool| f32::from(u8::from(c));
        match self {
            Self::Uninitialized => 0.0,
            Self::Add => a + b,
            Self::Subtract => a - b,
            Self::Multiply => a * b,
            Self::Divide if b != 0.0 => a / b,
            Self::Mod if b != 0.0 => a % b,
            Self::Divide | Self::Mod => 0.0,
            Self::Input1 => a,
            Self::Min => a.min(b),
            Self::Max => a.max(b),
            Self::Equal => flag(a == b),
            Self::Greater => flag(a > b),
            Self::Less => flag(a < b),
        }
    }
}

/// `C_OP_SetControlPointFieldToScalarExpression`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ScalarExpressionToCp {
    /// How the inputs make the value.
    pub expression: ScalarExpression,
    /// The first input.
    pub input1: FloatInput,
    /// The second input.
    pub input2: FloatInput,
    /// The value's mapping, if any.
    pub output_remap: Option<Mapping>,
    /// The control point written.
    pub control_point: ControlPoint,
    /// Its component (none: nothing is written).
    pub component: Option<u8>,
    /// How far the component moves to the value, 0..1.
    pub interpolation: FloatInput,
}

/// `C_OP_DistanceBetweenCPsToCP`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DistanceBetweenCps {
    /// One end.
    pub start: ControlPoint,
    /// The other.
    pub end: ControlPoint,
    /// The control point written.
    pub output: ControlPoint,
    /// Its component.
    pub component: u8,
    /// The distances mapped.
    pub input: [f32; 2],
    /// To these values, clamped.
    pub output_range: [f32; 2],
    /// Only once.
    pub set_once: bool,
}

/// `C_OP_SetControlPointPositions`' parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ControlPointPositions {
    /// The control points and their places, in order (a later one wins).
    pub points: [(ControlPoint, [f32; 3]); 4],
    /// Only in the first step.
    pub set_once: bool,
    /// The places are in the world's frame.
    pub world: bool,
    /// The control point whose frame they are in.
    pub head: ControlPoint,
    /// They take the head's orientation.
    pub orient: bool,
}

impl PreEmission {
    pub(super) fn read(class: &str, obj: &Obj<'_>) -> Result<Self> {
        Ok(match class {
            "C_OP_SetControlPointPositions" => {
                let point = |n: u8, place: [f32; 3]| -> Result<(ControlPoint, [f32; 3])> {
                    Ok((
                        obj.control_point(&format!("m_nCP{n}"), n)?,
                        obj.vec3(&format!("m_vecCP{n}Pos"), place)?,
                    ))
                };
                Self::SetControlPointPositions(Box::new(ControlPointPositions {
                    points: [
                        point(1, [128.0, 0.0, 0.0])?,
                        point(2, [0.0, 128.0, 0.0])?,
                        point(3, [-128.0, 0.0, 0.0])?,
                        point(4, [0.0, -128.0, 0.0])?,
                    ],
                    set_once: obj.bool("m_bSetOnce", false)?,
                    world: obj.bool("m_bUseWorldLocation", false)?,
                    head: obj.control_point("m_nHeadLocation", 0)?,
                    orient: obj.bool("m_bOrient", false)?,
                }))
            }
            "C_OP_StopAfterCPDuration" => Self::StopAfterDuration {
                duration: FloatInput::read(obj, "m_flDuration", 1.0)?,
                destroy: obj.bool("m_bDestroyImmediately", false)?,
                play_end_cap: obj.bool("m_bPlayEndCap", true)?,
            },
            "C_OP_HSVShiftToCP" => Self::HsvShiftToCp {
                color: obj.control_point("m_nColorCP", 60)?,
                enable: obj.control_point("m_nColorGemEnableCP", 61)?,
                output: obj.control_point("m_nOutputCP", 62)?,
                default_color: obj.color("m_DefaultHSVColor", [1.0; 3])?,
            },
            other => return Self::read_more(other, obj),
        })
    }

    /// The pre-emission operators of the town portal, the Glyph and the missiles' trails.
    fn read_more(class: &str, obj: &Obj<'_>) -> Result<Self> {
        Ok(match class {
            "C_OP_SetSingleControlPointPosition" => Self::SetSingleControlPointPosition {
                control_point: obj.control_point("m_nCP1", 1)?,
                position: VectorInput::read(obj, "m_vecCP1Pos", [128.0, 0.0, 0.0])?,
                // Before `vpcf57` the frame was the control point `m_nHeadLocation`, or the
                // world's.
                transform: if !obj.has("m_transformInput")
                    && obj.bool("m_bUseWorldLocation", false)?
                {
                    Transform::Identity
                } else {
                    Transform::read_member(obj, "m_transformInput", "m_nHeadLocation", true)?
                },
                set_once: obj.bool("m_bSetOnce", false)?,
            },
            "C_OP_SetControlPointFieldToScalarExpression" => {
                let component = obj.int("m_nOutVectorField", 0)?;
                Self::SetControlPointFieldToScalarExpression(Box::new(ScalarExpressionToCp {
                    expression: ScalarExpression::read(obj)?,
                    input1: FloatInput::read(obj, "m_flInput1", 0.0)?,
                    input2: FloatInput::read(obj, "m_flInput2", 0.0)?,
                    output_remap: obj
                        .object("m_flOutputRemap")?
                        .map(|remap| Mapping::read(&remap))
                        .transpose()?,
                    control_point: obj.control_point("m_nOutputCP", 2)?,
                    component: u8::try_from(component).ok().filter(|c| *c <= 2),
                    interpolation: FloatInput::read(obj, "m_flInterpolation", 1.0)?,
                }))
            }
            // A line of sight trace, or a value set on a parent or the root, is not read.
            "C_OP_DistanceBetweenCPsToCP"
                if !obj.bool("m_bLOS", false)?
                    && obj
                        .str("m_nSetParent")?
                        .is_none_or(|p| p == "PARTICLE_SET_PARENT_NO") =>
            {
                Self::DistanceBetweenCpsToCp(Box::new(distance_between(obj)?))
            }
            "C_OP_SetParentControlPointsToChildCP" => {
                let first = obj.control_point("m_nFirstSourcePoint", 0)?;
                Self::SetParentControlPointsToChildCp {
                    group: obj.int_in("m_nChildGroupID", 0, -1, 1 << 16)? as i32,
                    child_control_point: obj.control_point("m_nChildControlPoint", 0)?,
                    // No more than the control points from the first source on.
                    count: obj
                        .int_clamped("m_nNumControlPoints", 1, 0, 64)?
                        .min(64 - first.index() as i64) as u8,
                    first_source: first,
                    orientation: obj.bool("m_bSetOrientation", false)?,
                }
            }
            "C_OP_SetCPOrientationToPointAtCP" => Self::SetCpOrientationToPointAtCp {
                input: obj.control_point("m_nInputCP", 1)?,
                output: obj.control_point("m_nOutputCP", 0)?,
                interpolation: FloatInput::read(obj, "m_flInterpolation", 1.0)?,
                flat: obj.bool("m_b2DOrientation", false)?,
                avoid_singularity: obj.bool("m_bAvoidSingularity", false)?,
                point_away: obj.bool("m_bPointAway", false)?,
            },
            other => Self::Unsupported(other.to_owned()),
        })
    }
}

fn distance_between(obj: &Obj<'_>) -> Result<DistanceBetweenCps> {
    Ok(DistanceBetweenCps {
        start: obj.control_point("m_nStartCP", 0)?,
        end: obj.control_point("m_nEndCP", 1)?,
        output: obj.control_point("m_nOutputCP", 2)?,
        component: obj.int_clamped("m_nOutputCPField", 0, 0, 2)? as u8,
        input: [
            obj.float("m_flInputMin", 0.0)?,
            obj.float("m_flInputMax", 128.0)?,
        ],
        output_range: [
            obj.float("m_flOutputMin", 0.0)?,
            obj.float("m_flOutputMax", 1.0)?,
        ],
        set_once: obj.bool("m_bSetOnce", false)?,
    })
}
