//! Layered animation metadata and authored bone and morph tracks.
#![no_main]

use heatel::Resource;
use heatel::anim::AnimationSet;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let Ok(set) = AnimationSet::read(&resource) else {
        return;
    };
    for index in 0..set.clips.len().min(8) {
        let _ = set.decode_clip(index);
    }
    let _ = set.decode_clip(set.clips.len());
});
