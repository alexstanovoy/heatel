//! Model rigs and case-insensitive wearable bone merging, with an optional second rig.
#![no_main]

use heatel::Resource;
use heatel::fuzzing::files;
use heatel::model::ModelRig;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let parts = files::split(data);
    let first = Resource::parse(data).or_else(|_| Resource::parse(parts[0].1));
    let Ok(resource) = first else { return };
    let Ok(rig) = ModelRig::read(&resource) else {
        return;
    };
    assert_eq!(rig.bone_merge_map(&rig).len(), rig.bones.len());
    let _ = rig.bone_merge_map(&ModelRig {
        bones: Vec::new(),
        attachments: Vec::new(),
        submodels: Vec::new(),
    });
    if let Some((_, bytes)) = parts.get(1)
        && let Ok(resource) = Resource::parse(bytes)
        && let Ok(base) = ModelRig::read(&resource)
    {
        let _ = rig.bone_merge_map(&base);
        let _ = base.bone_merge_map(&rig);
    }
});
