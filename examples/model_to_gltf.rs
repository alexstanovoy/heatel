//! Converts a model with its materials, skeleton and the animations named to a `.glb` and the
//! PNG images it refers to, which any glTF viewer opens.
//!
//! ```sh
//! cargo run --release --example model_to_gltf -- "<...>/game/dota" \
//!     models/heroes/pudge/pudge.vmdl_c out/ idle run
//! ```

use std::path::Path;

use heatel::export::{ModelOptions, export_model};
use heatel::{DirSource, Texture, Vpk, read_from};

/// The PNG beside the `.glb` that a texture (`materials/.../x.vtex`) is written to.
fn png_name(vtex: &str) -> String {
    let file = vtex.rsplit('/').next().unwrap_or(vtex);
    format!("{}.png", file.trim_end_matches(".vtex"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [game, model, out, anims @ ..] = &args[..] else {
        return Err(
            "usage: model_to_gltf <game/dota> <models/...vmdl_c> <output folder> [animation...]"
                .into(),
        );
    };
    let anims: Vec<&str> = anims.iter().map(String::as_str).collect();
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk")?;
    let options = ModelOptions {
        meshes: None,
        skeleton: true,
        anims: &anims,
        texture_uri: &|vtex| png_name(vtex),
        generator: "heatel model_to_gltf",
    };
    let exported = export_model(&src, &[&pak], model, &options)?;
    let out = Path::new(out);
    std::fs::create_dir_all(out)?;
    let stem = model.rsplit('/').next().unwrap_or(model);
    let glb = out.join(format!("{}.glb", stem.trim_end_matches(".vmdl_c")));
    std::fs::write(&glb, &exported.glb)?;
    for image in &exported.textures {
        let source = Texture::parse(&read_from(&src, &[&pak], &format!("{}_c", image.vtex))?)?;
        std::fs::write(out.join(&image.uri), image.to_png(&source)?)?;
    }
    let clips: Vec<&str> = exported.clips.iter().map(|c| c.name.as_str()).collect();
    eprintln!(
        "{}: {} images, clips {clips:?}",
        glb.display(),
        exported.textures.len()
    );
    Ok(())
}
