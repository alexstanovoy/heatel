//! Converts a texture to PNG (its largest level, or a sprite sheet's first frame) or to KTX2
//! (every level, block-compressed as a GPU samples it).
//!
//! ```sh
//! cargo run --release --example texture_convert -- "<...>/game/dota" \
//!     materials/models/heroes/pudge/pudge2_apron_color_psd_ec2cd72e.vtex_c apron.png
//! ```

use heatel::export::ExportedTexture;
use heatel::texture::Role;
use heatel::{DirSource, Texture, Vpk};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [game, vtex, out] = &args[..] else {
        return Err("usage: texture_convert <game/dota> <...vtex_c> <out.png | out.ktx2>".into());
    };
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk")?;
    let texture = Texture::parse(&pak.read_file(&src, vtex)?)?;
    let bytes = if out.ends_with(".ktx2") {
        let image = ExportedTexture {
            vtex: vtex.trim_end_matches("_c").to_owned(),
            uri: out.clone(),
            pack: None,
            role: Role::Colour,
        };
        image.to_ktx2(&texture)?
    } else {
        texture.first_frame_png()?
    };
    std::fs::write(out, &bytes)?;
    eprintln!(
        "{vtex}: {} x {} {:?}, {} levels",
        texture.width(),
        texture.height(),
        texture.format(),
        texture.mips().len()
    );
    Ok(())
}
