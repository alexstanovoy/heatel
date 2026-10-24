//! Whole models out of an install: a `vmdl_c` with its materials and animations as one `.glb`,
//! and the images it refers to.

use std::collections::HashMap;

use crate::anim::read_clips;
use crate::error::{Error, Result};
use crate::gltf::{GltfOptions, write_glb};
use crate::material::{Material, Pack};
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
#[cfg(feature = "encode")]
use crate::texture::PixelFormat;
use crate::texture::{Role, Texture};
use crate::vpk::Vpk;

/// A model converted to `.glb`, and the images its materials use, which the caller writes
/// next to it.
#[derive(Clone, Debug)]
pub struct ExportedModel {
    /// The binary glTF file.
    pub glb: Vec<u8>,
    /// The images the file refers to, sorted, each once per name.
    pub textures: Vec<ExportedTexture>,
    /// The animations written, in file order; a name asked for that the model lacks (or an
    /// additive clip) is not among them.
    pub clips: Vec<ExportedClip>,
}

/// An animation in the `.glb`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportedClip {
    /// The animation's name in the model.
    pub name: String,
    /// Seconds to its last frame, `(frames - 1) / fps`: a clip that ends holding a pose holds
    /// it to the end, as Dota plays it.
    pub duration: f32,
}

/// An image the `.glb` refers to.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExportedTexture {
    /// The texture (`materials/.../x.vtex`) it is made from.
    pub vtex: String,
    /// Its file name relative to the `.glb`.
    pub uri: String,
    /// How its pixels derive from the texture's: [`Pack::apply`]. `None` is the texture as it is.
    pub pack: Option<Pack>,
    /// What the material reads it as.
    pub role: Role,
}

/// A compiled file (`models/.../x.vmdl_c`, `scripts/npc/items.txt`) from the first of
/// `packages` that has it.
pub fn read_from(src: &(impl Source + ?Sized), packages: &[&Vpk], path: &str) -> Result<Vec<u8>> {
    let vpk = packages
        .iter()
        .find(|vpk| vpk.get(path).is_some())
        .ok_or_else(|| Error::NotFound(path.to_owned()))?;
    vpk.read_file(src, path)
}

/// What [`export_model`] writes besides the model itself.
#[derive(Clone, Copy)]
pub struct ModelOptions<'a> {
    /// Only these render meshes, when given (by their names in the model).
    pub meshes: Option<&'a [&'a str]>,
    /// Whether to write the skeleton and skin (and then the animations of `anims`).
    pub skeleton: bool,
    /// The animations to write, by name; a name the model lacks is skipped.
    pub anims: &'a [&'a str],
    /// The image file a texture (`.vtex` path) is written to, relative to the `.glb`.
    pub texture_uri: &'a dyn Fn(&str) -> String,
    /// What the file says wrote it (glTF's `asset.generator`).
    pub generator: &'a str,
}

/// Fills in the morph targets of the kept meshes that have a morph set, out of its atlas. A
/// mesh of several vertex buffers keeps none: its morph grid is laid over one buffer's vertices.
fn morph_targets(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    model: &mut Model,
    meshes: Option<&[&str]>,
) -> Result<()> {
    let kept = model
        .meshes
        .iter_mut()
        .filter(|m| meshes.is_none_or(|keep| keep.contains(&m.name.as_str())));
    for mesh in kept {
        let (Some(set), [vertices]) = (&mesh.morph, mesh.vertices.as_slice()) else {
            continue;
        };
        let atlas = Texture::parse(&read_from(src, packages, &format!("{}_c", set.atlas))?)?;
        let size = (atlas.width() as usize, atlas.height() as usize);
        mesh.targets = set.targets(&atlas.to_rgba8()?, size, vertices.len())?;
    }
    Ok(())
}

/// Converts the model at `path` (`models/.../x.vmdl_c`) from the first of `packages` that has
/// it, with the materials it uses read from the same packages.
///
/// The `.glb` names each image by `options.texture_uri`; write each of
/// [`ExportedModel::textures`] there with [`ExportedTexture::to_png`] or
/// [`ExportedTexture::to_ktx2`].
pub fn export_model(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    options: &ModelOptions<'_>,
) -> Result<ExportedModel> {
    export_model_in_group(src, packages, path, options, None)
}

