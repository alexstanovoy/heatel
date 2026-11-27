//! Source 2 material parameters and typed shader views.
#![no_main]

use heatel::material::Source2Material;
use heatel::{Budget, Resource};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let Ok(material) = Source2Material::read(&resource, &mut Budget::new(32 << 20)) else {
        return;
    };
    let _ = material.hero_params();
    let _ = material.terrain_params();
    let _ = material.parameters().count();
    let _ = material.attributes().count();
    let _ = material.textures().count();
});
