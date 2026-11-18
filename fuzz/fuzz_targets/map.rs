//! A map's navigation grid (`.gnv`) and a model's collision shapes (`PHYS`). No input panics;
//! every triangle names a vertex of its mesh.
#![no_main]

use heatel::Resource;
use heatel::physics::Physics;
use heatel::world::GridNav;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(nav) = GridNav::read(data) {
        let _ = nav.walkable(nav.min[0], nav.min[1]);
    }
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    if let Ok(physics) = Physics::read(&resource) {
        for mesh in &physics.meshes {
            let count = mesh.vertices.len();
            assert!(
                mesh.triangles
                    .iter()
                    .flatten()
                    .all(|&i| (i as usize) < count)
            );
        }
    }
});
