//! A map's world as one glTF (`.gltf` and its `.bin`), the images it refers to and the layer
//! textures of its terrain.

use std::rc::Rc;

use super::crop::{Band, Cut, crop};
use super::scene::Builder;
use crate::error::{Result, malformed};
use crate::export::{ExportedTexture, read_from};
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
use crate::vpk::Vpk;

/// Shaders whose materials blend several layers (terrain) or animate (water): their textures
/// are exported whole, beside the glTF that carries only one layer.
const LAYERED: [&str; 2] = ["multiblend.vfx", "water_dota.vfx"];
/// Decoded models kept while the band is written (its nodes come model by model).
const MODEL_CACHE: usize = 8;

/// What [`export_world`] keeps of the world and how it names its files.
#[derive(Clone, Copy)]
pub struct WorldOptions<'a> {
    /// Keeps only the nodes whose bounds touch this band; every node when none.
    pub band: Option<Band>,
    /// Nodes left out by name prefix (`tree_`, `radiant_tower`, ...). The lower-detail layers
    /// (`n0_lr1_...` and up) are always left out.
    pub exclude: &'a [&'a str],
    /// The name of the document's scene.
    pub scene: &'a str,
    /// The file name of the geometry, as the document's buffer names it.
    pub bin_uri: &'a str,
    /// The image file a texture (`.vtex` path) is written to, relative to the `.gltf`.
    pub texture_uri: &'a dyn Fn(&str) -> String,
    /// What the file says wrote it (glTF's `asset.generator`).
    pub generator: &'a str,
}

/// A world cut to a band, ready to write.
#[derive(Clone, Debug)]
pub struct ExportedWorld {
    /// The `.gltf` document.
    pub gltf: Vec<u8>,
    /// Its geometry, the file its buffer names.
    pub bin: Vec<u8>,
    /// The images its materials use, written next to it.
    pub textures: Vec<ExportedTexture>,
    /// The layer textures of its terrain and water materials (`.vtex` paths without `_c`),
    /// sorted by their compiled paths, each once: the materials blend or animate them, so the
    /// document carries one layer and these are the textures to write whole.
    pub layers: Vec<String>,
}

/// Exports the world at `path` (`maps/.../world.vwrld_c`) from `packages` (the map's package
/// first, then `pak01_dir.vpk` for the models and materials it places).
pub fn export_world(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    options: &WorldOptions<'_>,
) -> Result<ExportedWorld> {
    let scene = Builder::new(src, packages, options.texture_uri).build(path)?;
    let mut cache: Vec<(usize, Rc<Model>)> = Vec::new();
    let mut load = |index: usize| -> Result<Rc<Model>> {
        if let Some((_, model)) = cache.iter().find(|(i, _)| *i == index) {
            return Ok(Rc::clone(model));
        }
        let model_path = &scene.models[index];
        let bytes = read_from(src, packages, &format!("{model_path}_c"))?;
        let model = Rc::new(Model::read(&Resource::parse(&bytes)?)?);
        if cache.len() == MODEL_CACHE {
            cache.remove(0);
        }
        cache.push((index, Rc::clone(&model)));
        Ok(model)
    };
    let cut = Cut {
        band: options.band.as_ref(),
        exclude: options.exclude,
        scene: options.scene,
        bin_uri: options.bin_uri,
        generator: options.generator,
    };
    let cropped = crop(&scene, &cut, &mut load)?;
    let gltf =
        serde_json::to_vec(&cropped.doc).map_err(|e| malformed("world", format!("glTF: {e}")))?;
    let images: Vec<usize> = cropped.images;
    let mut layers: Vec<String> = cropped
        .materials
        .iter()
        .map(|&m| &scene.materials[m])
        .filter(|m| LAYERED.contains(&m.shader.as_str()))
        .flat_map(|m| m.textures.iter())
        .cloned()
        .collect();
    layers.sort_by_key(|t| format!("{t}_c"));
    layers.dedup();
    Ok(ExportedWorld {
        gltf,
        bin: cropped.bin,
        textures: images
            .iter()
            .map(|&i| scene.images.list[i].clone())
            .collect(),
        layers,
    })
}
