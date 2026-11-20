# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate follows
[Semantic Versioning](https://semver.org/).

## [0.2.0] - Unreleased

### Breaking

- `particle::System` gained the public fields `snapshot_file`, `snapshot` and `snapshot_control_point`,
  so a complete struct literal no longer compiles. `System`, `Snapshot`, `ControlPointRemap` and
  `ParticleNoise` are now `#[non_exhaustive]`: later fields and variants are not breaking. Read
  systems with `System::read`/`System::from_document` and match `Snapshot` with a wildcard arm.
- Converter revision 14 (`OUTPUT_REV`): caches made by 0.1.0 must be regenerated (particles; tooltip texts of items named only by their `:n` form).

### Added

- Read particle speed-to-scalar remaps and model equipment slots (converter revision 17).

- Read `C_INIT_RadiusFromCPObject` and resolve named model attachments in the model's rest frame.
- `physics::Physics`: a model's collision shapes (`PHYS`), its triangle meshes and convex hulls, each
  naming its collision group and interaction layers (a map's ground, its water, a trigger's volume);
  read within a `Budget`, every key it uses required, its limits public (`MAX_SHAPES`, ...).
- `world::GridNav`: a Dota 2 map's navigation grid (`maps/<map>.gnv`), each cell's flags and whether it
  is walkable.
- `locate`: Dota 2 installs in every Steam library of the machine (`libraryfolders.vdf`, bounded) on
  Linux (native, Flatpak, Snap), macOS and Windows (Program Files, or the Steam folder a caller reads from
  the registry), and in a folder picked at any usual level. `find_game_dir` searches them all; before it
  tried three folders under `$HOME`, none on Windows nor beyond the first library.
  A library list is the player's file, not a trusted one: an entry (or a registered Steam folder) that is
  relative, has a `..` step or a control character, or is a network path (`\\server\share`, `/\server\share`,
  `\\?\UNC`, device paths) is not looked at, and a folder given relative to the working folder is made a
  whole path first. The rule has two named steps: well formed as a local absolute path in either system's
  syntax (text only, the same on every host: the security rule, which `library_paths` applies; a path that is not
  valid UTF-8 is judged by its lossy text, and a step of only dots and spaces, which Windows strips, is refused
  like `..`), then usable on this host (a full path with no `..` step as this host reads it, and on Windows a
  drive prefix: what `libraries` and `steam_roots` add before anything is opened).
- `C_OP_RemapDirectionToCPToVector` reads as `Operator::RemapDirectionToTransformToVector`: a vector attribute
  (the normal) from the direction from a control point to the particle.
- Typed control-point remapping, root-skinned particle snapshots (`vsnap` DATA/SNAP),
  snapshot placement initializers and bounded Perlin particle float inputs.
- `particle::Look::depth_bias` (`m_flDepthBias`), a new public field: a complete `Look` literal needs it.
- `Default` for `ParticleNoise` and `ControlPointRemap` (the reader's defaults): the way to build
  these `#[non_exhaustive]` values outside the crate.
- Snapshot decoding limits: 5000 points, eight streams, 128 bounded strings,
  64 KiB metadata and 1 MiB unpacked stream data; unsupported skinning/streams are named.

### Fixed

- World props use their placement's skin, retaining each draw's material slot.
- World placements honor indexed per-draw and whole-model material overrides.
- Model and world tints are decoded and blended in linear space. Distinct placement tints
  produce distinct material bindings.
- World overlays are lifted one centimetre in mesh space. A short normal array is rejected.
  These world and material changes update the converter revision to 28.

- A snapshot with an unsupported attribute whose name and type together were long enough no longer
  fails its own `Snapshot::validate`: the name is cut to `MAX_UNSUPPORTED_NAME` where it is made.
- `tooltips::tooltips` named an item only by the plain localization key, so an item the localization
  names only as `DOTA_Tooltip_Ability_<name>:n` (the neutral items and the Chasm Stone) got no tooltip
  at all, not its labels or notes either. The `:n` key now names it when the plain key is absent.

## [0.1.0] - 2026-11-13

First release: Valve's Source 2 formats read and converted, tested on Dota 2 only.

### Added

- `Budget`: bounds what reading takes, so that a hostile file is an `Error`, never an unbounded
  allocation or loop; callers reading several files share one.
- `OUTPUT_REV`: bumped whenever an export's output changes for the same input, for caches made
  from exports.
- `find_game_dir` and `DirSource`: a Dota 2 install on disk (`HEATEL_DOTA_PATH` is read by the
  tests and the gate only, never by the library).
- `scripts`: the items and abilities of Dota's `items.txt`, `npc_abilities.txt` and the heroes'
  scripts.
- `sound::read_sound_mixer` reads Dota's `scripts/soundmixers.txt` (mix groups matched by the
  path of a sound, the levels of each mix, and the layers that duck groups while another
  sounds), and `kv3::parse_text` reads the text KeyValues3 it is written in, both bounded.
- `C_OP_ConstrainDistance` shell constraints, including legacy control-point fields and
  collection transform/vector inputs.
- `C_INIT_CreateSequentialPathV2`, read into the same `SequentialPath` as `C_INIT_CreateSequentialPath`:
  its `count` and `max_distance` are `FloatInput`s (a legacy scalar is a `Literal`; 100 and 0 when the
  file leaves them out). Values are passed through raw, fractional, negative or huge, so the
  consumer rounds and clamps them. An input kind heatel does not read is `FloatInput::Unsupported`
  with the kind's name.
- `C_OP_SetControlPointToCenter`, including its offset, live-mean and ancestor-scope settings.
- Exported meshes retain their render bounds in `extras.render_bounds` (`min` and
  `max` in glTF metres), for clients that release CPU vertex buffers after upload.
- In-place animation export by activity through `anim::read_clips_in_place` and
  `ModelOptions::in_place_activities`, retaining authored bone poses. Raw export keeps root
  trajectories by default. `ModelOptions::new` supports struct update syntax for defaults.

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
  constraint passes; paths (`Path`, `SequentialPath`, `MaintainPath`), maintain emitters
  (`Emitter::Maintain`), noise emitters and
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
