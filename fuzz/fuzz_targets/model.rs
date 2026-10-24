//! A model exported whole: its meshes (CTRL buffers or a VBIB block,
//! raw, zstd or meshopt), materials, skeleton and the animations its own strings name,
//! written as `.glb`. The input is the model, then the files it names (`heatel::fuzzing::files`).
//! No input panics.
#![no_main]

use heatel::fuzzing::files;

use heatel::export::{ModelOptions, export_model};
use libfuzzer_sys::fuzz_target;

const MODEL: &str = "models/m.vmdl_c";

fuzz_target!(|data: &[u8]| {
    let parts = files::split(data);
    let pack = files::resolve(&[MODEL], &parts);
    let src = files::install(&[("pak01_dir.vpk", &pack)]);
    let Some(vpk) = files::open(&src, "pak01_dir.vpk") else {
        return;
    };
    let mut names = files::strings(parts[0].1);
    names.truncate(64);
    let anims: Vec<&str> = names.iter().map(String::as_str).collect();
    let uri = |vtex: &str| format!("{vtex}.png");
    let options = |meshes, skeleton, anims| ModelOptions {
        meshes,
        skeleton,
        anims,
        texture_uri: &uri,
        generator: "fuzz",
    };
    for skeleton in [false, true] {
        let _ = export_model(&src, &[&vpk], MODEL, &options(None, skeleton, &anims));
    }
    let first = [anims.first().copied().unwrap_or_default()];
    let _ = export_model(&src, &[&vpk], MODEL, &options(Some(&first), false, &[]));
});
