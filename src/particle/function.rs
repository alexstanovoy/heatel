//! What every function of a system has besides its class: when it runs and how strongly.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::input::FloatInput;
use super::read::Obj;

/// One function of a system: its class with its parameters, and when it runs.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Function<T> {
    /// The class and its parameters.
    pub class: T,
    /// When it runs and how strongly.
    pub timing: Timing,
}

/// When a function runs (`m_nOpEndCapState`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EndCap {
    /// Always.
    #[default]
    Always,
    /// Only before the end cap.
    Off,
    /// Only in the end cap.
    On,
}

/// When and how strongly a function runs: its end cap state, and a strength that fades in and
/// out over the system's age (`m_flOpStartFadeInTime` and the like).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Timing {
    /// In which phase it runs.
    pub end_cap: EndCap,
    /// The strength before the fades.
    pub strength: FloatInput,
    /// The age its strength starts and ends fading in.
    pub fade_in: [f32; 2],
    /// The age its strength starts and ends fading out (0 never).
    pub fade_out: [f32; 2],
    /// The period the fades repeat with (0 never).
    pub oscillate: f32,
    /// Only in the system's first frame (a pre-emission operator's `m_bRunOnce`).
    pub run_once: bool,
}

impl Timing {
    pub(super) fn read(obj: &Obj<'_>) -> Result<Self> {
        let states = [
            "PARTICLE_ENDCAP_ALWAYS_ON",
            "PARTICLE_ENDCAP_ENDCAP_OFF",
            "PARTICLE_ENDCAP_ENDCAP_ON",
        ];
        let end_cap = match obj.get("m_nOpEndCapState").and_then(|v| v.as_i64()) {
            // The numeric form counts from -1 (always).
            Some(n) => [EndCap::Always, EndCap::Off, EndCap::On][(n + 1).clamp(0, 2) as usize],
            None => [EndCap::Always, EndCap::Off, EndCap::On]
                [obj.choice("m_nOpEndCapState", &states, 0)?],
        };
        Ok(Self {
            end_cap,
            strength: FloatInput::read(obj, "m_flOpStrength", 1.0)?,
            fade_in: [
                obj.float("m_flOpStartFadeInTime", 0.0)?,
                obj.float("m_flOpEndFadeInTime", 0.0)?,
            ],
            fade_out: [
                obj.float("m_flOpStartFadeOutTime", 0.0)?,
                obj.float("m_flOpEndFadeOutTime", 0.0)?,
            ],
            oscillate: obj.float("m_flOpFadeOscillatePeriod", 0.0)?,
            run_once: obj.bool("m_bRunOnce", false)?,
        })
    }

    /// Whether it runs at full strength in every phase at every age.
    pub fn is_always(&self) -> bool {
        !self.run_once
            && self.end_cap == EndCap::Always
            && self.strength == FloatInput::Literal(1.0)
            && self.fade_in == [0.0; 2]
            && self.fade_out == [0.0; 2]
    }

    /// The strength its fades give at the function's time `time` (0..=1).
    pub fn fade(&self, time: f32) -> f32 {
        let [in_start, in_end] = self.fade_in;
        let [out_start, out_end] = self.fade_out;
        if in_start > time || (out_end > 0.0 && out_end < time) {
            return 0.0;
        }
        let in_end = in_end.max(in_start);
        let out_start = out_start.max(in_end);
        let out_end = out_end.max(out_start);
        let mut strength: f32 = 1.0;
        if in_end > time && in_end > in_start {
            strength = strength.min((time - in_start) / (in_end - in_start));
        }
        if time > out_start && out_end > out_start {
            strength = strength.min((time - out_end) / (out_start - out_end));
        }
        strength
    }

    /// The function's time at the system's age `age`: the age, or its phase in the oscillation.
    pub fn time(&self, age: f32) -> f32 {
        if self.oscillate > 0.0 {
            (age / self.oscillate).rem_euclid(1.0)
        } else {
            age.max(0.0)
        }
    }
}
