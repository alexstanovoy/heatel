//! Sounds: Valve's `vsnd_c` as the audio file inside, a WAV or an MP3.

mod lipsync;
mod mp3;
mod vsnd;
mod wav;

pub use lipsync::{Lipsync, MorphCurves, Phoneme, token};
pub use vsnd::{Sound, SoundFormat};
