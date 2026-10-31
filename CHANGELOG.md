# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/).

## [0.1.0] - unreleased

First release.

### Added

- `vpk`: VPK v1/v2 packages (directory and numbered archives, preload data, CRC checks).
- `resource`: compiled Source 2 resources and their block table, external references, `DATA`
  as KV3 or NTRO.
- `kv3`: binary KeyValues3 (legacy, LZ4 and zstd forms); `serde` feature for `Serialize`.
- `keyvalues`: KeyValues (KV1) text and localization tables.
- `texture`: `vtex_c` textures (block, ETC2/EAC and plain formats, PNG-stored), BC1, BC3, BC4,
  BC5 and BC7 decoding, sprite sheets (`Texture::sheet`: sequences and frames), mip generation,
  BC1/BC3/BC4/BC5 encoding (`encode` feature), PNG and KTX2 output, KTX2 block decoding
  (`ktx2_to_plain`).
- `sound`: `vsnd_c` sounds as the WAV or MP3 file they hold; a voice line's lip sync
  (`Lipsync`: its phonemes, emphasis and baked morph curves, in both the binary and the KV3
  layout; `token`, Source 2's string token hash the curves are named by).
- `face`: a mesh's morph set (`MorphSet`: flex deltas out of the `_vmorf` atlas, controllers,
  flex rules), Valve's phoneme tables (`PhonemeTable`) and facial choreography scenes (`Scene`);
  exported models carry face meshes' flexes as glTF morph targets, their rig in the mesh's extras.
- `model`, `vbib`, `vertices`, `meshopt`: models, their meshes (CTRL buffers, `VBIB` blocks,
  meshoptimizer and zstd compression), skeletons, skins and attachments.
- `material`: `vmat_c` materials and their glTF form.
- `anim`: embedded animation clips, each lasting to its last frame, `(frames - 1) / fps`, a pose
  held to the end included.
- `anim::read_sequences`: a model's sequences without decoding a frame: activities with their
  modifiers and weights, looping, frames, frame rate, and animation events by frame (sounds,
  particles with their configuration, attack sounds, speech, props, tag suppression).
- `sound::read_sound_events`: a `vsndevts_c`'s events with their files, volume, pitch jitter, sound
  level, distance, mix group, fades and layers.
- `talker::read_response_rules`: a `vrr_c`'s response groups with their requirements and
  responses, its criteria and includes.
- `items_game::read_hero_items`: the hero cosmetics of `items_game.txt` (slot, model, default
  cosmetics, `particle_create`, `particle`, `sound`, `activity` and control point modifiers),
  streamed from the 52 MB file.
- `particle::control_point_configurations`: a `vpcf_c`'s control point drivers and preview state.
- `export::export_model_in_group`: a model drawn with one of its skins (material groups), such as
  the sentry ward's.
- `gltf`, `export`: models with materials, skeleton, attachments (nodes under their bones) and
  clips as `.glb`, their images as PNG or KTX2.
- `particle`: `vpcf_c` particle systems as typed definitions (emitters, initializers,
  operators, forces, renderers, children; inputs, curves and gradients), upgraded from the
  format they are stored in, every value the file leaves out at the schema's default; `serde`
  feature for them. Pre-emission operators (`System::pre_emission`) and projected decals
  (`Renderer::Projected`, their materials by `System::materials`), refraction
  (`Look::refract`) and deferred lights (`Renderer::DeferredLight`) included. Constraints read
  as typed functions (`System::constraints`: `Constraint`), with `C_OP_BasicMovement`'s
  constraint passes; paths (`Path`, `SequentialPath`, `MaintainPath`), noise emitters and
  forces (`NoiseEmitter`, `CreationNoise`, `CurlNoise`), boxes, epitrochoids, transform
  remaps, model functions (`OnModel`, `LockToBone`) and model renderers (`Renderer::Models`,
  their models by `System::models`) read, with the control point operators of a town portal
  (`PreEmission::SetSingleControlPointPosition`, `ScalarExpressionToCp`, `DistanceBetweenCps`
  and others). A renderer's texture layers: each one's channel mix (`Look::channels`,
  `TextureChannels`) and the layers after the first (`Look::layers`: `TextureLayer`, combined
  by `LayerBlend`); `System::textures` lists theirs too.
- `world`: entity lumps, placements, map lighting, and worlds exported as glTF.
- `tooltips`, `shops`, `portraits`: Dota's item and ability tooltips, shop categories and
  portrait framings.
- `source`: installs on disk (`DirSource`, `find_game_dir`) or in memory and in a browser
  (`SliceSource`).
