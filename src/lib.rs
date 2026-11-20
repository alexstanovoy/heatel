// The README is the crate's front page on GitHub, crates.io and docs.rs alike. Its API links are reference links whose
// definitions in the README point at docs.rs; the same labels are defined here first, as intra-doc paths, and the first
// definition of a label wins, so rustdoc links the crate's own items.
#![doc = r"
[`Error`]: Error
[`Vpk`]: vpk::Vpk
[`Resource`]: resource::Resource
[`kv3::Value`]: kv3::Value
[`keyvalues::Kv`]: keyvalues::Kv
[`keyvalues::localization_table`]: keyvalues::localization_table
[`Texture`]: texture::Texture
[`Sound`]: sound::Sound
[`Lipsync`]: sound::Lipsync
[`MorphSet`]: face::MorphSet
[`PhonemeTable`]: face::PhonemeTable
[`Scene`]: face::Scene
[`Model`]: model::Model
[`Material`]: material::Material
[`Clip`]: anim::Clip
[`read_sequences`]: anim::read_sequences
[`System`]: particle::System
[`EntityLump`]: world::EntityLump
[`Physics`]: physics::Physics
[`placements`]: world::placements
[`read_sound_events`]: sound::read_sound_events
[`read_response_rules`]: talker::read_response_rules
[`find_game_dir`]: find_game_dir
[`locate`]: locate
[`scripts::entries`]: scripts::entries
[`tooltips`]: tooltips::tooltips
[`shops::categories`]: shops::categories
[`portraits::framings`]: portraits::framings
[`items_game::read_hero_items`]: items_game::read_hero_items
[`sound::read_sound_mixer`]: sound::read_sound_mixer
[`world::lighting`]: world::lighting
[`world::Band`]: world::Band
[`world::GridNav`]: world::GridNav
[`anim::EventData::AttackSound`]: anim::EventData::AttackSound
[`Source`]: Source
[`Slice`]: Slice
[`SliceSource`]: SliceSource
[`System::unsupported_classes`]: particle::System::unsupported_classes
[`Texture::compressed`]: texture::Texture::compressed
[`ExportedTexture::to_ktx2`]: export::ExportedTexture::to_ktx2
"]
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

/// Animation clips of a model's embedded animation, and what its sequences say about when they
/// play (activities) and what happens in them (events).
pub mod anim;
mod budget;
mod bytes;
mod error;
pub mod export;
pub mod face;
#[cfg(fuzzing)]
#[doc(hidden)]
pub mod fuzzing;
pub mod gltf;
/// The cosmetics of `items_game.txt`: the models and effects a hero wears, by item.
pub mod items_game;
pub mod keyvalues;
pub mod kv3;
#[cfg(any(unix, windows))]
pub mod locate;
/// Materials (`vmat_c`): shader, parameters, textures, and their glTF form.
pub mod material;
pub mod meshopt;
pub mod model;
mod ntro;
/// Particle systems (`vpcf_c`): typed definitions of their emitters, initializers, operators,
/// forces, renderers and children.
pub mod particle;
/// A model's collision shapes (`PHYS`): triangle meshes and convex hulls with their collision groups.
pub mod physics;
pub mod portraits;
/// Compiled Source 2 resources: the block container every `*_c` file is.
pub mod resource;
pub mod scripts;
pub mod shops;
pub mod sound;
/// Where files are read from: a folder on disk, or in-memory and browser files.
pub mod source;
/// Response rules (`vrr_c`): which voice line a unit says when.
pub mod talker;
pub mod texture;
pub mod tooltips;
pub mod vbib;
pub mod vertices;
/// VPK packages: a directory file and its numbered archives.
pub mod vpk;
pub mod world;

pub use budget::Budget;
pub use error::{Error, Result};

/// Bumped whenever an export's output changes for the same input (a converter's fix, a changed
/// format of what it writes): a consumer's cache fingerprint includes it, so caches made by an
/// older converter are made again.
pub const OUTPUT_REV: u32 = 29;
pub use export::{export_model, export_model_in_group, read_from};
#[cfg(any(unix, windows))]
pub use locate::find_game_dir;
pub use model::Model;
pub use resource::Resource;
pub use sound::Sound;
#[cfg(any(unix, windows))]
pub use source::DirSource;
pub use source::{Slice, SliceSource, Source};
pub use texture::Texture;
pub use vpk::Vpk;
