# heatel

Reads Valve's Source 2 formats out of a local Dota 2 install and converts them to open ones:

| Dota's file | heatel reads it as | and writes |
|---|---|---|
| `*.vpk` packages | [`Vpk`](vpk::Vpk): the directory tree, archives, CRCs | the stored files |
| any compiled `*_c` file | [`Resource`](resource::Resource): its blocks, references, `DATA` | |
| KV3 blocks, NTRO data | [`kv3::Value`] | JSON (feature `serde`) |
| scripts, localization (KV1 text) | [`keyvalues::Kv`], [`keyvalues::localization_table`] | |
| `vtex_c` textures | [`Texture`](texture::Texture): every level, block formats as stored | PNG, KTX2 |
| `vsnd_c` sounds | [`Sound`](sound::Sound) | the WAV or MP3 inside |
| `vmdl_c` models, `vmat_c` materials, embedded animations | [`Model`](model::Model), [`Material`](material::Material), [`Clip`](anim::Clip) | `.glb` with PNG or KTX2 images |
| `vwrld_c` worlds, `vents_c` entity lumps | [`EntityLump`](world::EntityLump), [`placements`](world::placements), [`lighting`](world::lighting) | `.gltf` + `.bin` |
| `items.txt`, `npc_abilities.txt`, `shops.txt`, `portraits.txt` | [`tooltips`](tooltips::tooltips), [`categories`](shops::categories), [`framings`](portraits::framings) | |

**heatel reads files from your own Dota 2 install. It ships no Valve content and is not
affiliated with or endorsed by Valve Corporation. Dota 2 and Source 2 are trademarks of Valve
Corporation.** What you convert stays subject to Valve's terms for your copy of the game.

## Quick start

```toml
[dependencies]
heatel = "0.1"
```

Open the install (its `game/dota` folder) and read from its main package:

```rust,no_run
use heatel::{DirSource, Texture, Vpk, find_game_dir};

let game = find_game_dir(None).expect("no Dota 2 in the usual Steam places");
let src = DirSource::new(game);
let pak = Vpk::open(&src, "pak01_dir.vpk")?;
for entry in pak.entries().filter(|e| e.ext() == "vtex_c").take(5) {
    let texture = Texture::parse(&pak.read(&src, entry)?)?;
    println!("{}: {} x {} {:?}", entry.path(), texture.width(), texture.height(), texture.format());
}
# Ok::<(), heatel::Error>(())
```

A model with its materials, skeleton and animations as a `.glb`, its images as PNG:

```rust,no_run
use heatel::export::{ModelOptions, export_model};
use heatel::{DirSource, Texture, Vpk, read_from};

let src = DirSource::new("/path/to/dota 2 beta/game/dota");
let pak = Vpk::open(&src, "pak01_dir.vpk")?;
let png = |vtex: &str| format!("{}.png", vtex.rsplit('/').next().unwrap_or(vtex));
let options = ModelOptions {
    meshes: None,
    skeleton: true,
    anims: &[],
    texture_uri: &png,
    generator: "my tool",
};
let model = export_model(&src, &[&pak], "models/heroes/pudge/pudge.vmdl_c", &options)?;
std::fs::write("pudge.glb", &model.glb).unwrap();
for image in &model.textures {
    let texture = Texture::parse(&read_from(&src, &[&pak], &format!("{}_c", image.vtex))?)?;
    std::fs::write(&image.uri, image.to_png(&texture)?).unwrap();
}
# Ok::<(), heatel::Error>(())
```

The [`examples`](https://docs.rs/crate/heatel/latest/source/examples/) do the common jobs from
the command line: `vpk_list`, `vpk_extract`, `model_to_gltf`, `texture_convert` (PNG or KTX2),
`sound_extract` (WAV or MP3) and `kv3_to_json` (`--features serde`), e.g.

```sh
cargo run --release --example model_to_gltf -- "<...>/dota 2 beta/game/dota" \
    models/heroes/pudge/pudge.vmdl_c out/ idle
```

## In a browser

Everything builds for `wasm32-unknown-unknown`. Reads go through the [`Source`] trait, which is
synchronous: implement [`Slice`] over a `File` the user picked (`File.slice` read with
`FileReaderSync` in a Web Worker), put the files in a [`SliceSource`] under their paths in
`game/dota` (`pak01_dir.vpk`, `pak01_042.vpk`, `maps/dota.vpk`), and use it as a folder.
Only the bytes asked for are read.

## Untrusted input

Every reader treats its file as hostile: reads are range-checked, allocations, recursion and
work are bounded by what a real file of the kind needs, and a malformed file is an
[`Error`], never a panic. The readers are fuzzed (`fuzz/`, libFuzzer on stable Rust), and the
crate has no `unsafe` code (`#![forbid(unsafe_code)]`).

## Features

- `encode` (default): BC1/BC3/BC4/BC5 compression of RGBA8 images
  ([`Texture::compressed`](texture::Texture::compressed),
  [`ExportedTexture::to_ktx2`](export::ExportedTexture::to_ktx2)).
- `serde`: `Serialize` for KV3 documents and values.

## Tests with an install

Tests that need Dota's own files are ignored by default; `HEATEL_DOTA_PATH` names a
`dota 2 beta` folder when it is not in the usual Steam places:

```sh
cargo test --release -- --ignored
```

## Minimum Rust version

1.88.

## License

MIT, see the `LICENSE` file.
