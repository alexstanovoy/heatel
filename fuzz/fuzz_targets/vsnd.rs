//! A `vsnd_c` sound, the audio file made of it and its lip sync: no input panics, and every
//! phoneme read runs forwards.
#![no_main]

use heatel::Sound;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(sound) = Sound::parse(data) {
        let _ = sound.file();
    }
    if let Ok(lipsync) = heatel::sound::Lipsync::parse(data) {
        assert!(lipsync.phonemes.iter().all(|p| p.start <= p.end));
    }
});
