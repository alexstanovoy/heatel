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
- `sound`: `vsnd_c` sounds as the WAV or MP3 file they hold.
- `model`, `vbib`, `vertices`, `meshopt`: models, their meshes (CTRL buffers, `VBIB` blocks,
  meshoptimizer and zstd compression), skeletons, skins and attachments.
- `material`: `vmat_c` materials and their glTF form.
- `anim`: embedded animation clips, each lasting to its last frame, `(frames - 1) / fps`, a pose
  held to the end included.
- `gltf`, `export`: models with materials, skeleton, attachments (nodes under their bones) and
  clips as `.glb`, their images as PNG or KTX2.
- `particle`: `vpcf_c` particle systems as typed definitions (emitters, initializers,
  operators, forces, renderers, children; inputs, curves and gradients), upgraded from the
  format they are stored in, every value the file leaves out at the schema's default; `serde`
  feature for them. Pre-emission operators (`System::pre_emission`) and projected decals
  (`Renderer::Projected`, their materials by `System::materials`) and refraction
  (`Look::refract`) included.
- `world`: entity lumps, placements, map lighting, and worlds exported as glTF.
- `tooltips`, `shops`, `portraits`: Dota's item and ability tooltips, shop categories and
  portrait framings.
- `source`: installs on disk (`DirSource`, `find_game_dir`) or in memory and in a browser
  (`SliceSource`).
