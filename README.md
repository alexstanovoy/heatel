# heatel

Reads Valve's Source 2 file formats and converts them to open ones: models with their skeletons, animations and faces
to glTF, textures to PNG or KTX2, sounds to WAV or MP3, particle systems and KV3 data to JSON. Pure Rust, no `unsafe` in
the crate, fuzzed against hostile files, and it runs in a browser on files the user picks, reading only the bytes it
needs.

**Tested on Dota 2, and only on Dota 2.** Another Source 2 game's files may use versions or variants heatel does not
read; mostly they are an [`Error`] that names what was met, see [Limits](#limits). A few
helpers read Dota 2's own scripts and maps and mean nothing elsewhere, see [Dota 2 data](#dota-2-data).

## Source 2 formats

| File | heatel reads it as | and writes |
|---|---|---|
| `*.vpk` packages | [`Vpk`](vpk::Vpk): the directory tree, archives, CRCs | the stored files |
| any compiled `*_c` file | [`Resource`](resource::Resource): its blocks, references, `DATA` | |
| KV3 blocks (binary and text), NTRO data | [`kv3::Value`] | JSON (feature `serde`) |
| KV1 text, localization | [`keyvalues::Kv`], [`keyvalues::localization_table`] | |
| `vtex_c` textures | [`Texture`](texture::Texture): every level, block formats as stored | PNG, KTX2 |
| `vsnd_c` sounds | [`Sound`](sound::Sound), a voice line's [`Lipsync`](sound::Lipsync) (phonemes, baked morph curves) | the WAV or MP3 inside |
| a mesh's `MRPH` morph set and `_vmorf.vtex` atlas, phoneme tables, `vcd_c` scenes | [`MorphSet`](face::MorphSet) (flexes, controllers, rules), [`PhonemeTable`](face::PhonemeTable), [`Scene`](face::Scene) | glTF morph targets |
| `vmdl_c` models, `vmat_c` materials, embedded animations | [`Model`](model::Model), [`Material`](material::Material), [`Clip`](anim::Clip) | `.glb` with PNG or KTX2 images |
| a model's sequences | [`read_sequences`](anim::read_sequences): activities, frames, events | |
| `vpcf_c` particle systems | [`System`](particle::System): emitters, initializers, operators, forces, renderers, children | JSON (feature `serde`) |
| `vwrld_c` worlds, `vents_c` entity lumps | [`EntityLump`](world::EntityLump), [`placements`](world::placements) | `.gltf` + `.bin` |
| `vsndevts_c` sound events, `vrr_c` response rules | [`read_sound_events`](sound::read_sound_events), [`read_response_rules`](talker::read_response_rules) | |

## Dota 2 data

These know Dota 2's file names, scripts or entity classes, so they are of no use for another game:

| Dota 2's file | heatel reads it as |
|---|---|
| the Steam install | [`find_game_dir`] finds its `game/dota` folder |
| `items.txt`, `npc_abilities.txt`, the heroes' scripts, localization | [`scripts::entries`], [`tooltips`](tooltips::tooltips) |
| `shops.txt` | [`shops::categories`] |
| `portraits.txt` | [`portraits::framings`] |
| `items_game.txt` | [`items_game::read_hero_items`]: the cosmetics a hero wears |
| `soundmixers.txt` | [`sound::read_sound_mixer`] |
| the map's `ent_dota_lightinfo` entities | [`world::lighting`] |
| the map's mid-lane diagonal | [`world::Band`] cuts a world to a band along it |
| `AE_DOTA_PLAY_SOUND_ATTACK` animation events | [`anim::EventData::AttackSound`] |

**heatel ships no Valve content and is not affiliated with or endorsed by Valve Corporation. Dota 2 and Source 2 are
trademarks of Valve Corporation.** It reads files you already have, such as your own Dota 2 install, and what you
convert stays subject to Valve's terms for your copy of the game.

## Quick start

```toml
[dependencies]
heatel = "0.1"
```

Open a Dota 2 install (its `game/dota` folder) and read from its main package:

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
    skeleton: true,
    generator: "my tool",
    ..ModelOptions::new(&png)
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
`FileReaderSync` in a Web Worker), put the files in a [`SliceSource`] under the paths they have in
the game folder (Dota 2: `pak01_dir.vpk`, `pak01_042.vpk`, `maps/dota.vpk`), and use it as a folder.
Only the bytes asked for are read.

