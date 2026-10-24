//! Copies one file out of an install's main package, as it is stored (compiled files keep
//! their `_c` form).
//!
//! ```sh
//! cargo run --example vpk_extract -- "<...>/game/dota" scripts/npc/items.txt items.txt
//! ```

use heatel::{DirSource, Vpk};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [game, path, out] = &args[..] else {
        return Err("usage: vpk_extract <game/dota> <path in the package> <output file>".into());
    };
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk")?;
    let bytes = pak.read_file(&src, path)?;
    std::fs::write(out, &bytes)?;
    eprintln!("{path}: {} bytes", bytes.len());
    Ok(())
}
