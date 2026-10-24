//! Lists the files of an install's main package, with their sizes.
//!
//! ```sh
//! cargo run --example vpk_list -- "<...>/dota 2 beta/game/dota" models/heroes/pudge
//! ```

use heatel::{DirSource, Vpk};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let game = args
        .next()
        .ok_or("usage: vpk_list <game/dota> [path prefix]")?;
    let prefix = args.next().unwrap_or_default();
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk")?;
    let mut paths: Vec<(String, u64)> = pak
        .entries()
        .map(|entry| (entry.path(), entry.size()))
        .filter(|(path, _)| path.starts_with(&prefix))
        .collect();
    paths.sort();
    for (path, size) in &paths {
        println!("{size:>10} {path}");
    }
    eprintln!("{} of {} files", paths.len(), pak.len());
    Ok(())
}
