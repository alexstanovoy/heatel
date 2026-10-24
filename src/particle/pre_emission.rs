//! Pre-emission operators: what a system does to its control points and itself each step,
//! before it emits.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::input::FloatInput;
use super::read::Obj;
use super::transform::ControlPoint;

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
    /// A class this crate does not read.
    Unsupported(String),
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
            other => Self::Unsupported(other.to_owned()),
        })
    }
}
