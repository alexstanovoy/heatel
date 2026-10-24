//! Constraints: what holds particles in place after `C_OP_BasicMovement` has moved them.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::field::Field;
use super::input::FloatInput;
use super::path::Path;
use super::read::Obj;
use super::transform::ControlPoint;

/// A constraint class.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Constraint {
    /// `C_OP_ConstrainDistanceToPath`: particles kept within a distance of a point that travels
    /// along a path with their age.
    DistanceToPath(Box<DistanceToPath>),
    /// `C_OP_PlanarConstraint`: particles kept in front of a plane.
    Planar(Box<Planar>),
    /// `C_OP_WorldTraceConstraint`: particles stopped, bounced or killed where their step meets
    /// the world.
    WorldTrace(Box<WorldTrace>),
    /// A class this crate does not read.
    Unsupported(String),
}

/// `C_OP_ConstrainDistanceToPath`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DistanceToPath {
    /// The least distance from the travelling point.
    pub min_distance: f32,
    /// The largest at the path's start, middle and end, each raised to at least the least; a
    /// middle or an end still below 0 is the one before it.
    pub max_distance: [f32; 3],
    /// The path.
    pub path: Path,
    /// The seconds the point takes to the path's end.
    pub travel_time: f32,
    /// An attribute that scales the travel time per particle (`DISABLED` none).
    pub scale_field: Field,
    /// An attribute that holds the point's share of the way instead (`DISABLED` none).
    pub manual_field: Field,
}

/// `C_OP_PlanarConstraint`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Planar {
    /// A point of the plane.
    pub point: [f32; 3],
    /// Its normal, the side particles are kept on.
    pub normal: [f32; 3],
    /// The control point whose frame they are in.
    pub control_point: ControlPoint,
    /// The point is in the world's frame.
    pub global_origin: bool,
    /// The normal is in the world's frame.
    pub global_normal: bool,
    /// The radius' scale: a particle rests on the plane by its radius.
    pub radius_scale: FloatInput,
    /// Only particles nearer than this to the plane's point are held (0 or less: all).
    pub max_distance: FloatInput,
    /// The old code: particles are held in blocks of four, a block wholly once one of it is
    /// near enough.
    pub old_code: bool,
}

/// `C_OP_WorldTraceConstraint`'s parameters.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WorldTrace {
    /// It does not collide (`COLLISION_MODE_DISABLED`).
    pub disabled: bool,
    /// The radius' scale: a step is traced half the radius further.
    pub radius_scale: FloatInput,
    /// The share of the speed kept when bouncing off.
    pub bounce: FloatInput,
    /// The share of the speed along the surface kept.
    pub slide: FloatInput,
    /// A random turn of the bounce.
    pub random_direction_scale: FloatInput,
    /// The bounce keeps the step's length.
    pub decay_bounce: bool,
    /// A particle dies where it meets the world.
    pub kill_on_contact: bool,
    /// Slower than this (units a second) a particle that meets the world dies (below 0 never).
    pub min_speed: f32,
    /// A particle killed on contact bounces still.
    pub bounce_killed: bool,
    /// The surface's normal is written to the particle's.
    pub set_normal: bool,
    /// An attribute a particle that stops is stuck at (`DISABLED` none).
    pub stick_field: Field,
    /// Slower than this a particle stops and sticks (below 0 always).
    pub stop_speed: FloatInput,
}

impl Constraint {
    pub(super) fn read(class: &str, obj: &Obj<'_>) -> Result<Self> {
        Ok(match class {
            "C_OP_ConstrainDistanceToPath" => {
                let min = obj.float("m_fMinDistance", 0.0)?;
                Self::DistanceToPath(Box::new(DistanceToPath {
                    min_distance: min,
                    max_distance: [
                        obj.float("m_flMaxDistance0", 100.0)?.max(min),
                        obj.float("m_flMaxDistanceMid", -1.0)?.max(min),
                        obj.float("m_flMaxDistance1", -1.0)?.max(min),
                    ],
                    path: Path::read(obj, "m_PathParameters")?,
                    travel_time: obj.float("m_flTravelTime", 10.0)?,
                    scale_field: obj.field("m_nFieldScale", Field::DISABLED)?,
                    manual_field: obj.field("m_nManualTField", Field::DISABLED)?,
                }))
            }
            "C_OP_PlanarConstraint" => Self::Planar(Box::new(Planar {
                point: obj.vec3("m_PointOnPlane", [0.0; 3])?,
                normal: obj.vec3("m_PlaneNormal", [0.0, 0.0, 1.0])?,
                control_point: obj.control_point("m_nControlPointNumber", 0)?,
                global_origin: obj.bool("m_bGlobalOrigin", false)?,
                global_normal: obj.bool("m_bGlobalNormal", false)?,
                radius_scale: FloatInput::read(obj, "m_flRadiusScale", 1.0)?,
                max_distance: FloatInput::read(obj, "m_flMaximumDistanceToCP", -1.0)?,
                old_code: obj.bool("m_bUseOldCode", true)?,
            })),
            "C_OP_WorldTraceConstraint" => Self::WorldTrace(Box::new(world_trace(obj)?)),
            other => Self::Unsupported(other.to_owned()),
        })
    }
}

fn world_trace(obj: &Obj<'_>) -> Result<WorldTrace> {
    let modes = [
        "COLLISION_MODE_DISABLED",
        "COLLISION_MODE_INITIAL_TRACE_DOWN",
        "COLLISION_MODE_PER_FRAME_PLANESET",
        "COLLISION_MODE_USE_NEAREST_TRACE",
        "COLLISION_MODE_PER_PARTICLE_TRACE",
    ];
    // The numeric form counts from -1 (disabled); every other mode traces the world, by
    // shortcuts of their own.
    let disabled = match obj.get("m_nCollisionMode").and_then(|v| v.as_i64()) {
        Some(n) => n < 0,
        None => obj.choice("m_nCollisionMode", &modes, 4)? == 0,
    };
    Ok(WorldTrace {
        disabled,
        radius_scale: FloatInput::read(obj, "m_flRadiusScale", 1.0)?,
        bounce: FloatInput::read(obj, "m_flBounceAmount", 0.0)?,
        slide: FloatInput::read(obj, "m_flSlideAmount", 0.0)?,
        random_direction_scale: FloatInput::read(obj, "m_flRandomDirScale", 0.0)?,
        decay_bounce: obj.bool("m_bDecayBounce", false)?,
        kill_on_contact: obj.bool("m_bKillonContact", false)?,
        min_speed: obj.float("m_flMinSpeed", -1.0)?,
        bounce_killed: obj.bool("m_bKillonContactBounce", false)?,
        set_normal: obj.bool("m_bSetNormal", false)?,
        stick_field: obj.field("m_nStickOnCollisionField", Field::DISABLED)?,
        stop_speed: FloatInput::read(obj, "m_flStopSpeed", -1.0)?,
    })
}
