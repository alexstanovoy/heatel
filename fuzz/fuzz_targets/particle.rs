//! A particle system (`.vpcf_c`): its definition read into typed functions, renderers and
//! children, and checked again as a cache reads it back. No input panics.
#![no_main]

use heatel::Resource;
use heatel::particle::System;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let Ok(system) = System::read(&resource) else {
        return;
    };
    assert!(system.validate().is_ok());
    let _ = system.unsupported_classes();
    let _ = system.textures().count();
    let _ = system.materials().count();
});
