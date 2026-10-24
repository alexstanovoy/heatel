//! Dota's `portraits.txt`: the framings of the models its own entries name. No input panics;
//! every number of a framing is finite.
#![no_main]

use heatel::fuzzing::files;
use heatel::portraits::framings;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut models: Vec<String> = files::named(data)
        .iter()
        .filter_map(|m| Some(format!("{}.vmdl", m.strip_suffix(".vmdl_c")?)))
        .collect();
    models.truncate(64);
    let wanted: Vec<(&str, &str)> = models.iter().map(|m| (m.as_str(), m.as_str())).collect();
    let Ok(framed) = framings(data, &wanted) else {
        return;
    };
    for f in framed {
        let (c, l) = (f.camera, f.light);
        let numbers = [
            c.position, c.angles, l.position, l.angles, l.color, l.ambient,
        ]
        .concat();
        let singles = [c.fov, c.far, l.fov, l.scale, l.ambient_scale];
        assert!(
            numbers.iter().chain(&singles).all(|n| n.is_finite()),
            "{f:?}"
        );
        if let Some(b) = f.background {
            assert!(
                b.tints
                    .as_flattened()
                    .iter()
                    .all(|n| (0.0..=1.0).contains(n))
            );
        }
    }
});
