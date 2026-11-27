//! Sounds: Valve's `vsnd_c` as the audio file inside, a WAV or an MP3; the sound events
//! (`vsndevts_c`) that name them.

mod events;
mod graph;
mod lipsync;
mod mixer;
mod mp3;
mod operators;
pub mod soundscapes;
mod vsnd;
mod wav;

pub use events::{
    SoundDefaults, SoundEvent, SoundLayer, SoundLimits, SoundStack, read_sound_events,
    read_sound_stacks,
};
pub use graph::{ResolvedSoundEvent, SoundEventGraph};
pub use lipsync::{Lipsync, MorphCurves, Phoneme, token};
pub use mixer::{GroupLevel, LayerTrigger, Mix, MixGroup, MixLayer, SoundMixer, read_sound_mixer};
pub use operators::{
    SoundCurve, SoundEventLink, SoundFile, SoundFileTrack, SoundOperatorLimits, SoundPlayback,
    SoundSelectionMode, SoundSpatial, SoundTrack,
};
pub use vsnd::{Sound, SoundFormat};

#[cfg(test)]
mod graph_tests;
