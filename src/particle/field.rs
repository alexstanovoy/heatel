//! Particle attributes and the ways a function writes them.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A particle attribute, by its number in Source 2's attribute table (`m_nOutputField: 3` is the
/// radius). Only the numbers the table has exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(try_from = "u8", into = "u8")
)]
pub struct Field(u8);

impl Field {
    /// Position (vector).
    pub const POSITION: Self = Self(0);
    /// Lifetime in seconds.
    pub const LIFE_DURATION: Self = Self(1);
    /// Position one step back (vector): the velocity is the difference.
    pub const POSITION_PREVIOUS: Self = Self(2);
    /// Radius.
    pub const RADIUS: Self = Self(3);
    /// Roll in radians.
    pub const ROLL: Self = Self(4);
    /// Roll speed in radians per second.
    pub const ROLL_SPEED: Self = Self(5);
    /// Colour, 0..1 per channel (vector).
    pub const COLOR: Self = Self(6);
    /// Alpha, 0..1.
    pub const ALPHA: Self = Self(7);
    /// Creation time.
    pub const CREATION_TIME: Self = Self(8);
    /// Sprite sheet sequence.
    pub const SEQUENCE_NUMBER: Self = Self(9);
    /// Trail length.
    pub const TRAIL_LENGTH: Self = Self(10);
    /// Particle id.
    pub const PARTICLE_ID: Self = Self(11);
    /// Yaw in radians.
    pub const YAW: Self = Self(12);
    /// Second sprite sheet sequence.
    pub const SECOND_SEQUENCE_NUMBER: Self = Self(13);
    /// The hitbox of the model a particle sits on.
    pub const HITBOX_INDEX: Self = Self(14);
    /// Where in that hitbox (vector); also a scratch vector of placing on the ground.
    pub const HITBOX_OFFSET_POSITION: Self = Self(15);
    /// Alternate alpha.
    pub const ALPHA_ALTERNATE: Self = Self(16);
    /// Scratch vector.
    pub const SCRATCH_VECTOR: Self = Self(17);
    /// Scratch float.
    pub const SCRATCH_FLOAT: Self = Self(18);
    /// No attribute: writes go nowhere.
    pub const DISABLED: Self = Self(19);
    /// Pitch in radians.
    pub const PITCH: Self = Self(20);
    /// Normal (vector).
    pub const NORMAL: Self = Self(21);
    /// Second scratch float.
    pub const SCRATCH_FLOAT_1: Self = Self(26);
    /// Third scratch float.
    pub const SCRATCH_FLOAT_2: Self = Self(27);
    /// Second scratch vector.
    pub const SCRATCH_VECTOR_2: Self = Self(30);
    /// The index of the parent's particle a particle was made from.
    pub const PARENT_PARTICLE_INDEX: Self = Self(33);
    /// How much forces move the particle.
    pub const FORCE_SCALE: Self = Self(34);
    /// The number of the parent's particle a particle was made from.
    pub const PARENT_PARTICLE_ID: Self = Self(46);
    /// The highest number the table has.
    const LAST: u8 = 48;

    /// The field of number `id`, if the table has it.
    pub fn from_id(id: i64) -> Option<Self> {
        u8::try_from(id)
            .ok()
            .filter(|id| *id <= Self::LAST)
            .map(Self)
    }

    /// Its number in the attribute table.
    pub fn id(self) -> u8 {
        self.0
    }

    /// Whether it holds an angle: stored in radians, authored in degrees.
    pub fn is_angle(self) -> bool {
        matches!(
            self,
            Self::ROLL | Self::ROLL_SPEED | Self::YAW | Self::PITCH
        )
    }

    /// The range set operators hold a value of it in.
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::ALPHA | Self::ALPHA_ALTERNATE => (0.0, 1.0),
            Self::RADIUS | Self::TRAIL_LENGTH => (0.0, f32::MAX),
            _ => (f32::MIN, f32::MAX),
        }
    }
}

impl TryFrom<u8> for Field {
    type Error = String;

    fn try_from(id: u8) -> Result<Self, Self::Error> {
        Self::from_id(i64::from(id)).ok_or_else(|| format!("particle field {id} is not known"))
    }
}

impl From<Field> for u8 {
    fn from(field: Field) -> Self {
        field.0
    }
}

/// How a value is written to an attribute (`ParticleSetMethod_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum SetMethod {
    /// The value replaces it.
    #[default]
    Replace,
    /// The value times the value at spawn.
    ScaleInitial,
    /// The value plus the value at spawn.
    AddToInitial,
    /// The current value plus the value per second.
    RampCurrent,
    /// The value times the current value.
    ScaleCurrent,
    /// The value plus the current value.
    AddToCurrent,
}

impl SetMethod {
    pub(super) const NAMES: [&str; 6] = [
        "PARTICLE_SET_REPLACE_VALUE",
        "PARTICLE_SET_SCALE_INITIAL_VALUE",
        "PARTICLE_SET_ADD_TO_INITIAL_VALUE",
        "PARTICLE_SET_RAMP_CURRENT_VALUE",
        "PARTICLE_SET_SCALE_CURRENT_VALUE",
        "PARTICLE_SET_ADD_TO_CURRENT_VALUE",
    ];

    pub(super) fn from_index(index: usize) -> Self {
        [
            Self::Replace,
            Self::ScaleInitial,
            Self::AddToInitial,
            Self::RampCurrent,
            Self::ScaleCurrent,
            Self::AddToCurrent,
        ][index.min(5)]
    }

    /// `value` written over `current`, whose value at spawn was `initial`, over a step of `dt`
    /// seconds.
    pub fn apply(self, value: f32, initial: f32, current: f32, dt: f32) -> f32 {
        match self {
            Self::Replace => value,
            Self::ScaleInitial => value * initial,
            Self::AddToInitial => value + initial,
            Self::RampCurrent => current + value * dt,
            Self::ScaleCurrent => value * current,
            Self::AddToCurrent => value + current,
        }
    }

    /// Whether it reads the value at spawn.
    pub fn reads_initial(self) -> bool {
        matches!(self, Self::ScaleInitial | Self::AddToInitial)
    }

    /// Whether a value for an angle is in degrees (all but the scales).
    pub fn takes_degrees(self) -> bool {
        !matches!(self, Self::ScaleInitial | Self::ScaleCurrent)
    }
}
