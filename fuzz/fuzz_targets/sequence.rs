//! A model's sequences (the `ASEQ` and `ANIM` blocks read without decoding a frame): their
//! activities, lengths and events, however the strings and counts of the blocks lie. No input
//! panics, and the work stays within the bounds of the format.
#![no_main]

use heatel::anim::read_sequences;
use heatel::Resource;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let Ok(list) = read_sequences(&resource) else {
        return;
    };
    for s in &list {
        assert!(s.fps > 0.0 && s.fps.is_finite());
        let _ = (s.duration(), s.time_of(u32::MAX));
    }
});
