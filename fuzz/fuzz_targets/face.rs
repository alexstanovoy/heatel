//! What moves a face: a morph set (a KV3 `MRPH` block) and its deltas out of an atlas, a
//! choreography scene (a KV3 `vcd3` document) and a phoneme table. The input is split at its
//! first NUL: KV3 before (read as a morph set and as a scene), text after. No input panics, and
//! every delta read is finite.
#![no_main]

use heatel::face::{MorphSet, PhonemeTable, Scene};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let split = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let (kv3, text) = (&data[..split], data.get(split + 1..).unwrap_or_default());
    if let Ok(doc) = heatel::kv3::decode(kv3) {
        if let Ok(set) = MorphSet::read(&doc.root, "atlas") {
            // A small atlas of the input's own bytes.
            let atlas: Vec<u8> = (0..16 * 16 * 4).map(|i| data[i % data.len()]).collect();
            if let Ok(targets) = set.targets(&atlas, (16, 16), 64) {
                for t in targets {
                    assert!(t.positions.iter().chain(&t.normals).flatten().all(|v| v.is_finite()));
                }
            }
        }
        let _ = Scene::read(&doc.root);
    }
    let _ = PhonemeTable::parse(text);
});
