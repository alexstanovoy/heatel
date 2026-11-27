//! Typed world geometry and environment from a world and its named in-memory files.
#![no_main]

use heatel::fuzzing::files;
use heatel::world::{WorldDescriptors, WorldEnvironment};
use libfuzzer_sys::fuzz_target;

const WORLD: &str = "maps/w/world.vwrld_c";

fuzz_target!(|data: &[u8]| {
    let parts = files::split(data);
    let pack = files::resolve(&[WORLD], &parts);
    let source = files::install(&[("maps/w.vpk", &pack)]);
    let Some(package) = files::open(&source, "maps/w.vpk") else {
        return;
    };
    let _ = WorldDescriptors::read(&source, &[&package], WORLD);
    let _ = WorldEnvironment::read(&source, &[&package], WORLD);
});
