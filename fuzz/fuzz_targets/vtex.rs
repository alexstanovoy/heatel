//! A `vtex_c` texture and every way it is written: its first sprite frame, the KTX2 file, the
//! RGBA8 pixels, the PNG files, the images packed from its channels, and a model's image of
//! each role and packing (small textures only: compressing is slow). No input panics.
#![no_main]

use heatel::export::ExportedTexture;
use heatel::material::Pack;
use heatel::texture::{Encoding, Role, Texture};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(texture) = Texture::parse(data) else {
        return;
    };
    let frame = texture.first_frame();
    for texture in [&texture, &frame] {
        let _ = texture.to_ktx2(Encoding::Srgb);
        let _ = texture.to_png();
        let Ok(mut pixels) = texture.to_rgba8() else {
            continue;
        };
        for pack in [Pack::Channel(1), Pack::Orm, Pack::Rgb] {
            pack.apply(&mut pixels);
        }
        if let Ok(packed) = Texture::from_rgba8(texture.width(), texture.height(), pixels) {
            let _ = packed.to_png();
        }
    }
    let _ = texture.first_frame_png();
    if u64::from(texture.width()) * u64::from(texture.height()) > 256 * 256 {
        return;
    }
    for role in [Role::Colour, Role::Normal, Role::Data] {
        for pack in [
            None,
            Some(Pack::Channel(3)),
            Some(Pack::Orm),
            Some(Pack::Rgb),
        ] {
            let image = ExportedTexture {
                vtex: "materials/t.vtex".to_owned(),
                uri: "t.ktx2".to_owned(),
                pack,
                role,
            };
            let _ = image.to_ktx2(&texture);
            let _ = image.to_png(&texture);
        }
    }
});
