//! A world (`.vwrld_c`) exported: its nodes (`.vwnod_c`),
//! entity lumps, the models and materials they place, cut to a band that keeps everything, and uncut.
//! The input is the world, then the files it names (`heatel::fuzzing::files`). No input panics.
#![no_main]

use heatel::fuzzing::files;

use heatel::world::{Band, WorldOptions, export_world};
use libfuzzer_sys::fuzz_target;

const WORLD: &str = "maps/w/world.vwrld_c";

fuzz_target!(|data: &[u8]| {
    let parts = files::split(data);
    let pack = files::resolve(&[WORLD], &parts);
    let src = files::install(&[("maps/w.vpk", &pack)]);
    let Some(vpk) = files::open(&src, "maps/w.vpk") else {
        return;
    };
    let band = Band {
        half_width: 1e9,
        v_min: -1e9,
        v_max: 1e9,
    };
    let uri = |vtex: &str| format!("{vtex}.png");
    for band in [Some(band), None] {
        let options = WorldOptions {
            band,
            exclude: &["tree_"],
            scene: "w",
            bin_uri: "w.bin",
            texture_uri: &uri,
            generator: "fuzz",
        };
        let _ = export_world(&src, &[&vpk], WORLD, &options);
    }
});
