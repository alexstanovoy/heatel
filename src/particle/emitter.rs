//! Emitters: when a system creates particles.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::input::FloatInput;
use super::read::Obj;

/// An emitter class.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Emitter {
    /// `C_OP_InstantaneousEmitter`: one burst at `start_time`.
    Instantaneous {
        /// How many.
        count: FloatInput,
        /// When, after the system starts.
        start_time: FloatInput,
        /// Most per step (-1 no limit).
        max_per_frame: i32,
    },
    /// `C_OP_ContinuousEmitter`: `rate` a second from `start_time` for `duration` (0 forever).
    Continuous {
        /// Particles a second.
        rate: FloatInput,
        /// When, after the system starts.
        start_time: FloatInput,
        /// How long (0 forever).
        duration: FloatInput,
    },
    /// A class this crate does not read.
    Unsupported(String),
}

impl Emitter {
    pub(super) fn read(class: &str, obj: &Obj<'_>) -> Result<Self> {
        Ok(match class {
            "C_OP_InstantaneousEmitter" if !obj.has("m_flInitFromKilledParentParticles") => {
                Self::Instantaneous {
                    count: FloatInput::read(obj, "m_nParticlesToEmit", 100.0)?,
                    start_time: FloatInput::read(obj, "m_flStartTime", 0.0)?,
                    max_per_frame: obj.int_in(
                        "m_nMaxEmittedPerFrame",
                        -1,
                        -1,
                        i64::from(i32::MAX),
                    )? as i32,
                }
            }
            "C_OP_ContinuousEmitter" if !obj.bool("m_bInitFromKilledParentParticles", false)? => {
                Self::Continuous {
                    rate: FloatInput::read(obj, "m_flEmitRate", 100.0)?,
                    start_time: FloatInput::read(obj, "m_flStartTime", 0.0)?,
                    duration: FloatInput::read(obj, "m_flEmissionDuration", 0.0)?,
                }
            }
            other => Self::Unsupported(other.to_owned()),
        })
    }
}
