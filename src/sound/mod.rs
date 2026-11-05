//! Sounds: Valve's `vsnd_c` as the audio file inside, a WAV or an MP3; the sound events
//! (`vsndevts_c`) that name them.

mod events;
mod lipsync;
mod mp3;
mod vsnd;
mod wav;

pub use events::{
    SoundEvent, SoundLayer, SoundLimits, SoundStack, read_sound_events, read_sound_stacks,
};
pub use lipsync::{Lipsync, MorphCurves, Phoneme, token};
pub use vsnd::{Sound, SoundFormat};
