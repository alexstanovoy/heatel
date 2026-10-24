//! What moves a face: a mesh's morph set (its flexes as morph targets, and the controllers and
//! rules that weigh them), Valve's phoneme tables and facial choreography scenes. A voice
//! line's own lip sync is [`sound::Lipsync`](crate::sound::Lipsync).

mod morph;
mod phonemes;
mod vcd;

pub use morph::{Controller, Flex, MorphSet, Op, Target};
pub use phonemes::{PhonemeSetting, PhonemeTable};
pub use vcd::{FlexEvent, FlexTrack, RampKey, Scene};
