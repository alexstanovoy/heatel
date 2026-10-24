//! Prints a compiled resource's data as JSON: its `DATA` block (KV3, or NTRO read into the
//! same values), or the KV3 block named.
//!
//! ```sh
//! cargo run --features serde --example kv3_to_json -- "<...>/game/dota" \
//!     models/heroes/pudge/pudge.vmdl_c CTRL
//! ```

use heatel::{DirSource, Resource, Vpk};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (game, path, block) = match &args[..] {
        [game, path] => (game, path, None),
        [game, path, block] => (game, path, Some(block)),
        _ => return Err("usage: kv3_to_json <game/dota> <resource path> [block]".into()),
    };
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk")?;
    let bytes = pak.read_file(&src, path)?;
    let resource = Resource::parse(&bytes)?;
    let value = match block {
        None => resource.data().ok_or("the resource has no DATA block")??,
        Some(name) => {
            let kind: [u8; 4] = name
                .as_bytes()
                .try_into()
                .map_err(|_| "a block is 4 letters")?;
            resource.kv3(&kind).ok_or("no such KV3 block")??.root
        }
    };
    let stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(stdout, &value)?;
    println!();
    Ok(())
}