/// [`export_model`] with the model's skin `group` (a name of its material groups, `"1"` for the
/// sentry ward's) in place of its default one: each draw call's material is swapped for the one
/// the group lists at the same place. A group the model lacks is an error.
pub fn export_model_in_group(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    options: &ModelOptions<'_>,
    group: Option<&str>,
) -> Result<ExportedModel> {
    let ModelOptions {
        meshes,
        skeleton,
        anims,
        texture_uri,
        generator,
    } = *options;
    let bytes = read_from(src, packages, path)?;
    let resource = Resource::parse(&bytes)?;
    let mut model = Model::read(&resource)?;
    if let Some(group) = group {
        model.use_material_group(group)?;
    }
    morph_targets(src, packages, &mut model, meshes)?;
    let clips = if skeleton {
        read_clips(&resource, anims)?
    } else {
        Vec::new()
    };
    let mut materials = HashMap::new();
    let used = model
        .meshes
        .iter()
        .filter(|m| meshes.is_none_or(|keep| keep.contains(&m.name.as_str())))
        .flat_map(|m| &m.primitives)
        .map(|p| p.material.as_str());
    for material in used {
        if materials.contains_key(material) || material.is_empty() {
            continue;
        }
        let bytes = read_from(src, packages, &format!("{material}_c"))?;
        let resource = Resource::parse(&bytes)?;
        if let Some(data) = resource.data() {
            materials.insert(material.to_owned(), Material::from_vmat(material, &data?)?);
        }
    }
    let options = GltfOptions {
        name: path,
        meshes,
        skeleton,
        materials: &materials,
        texture_uri,
        clips: &clips,
        generator,
    };
    let (glb, mut textures) = write_glb(&model, &options)?;
    textures.sort();
    Ok(ExportedModel {
        glb,
        textures,
        clips: clips
            .iter()
            .map(|c| ExportedClip {
                name: c.name.clone(),
                duration: c.duration(),
            })
            .collect(),
    })
}

impl ExportedTexture {
    /// The image as a PNG of its largest level: `source` (the decoded [`vtex`](Self::vtex))
    /// with its channels [packed](Pack::apply) when the image is made of them.
    pub fn to_png(&self, source: &Texture) -> Result<Vec<u8>> {
        match self.pack {
            None => source.to_png(),
            Some(pack) => {
                let mut pixels = source.to_rgba8()?;
                pack.apply(&mut pixels);
                Texture::from_rgba8(source.width(), source.height(), pixels)?.to_png()
            }
        }
    }

    /// The image as a KTX2 file with every mip level, block-compressed: `source` (the decoded
    /// [`vtex`](Self::vtex)) as Dota stores it where a GPU samples it so (its block format, its
    /// levels); a DXT5 colour without its alpha as BC1, exactly; an image made from the texture
    /// (packed channels, a normal Dota keeps in DXT5's alpha and green, a PNG it stores)
    /// compressed from its decoded levels to BC5 for a normal, BC3 for an image with alpha, BC1
    /// otherwise.
    #[cfg(feature = "encode")]
    pub fn to_ktx2(&self, source: &Texture) -> Result<Vec<u8>> {
        let encoding = self.role.encoding();
        let image = match self.pack {
            None if source.gpu_ready() && source.format() != PixelFormat::Rgba8 => source.clone(),
            Some(Pack::Rgb) if source.format() == PixelFormat::Bc3 => source.bc3_colour_as_bc1()?,
            pack => {
                let mut levels = source.levels_rgba8()?;
                if let Some(pack) = pack {
                    levels.iter_mut().for_each(|level| pack.apply(level));
                }
                let translucent = levels[0].as_chunks::<4>().0.iter().any(|px| px[3] < 255);
                let format = match (self.role, pack) {
                    // Its blue is worked out from red and green when drawn.
                    (Role::Normal, None | Some(Pack::Rgb)) => PixelFormat::Bc5,
                    (_, None) if translucent => PixelFormat::Bc3,
                    _ => PixelFormat::Bc1,
                };
                Texture::from_rgba8_levels(source.width(), source.height(), levels)?
                    .with_mips(encoding)
                    .compressed(format, encoding)?
            }
        };
        Ok(image.to_ktx2(encoding))
    }
}
