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
pub mod keyvalues;
pub mod kv3;
#[cfg(any(unix, windows))]
mod locate;
/// Materials (`vmat_c`): shader, parameters, textures, and their glTF form.
pub mod material;
pub mod meshopt;
pub mod model;
mod ntro;
/// Particle systems (`vpcf_c`): typed definitions of their emitters, initializers, operators,
/// forces, renderers and children.
pub mod particle;
pub mod portraits;
/// Compiled Source 2 resources: the block container every `*_c` file is.
pub mod resource;
pub mod shops;
pub mod sound;
/// Where files are read from: a folder on disk, or in-memory and browser files.
pub mod source;
pub mod texture;
pub mod tooltips;
pub mod vbib;
pub mod vertices;
/// VPK packages: a directory file and its numbered archives.
pub mod vpk;
pub mod world;

pub use error::{Error, Result};
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
