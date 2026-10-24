//! An entity lump (`.vents_c`): its entities and child lumps, the models it places, what they
//! place and the map's lighting. No input panics; every placement is finite.
#![no_main]

use heatel::Resource;
use heatel::world::{EntityLump, lighting, placements};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let Ok(lump) = EntityLump::read(&resource) else {
        return;
    };
    let _ = lump.models();
    for p in placements(&lump.entities) {
        let numbers = [p.origin[0], p.origin[1], p.origin[2], p.yaw, p.scale];
        assert!(numbers.iter().all(|n| n.is_finite()), "{p:?}");
    }
    let _ = lighting(&lump.entities);
});