## Untrusted input

Every reader treats its file as hostile: reads are range-checked, allocations, recursion and
work are bounded by what a real file of the kind needs, and a malformed file is an
[`Error`], never a panic. The readers are fuzzed (`fuzz/`, libFuzzer on stable Rust), and heatel's
own code has no `unsafe` (`#![forbid(unsafe_code)]`).

## Limits

heatel is built and tested against Dota 2. What it reads of each format is what Dota 2's files use, and the rest is an
[`Error`] that names it: a resource header other than version 12, a VPK other than version 1 or 2, KV3 other than
versions 1 to 5 and the first `VKV\x03` layout, a `vtex_c` other than version 1 or one that is a cube map, volume, array
or JPEG, a sound that is not 8- or 16-bit PCM or MP3, a vertex attribute in a format it has no reader for. Three cases are
softer: a particle class heatel does not read is kept as `Unsupported` and listed by
[`System::unsupported_classes`](particle::System::unsupported_classes); a texture parameter of a shader that heatel has no
table for is taken as one input named after the parameter; an animation channel in a codec it does not read is left out.

It reads and converts, never writes Valve's formats. Materials are mapped by a table of each shader's texture parameters
rather than by running Valve's compiled shaders, so base colour and normals are right and the other channels are
approximate. Particle systems are read as definitions; simulating and drawing them is up to you.

## Features

- `encode` (default): BC1/BC3/BC4/BC5 compression of RGBA8 images
  ([`Texture::compressed`](texture::Texture::compressed),
  [`ExportedTexture::to_ktx2`](export::ExportedTexture::to_ktx2)).
- `serde`: `Serialize` for KV3 documents and values; `Serialize` and `Deserialize` for particle
  systems.

## Tests with an install

The tests that read a Dota 2 install are ignored by default; `HEATEL_DOTA_PATH` names a `dota 2 beta` folder when it
is not in the usual Steam places. They were recorded on Dota 2 patch 7.41 (the counts in their comments, 18 675 models,
83 312 particle systems and 946 sound event files, are that patch's; the tests themselves only require lower bounds on
them), and their expectations on individual models, particle systems, sounds and scripts are the values that patch
holds. Another patch may change a value and fail one:

```sh
cargo test --release -- --ignored
```

## Minimum Rust version

1.88.

## Acknowledgements

heatel stands on the reverse-engineering work of the
[ValveResourceFormat](https://github.com/ValveResourceFormat/ValveResourceFormat) project (VRF, the library behind
Source 2 Viewer; MIT licensed). heatel's glTF exports of materials, animation clips, vertex attributes and worlds, its
sprite sheet crop and a few particle operators are written to give the same results as Source 2 Viewer's, which was
the reference to compare against. One part is derived from its code, not only compatible with it: the table of which
inputs the channels of each shader's texture parameters hold, with the rules that match them to glTF's slots
(`src/material/inputs.rs`), and the constants of the sRGB to linear conversion (`src/material/convert.rs`). Its
copyright and licence are in [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES).

The libraries heatel builds on (meshoptimizer through `meshoptimizer-rs`, `lz4_flex`, `ruzstd`, `texpresso`,
`texture2ddecoder`, `png` and others) are listed in `Cargo.toml`.

## License

MIT, see the `LICENSE` file. Material derived from other projects is listed in `THIRD-PARTY-NOTICES`.
