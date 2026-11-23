//! A map's world as one glTF (`.gltf` and its `.bin`), the images it refers to and the layer
//! textures of its terrain.

use std::rc::Rc;

use super::crop::{Band, Cut, crop};
use super::scene::{Builder, MAX_SCENE_BYTES, MAX_WORLD_READ_BYTES};
use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::export::{ExportedTexture, read_from_in};
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
use crate::vpk::Vpk;

/// Shaders whose materials blend several layers (terrain) or animate (water): their textures
/// are exported whole, beside the glTF that carries only one layer.
const LAYERED: [&str; 2] = ["multiblend.vfx", "water_dota.vfx"];
/// Decoded models kept while the band is written (its nodes come model by model).
const MODEL_CACHE: usize = 8;
/// Most live decoded bytes held in the model cache at once.
const MAX_CACHE_BYTES: usize = 512 << 20;

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

/// A model omitted from a world export because it could not be read.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SkippedModel {
    /// The model path, without the compiled suffix.
    pub path: String,
    /// Why the model could not be exported.
    pub reason: String,
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
    /// Malformed models omitted from the world, each reported once.
    pub skipped_models: Vec<SkippedModel>,
}

/// Exports the world at `path` (`maps/.../world.vwrld_c`) from `packages` (the map's package
/// first, then `pak01_dir.vpk` for the models and materials it places).
pub fn export_world(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    options: &WorldOptions<'_>,
) -> Result<ExportedWorld> {
    let (scene, mut total_read) = Builder::new(src, packages, options.texture_uri).build(path)?;
    let mut cache: Vec<(usize, Rc<Model>, usize)> = Vec::new();
    let mut cache_bytes: usize = 0;
    let mut load = |index: usize| -> Result<Rc<Model>> {
        if let Some((_, model, _)) = cache.iter().find(|(i, _, _)| *i == index) {
            return Ok(Rc::clone(model));
        }
        let model_path = &scene.models[index];
        let mut model_budget = Budget::with_limit("world", MAX_SCENE_BYTES);
        let decoded = read_from_in(src, packages, &format!("{model_path}_c"), &mut model_budget)
            .and_then(|bytes| Model::read_in(&Resource::parse(&bytes)?, &mut model_budget));
        let spent = model_budget.spent();
        total_read = total_read.saturating_add(spent as u64);
        if total_read > MAX_WORLD_READ_BYTES {
            return Err(malformed(
                "world",
                format!("reading more than {MAX_WORLD_READ_BYTES} bytes"),
            ));
        }
        let model = Rc::new(decoded?);
        while !cache.is_empty()
            && (cache.len() == MODEL_CACHE || cache_bytes.saturating_add(spent) > MAX_CACHE_BYTES)
        {
            let (_, _, evicted) = cache.remove(0);
            cache_bytes = cache_bytes.saturating_sub(evicted);
        }
        if spent <= MAX_CACHE_BYTES {
            cache_bytes += spent;
            cache.push((index, Rc::clone(&model), spent));
        }
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
        skipped_models: scene.skipped_models,
    })
}
