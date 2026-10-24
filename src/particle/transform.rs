//! Where a function places or reads things: a control point's transform.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result as HeatelResult;
use crate::kv3::Value;

use super::read::{MAX_CONTROL_POINT, Obj};

/// A control point's number, 0..=63.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(try_from = "u8", into = "u8")
)]
pub struct ControlPoint(u8);

impl ControlPoint {
    /// Control point `index`, if a system has it.
    pub const fn new(index: u8) -> Option<Self> {
        if index <= MAX_CONTROL_POINT {
            Some(Self(index))
        } else {
            None
        }
    }

    /// Its number.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

impl TryFrom<u8> for ControlPoint {
    type Error = String;

    fn try_from(index: u8) -> Result<Self, Self::Error> {
        Self::new(index).ok_or_else(|| format!("control point {index} is past {MAX_CONTROL_POINT}"))
    }
}

impl From<ControlPoint> for u8 {
    fn from(point: ControlPoint) -> Self {
        point.0
    }
}

/// A transform input (`CParticleTransformInput`): a control point's position, with or
/// without its orientation, or the identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Transform {
    /// The world's origin, unrotated.
    Identity,
    /// A control point.
    ControlPoint {
        /// The control point.
        control_point: ControlPoint,
        /// Whether its orientation rotates what is placed.
        orientation: bool,
        /// The last control point of a range a particle picks one of, if any.
        range_max: Option<ControlPoint>,
    },
}

impl Transform {
    /// Control point `cp` with its orientation.
    pub const fn point(control_point: ControlPoint) -> Self {
        Self::ControlPoint {
            control_point,
            orientation: true,
            range_max: None,
        }
    }

    /// The input `m_TransformInput` of `obj`, or what an older format put in its place: the
    /// control point `legacy_key` (`m_nControlPointNumber`), oriented when `legacy_orientation`.
    pub(super) fn read(
        obj: &Obj<'_>,
        legacy_key: &str,
        legacy_orientation: bool,
    ) -> HeatelResult<Self> {
        Self::read_member(obj, "m_TransformInput", legacy_key, legacy_orientation)
    }

    /// The input `key` of `obj`, or in its place the control point `legacy_key`, as
    /// [`read`](Self::read) reads `m_TransformInput`.
    pub(super) fn read_member(
        obj: &Obj<'_>,
        key: &str,
        legacy_key: &str,
        legacy_orientation: bool,
    ) -> HeatelResult<Self> {
        let Some(value) = obj.get(key) else {
            let control_point = obj.control_point(legacy_key, 0)?;
            return Ok(Self::ControlPoint {
                control_point,
                orientation: legacy_orientation,
                range_max: None,
            });
        };
        if !matches!(value, Value::Object(_)) {
            return Ok(Self::point(obj.control_point(key, 0)?));
        }
        let input = Obj::new(value)?;
        let kinds = [
            "PT_TYPE_INVALID",
            "PT_TYPE_CONTROL_POINT",
            "PT_TYPE_CONTROL_POINT_RANGE",
        ];
        let kind = match input.get("m_nType") {
            Some(Value::String(name)) if !kinds.contains(&name.as_str()) => {
                // A named value or another source this crate does not read stands still.
                return Ok(Self::Identity);
            }
            _ => input.choice("m_nType", &kinds, 1)?,
        };
        if kind == 0 {
            return Ok(Self::Identity);
        }
        let control_point = input.control_point("m_nControlPoint", 0)?;
        let range_max = (kind == 2)
            .then(|| {
                input.int_in(
                    "m_nControlPointRangeMax",
                    0,
                    0,
                    i64::from(MAX_CONTROL_POINT),
                )
            })
            .transpose()?
            .and_then(|max| ControlPoint::new(max as u8));
        Ok(Self::ControlPoint {
            control_point,
            orientation: input.bool("m_bUseOrientation", true)?,
            range_max,
        })
    }

    /// The control point it reads, if any.
    pub fn control_point(&self) -> Option<ControlPoint> {
        match self {
            Self::Identity => None,
            Self::ControlPoint { control_point, .. } => Some(*control_point),
        }
    }
}
