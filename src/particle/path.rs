//! Paths between control points (`CPathParameters`): a quadratic Bezier from a start control
//! point to an end one, bent towards a midpoint, and the functions that place particles along one.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::input::FloatInput;
use super::read::{MAX_CONTROL_POINT, Obj};
use super::transform::ControlPoint;

/// A path: start, midpoint and end make a quadratic Bezier, which bends towards the midpoint
/// without passing through it (unless a mid control point names where it passes at half way).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Path {
    /// Where it starts.
    pub start: ControlPoint,
    /// A control point it passes through at half way, if any (then the midpoint and the bulge
    /// do not apply).
    pub mid: Option<ControlPoint>,
    /// Where it ends.
    pub end: ControlPoint,
    /// Which control point's forward the bulge pushes the midpoint along: 0 none (a random
    /// offset of up to the bulge per axis, fixed for the system), 1 the start's, 2 the end's.
    pub bulge_control: u8,
    /// How far the midpoint is pushed, as a share of the path's length along a control point's
    /// forward, or in units at random.
    pub bulge: f32,
    /// Where between start and end the midpoint lies, 0..1.
    pub mid_point: f32,
    /// Added to the start.
    pub start_offset: [f32; 3],
    /// Added to the midpoint last.
    pub mid_offset: [f32; 3],
    /// Added to the end.
    pub end_offset: [f32; 3],
}

impl Path {
    /// The path member `key` of `obj`, the schema's defaults when absent. Control points out of
    /// range are clamped to the ones a system has, as the game clamps them.
    pub(super) fn read(obj: &Obj<'_>, key: &str) -> Result<Self> {
        Self::read_members(obj.object(key)?.as_ref())
    }

    fn read_members(path: Option<&Obj<'_>>) -> Result<Self> {
        let int = |key: &str, default: i64| -> Result<i64> {
            path.map_or(Ok(default), |p| p.int(key, default))
        };
        let float = |key: &str, default: f32| -> Result<f32> {
            path.map_or(Ok(default), |p| p.float(key, default))
        };
        let vector = |key: &str| -> Result<[f32; 3]> {
            path.map_or(Ok([0.0; 3]), |p| p.vec3(key, [0.0; 3]))
        };
        let point = |key: &str| -> Result<ControlPoint> {
            let index = int(key, 0)?.clamp(0, i64::from(MAX_CONTROL_POINT));
            Ok(ControlPoint::new(index as u8).unwrap_or_default())
        };
        let mid = int("m_nMidControlPointNumber", -1)?;
        Ok(Self {
            start: point("m_nStartControlPointNumber")?,
            mid: (mid >= 0)
                .then(|| ControlPoint::new(mid.min(i64::from(MAX_CONTROL_POINT)) as u8))
                .flatten(),
            end: point("m_nEndControlPointNumber")?,
            bulge_control: int("m_nBulgeControl", 0)?.clamp(0, 2) as u8,
            bulge: float("m_flBulge", 0.0)?,
            mid_point: float("m_flMidPoint", 0.5)?,
            start_offset: vector("m_vStartPointOffset")?,
            mid_offset: vector("m_vMidPointOffset")?,
            end_offset: vector("m_vEndOffset")?,
        })
    }
}

/// `C_INIT_CreateSequentialPath`'s parameters: particles placed one after another at evenly
/// spaced points of a path.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SequentialPath {
    /// A random offset of up to this per axis.
    pub max_distance: f32,
    /// How many points the path is cut into.
    pub count: f32,
    /// Past the end the walk starts over rather than turning back.
    pub looped: bool,
    /// The path runs through every control point from the start to the end, a segment between
    /// each pair, rather than from the start straight to the end.
    pub pairs: bool,
    /// Where on the path a particle was made is kept (its share of the way and its segment's
    /// control points, in the hitbox offset attribute), for `C_OP_LockToSavedSequentialPath`.
    pub save_offset: bool,
    /// The path.
    pub path: Path,
}

/// `C_OP_MaintainSequentialPath`'s parameters: particles pulled each step to evenly spaced
/// points of a path, in order.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MaintainPath {
    /// How far a particle may stay from its point.
    pub max_distance: FloatInput,
    /// How many points the path is cut into.
    pub count: FloatInput,
    /// How strongly a particle is pulled onto its point: 1 onto it, 0 its offset kept.
    pub cohesion: FloatInput,
    /// The control points must move at least this (squared) for the pull to run again (0
    /// always runs).
    pub tolerance: f32,
    /// Past the last point the order starts over rather than turning back.
    pub looped: bool,
    /// As many points as particles.
    pub use_particle_count: bool,
    /// The path.
    pub path: Path,
}

impl SequentialPath {
    pub(super) fn read(obj: &Obj<'_>) -> Result<Self> {
        Ok(Self {
            max_distance: obj.float("m_fMaxDistance", 0.0)?,
            count: obj.float("m_flNumToAssign", 100.0)?,
            looped: obj.bool("m_bLoop", true)?,
            pairs: obj.bool("m_bCPPairs", false)?,
            save_offset: obj.bool("m_bSaveOffset", false)?,
            path: Path::read(obj, "m_PathParams")?,
        })
    }
}

impl MaintainPath {
    pub(super) fn read(obj: &Obj<'_>) -> Result<Self> {
        Ok(Self {
            max_distance: FloatInput::read(obj, "m_fMaxDistance", 0.0)?,
            count: FloatInput::read(obj, "m_flNumToAssign", 100.0)?,
            cohesion: FloatInput::read(obj, "m_flCohesionStrength", 1.0)?,
            tolerance: obj.float("m_flTolerance", 0.0)?,
            looped: obj.bool("m_bLoop", true)?,
            use_particle_count: obj.bool("m_bUseParticleCount", false)?,
            path: Path::read(obj, "m_PathParams")?,
        })
    }
}
