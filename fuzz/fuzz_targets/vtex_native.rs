//! Native texture mips, image addressing, integer/float decoding and embedded streams.
#![no_main]

use heatel::texture::{Encoding, Rgba8Options, VTex, embedded_image};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = embedded_image(data);
    let Ok(texture) = VTex::parse(data) else {
        return;
    };
    let meta = texture.meta();
    for level in 0..=meta.mip_count {
        let Some(mip) = texture.mip(level) else {
            continue;
        };
        let [width, height, depth] = mip.dimensions();
        // Decode bounded images so one mutation cannot exhaust the fuzzing worker.
        if u64::from(width) * u64::from(height) > 256 * 256 {
            continue;
        }
        for layer in [0, depth.saturating_sub(1), depth] {
            for face in [0, meta.flags.face_count() - 1, meta.flags.face_count()] {
                let _ = mip.image(layer, face);
                let _ = mip.decode_rgba8(layer, face, Rgba8Options::default());
                for encoding in [None, Some(Encoding::Srgb), Some(Encoding::Linear)] {
                    let _ = mip.decode_linear_rgba32f(layer, face, encoding);
                }
            }
        }
    }
});
