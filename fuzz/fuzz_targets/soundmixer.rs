//! A text KV3 script and a sound mixer file: no input panics, hangs or allocates without bound.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = heatel::kv3::parse_text(data);
    let _ = heatel::sound::read_sound_mixer(data, &mut heatel::Budget::default());
});
