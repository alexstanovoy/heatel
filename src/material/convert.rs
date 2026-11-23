//! A material as Source2Viewer 20 wrote it into glTF (`GenerateGLTFMaterialFromRenderMaterial`
//! with adapted textures): its colour factor from the tint, alpha mode, and which image of which
//! texture's channels feeds each slot.

use std::collections::HashMap;

use serde_json::{Map, Value as Json, json};

use super::inputs::{self, Channels, RGB, RGBA, Remap, Slot};
use super::look::GroundLayerLook;
use super::pack::{Pack, orm_uri};
use super::vmat::{Alpha, Material};
use crate::error::{Result, malformed};
use crate::export::ExportedTexture;
use crate::texture::Role;

/// A glTF material; its textures are indices into the [`Images`] it was made with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GltfMaterial {
    pub name: String,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub alpha: Alpha,
    pub cutoff: Option<f32>,
    pub double_sided: bool,
    pub unlit: bool,
    /// `KHR_materials_specular`'s colour factor, when the material has the extension.
    pub specular: Option<[f32; 3]>,
    pub base_color_texture: Option<usize>,
    pub normal_texture: Option<usize>,
    pub metallic_roughness_texture: Option<usize>,
    pub occlusion_texture: Option<usize>,
    pub emissive_texture: Option<usize>,
    pub emissive: Option<[f32; 3]>,
    pub specular_texture: Option<usize>,
    pub extras: Json,
    pub look: Option<Json>,
}

/// The images materials use, each made once: the first material that uses a texture decides
/// how its image is made, as Source2Viewer names images after their texture alone.
#[derive(Default)]
pub(crate) struct Images {
    pub list: Vec<ExportedTexture>,
    by_key: HashMap<String, usize>,
}

impl Images {
    fn get(&mut self, key: String, make: impl FnOnce() -> ExportedTexture) -> usize {
        *self.by_key.entry(key).or_insert_with(|| {
            self.list.push(make());
            self.list.len() - 1
        })
    }
}

/// `material` drawn with `tint`, its images named by `texture_uri`.
pub(crate) fn convert(
    material: &Material,
    tint: [f32; 4],
    images: &mut Images,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<GltfMaterial> {
    convert_world(material, linear_tint(tint), images, texture_uri)
}

/// Convert a material with its model and placement tint already in linear space.
pub(crate) fn convert_world(
    material: &Material,
    tint: [f32; 4],
    images: &mut Images,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<GltfMaterial> {
    let mut color = tint;
    if let Some(amount) = material.float("g_flModelTintAmount") {
        color = color.map(|component| 1.0 + (component - 1.0) * amount);
    }
    if let Some([red, green, blue, _]) = material.vector("g_vColorTint") {
        let tint = srgb_to_linear([red, green, blue]);
        for (component, tint) in color.iter_mut().zip(tint) {
            *component *= tint;
        }
        color[3] = 1.0;
    }
    convert_with_color(material, clamp_color(color), images, texture_uri)
}

fn convert_with_color(
    material: &Material,
    base_color: [f32; 4],
    images: &mut Images,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<GltfMaterial> {
    let translucent = material.int("F_TRANSLUCENT") > 0 || material.shader.ends_with("_glass.vfx");
    let alpha_test = material.int("F_ALPHA_TEST") > 0;
    let remaps: Vec<(&str, Vec<Remap>)> = material
        .textures
        .iter()
        .map(|(key, path)| (path.as_str(), inputs::remaps(material, key)))
        .collect();
    let specular = remaps
        .iter()
        .any(|(_, r)| r.iter().any(|r| r.slot == Slot::SpecularFactor));
    let look = if let Some(figure) = material.figure_look()? {
        Some(figure.to_json())
    } else if let Some(water) = material.water_look()? {
        Some(water.to_json())
    } else {
        material
            .ground_layers()?
            .map(|layers| GroundLayerLook::layers_to_json(&layers))
    };
    let mut out = GltfMaterial {
        name: material.name.clone(),
        base_color,
        metallic: material
            .float("g_flMetalness")
            .filter(|m| m.is_finite())
            .map_or(0.0, |m| m.clamp(0.0, 1.0)),
        alpha: if translucent {
            Alpha::Blend
        } else if alpha_test {
            Alpha::Mask
        } else {
            Alpha::Opaque
        },
        cutoff: material
            .float("g_flAlphaTestReference")
            .filter(|c| alpha_test && c.is_finite()),
        double_sided: material.int("F_RENDER_BACKFACES") > 0,
        unlit: material.int("F_UNLIT") > 0,
        specular: specular.then(|| {
            material
                .vector("g_vSpecularColor")
                .filter(|v| v.iter().all(|c| c.is_finite()))
                .map_or([1.0; 3], |v| [v[0], v[1], v[2]])
        }),
        base_color_texture: None,
        normal_texture: None,
        metallic_roughness_texture: None,
        occlusion_texture: None,
        emissive_texture: None,
        emissive: None,
        specular_texture: None,
        extras: material.extras.clone(),
        look,
    };
    tie_textures(&mut out, remaps, images, texture_uri)?;
    if out.emissive_texture.is_some() && material.int("F_SELF_ILLUM") > 0 {
        let [r, g, b, _] = clamp_color(material.vector("g_vSelfIllumTint").unwrap_or([1.0; 4]));
        let scale = material
            .float("g_flSelfIllumScale")
            .filter(|scale| scale.is_finite())
            .unwrap_or(1.0)
            .max(0.0);
        out.emissive = Some(srgb_to_linear([r, g, b]).map(|channel| channel * scale));
    }
    Ok(out)
}

/// Decode an authored tint's RGB. Alpha is already a linear coverage factor.
pub(crate) fn linear_tint([red, green, blue, alpha]: [f32; 4]) -> [f32; 4] {
    let [red, green, blue] = srgb_to_linear([red, green, blue]);
    [red, green, blue, alpha]
}

fn clamp_color(color: [f32; 4]) -> [f32; 4] {
    // A number that is not finite (only a damaged file has one) leaves the colour white.
    color.map(|c| {
        if c.is_finite() {
            c.clamp(0.0, 1.0)
        } else {
            1.0
        }
    })
}

/// sRGB-encoded colour to linear, with Source2Viewer's rounding (derived from ValveResourceFormat's,
/// MIT: see THIRD-PARTY-NOTICES).
pub(crate) fn srgb_to_linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|c| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            (2.4 * c.mul_add(1.0 / 1.055, 0.055 / 1.055).ln()).exp() - 0.000_000_1
        }
    })
}

/// Puts each texture's image into the slots it feeds; the metalness goes into an
/// occlusion/roughness/metallic image of its own.
fn tie_textures(
    out: &mut GltfMaterial,
    remaps: Vec<(&str, Vec<Remap>)>,
    images: &mut Images,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<()> {
    // One entry per texture, at its first parameter's place, with its last parameter's remaps.
    let mut by_texture: Vec<(&str, Vec<Remap>)> = Vec::new();
    for (path, list) in remaps.into_iter().filter(|(_, l)| !l.is_empty()) {
        match by_texture.iter_mut().find(|(p, _)| *p == path) {
            Some(slot) => slot.1 = list,
            None => by_texture.push((path, list)),
        }
    }
    let orm_occlusion = by_texture
        .iter()
        .any(|(_, l)| l.iter().any(|r| r.slot == Slot::MetallicRoughness));
    let mut orm: Vec<(&str, Vec<Remap>)> = Vec::new();
    for (path, list) in &mut by_texture {
        let (packed, rest) = list
            .iter()
            .partition(|r| matches!(r.slot, Slot::Occlusion | Slot::MetallicRoughness));
        if !Vec::is_empty(&packed) {
            orm.push((path, packed));
        }
        *list = rest;
    }
    for (path, list) in &by_texture {
        let Some(main) = list.first() else {
            continue;
        };
        let file = path.rsplit('/').next().unwrap_or(path);
        let pack = pack_of(main, path)?;
        let role = match main.slot {
            Slot::BaseColor | Slot::Emissive => Role::Colour,
            Slot::Normal => Role::Normal,
            _ => Role::Data,
        };
        let image = images.get(file.to_owned(), || ExportedTexture {
            vtex: (*path).to_owned(),
            uri: texture_uri(path),
            pack,
            role,
        });
        tie(out, main.slot, image, orm_occlusion);
    }
    if !orm.is_empty() {
        let image = orm_image(&orm, images, texture_uri)?;
        tie(out, Slot::MetallicRoughness, image, orm_occlusion);
    }
    Ok(())
}

fn tie(out: &mut GltfMaterial, slot: Slot, image: usize, orm_occlusion: bool) {
    match slot {
        Slot::BaseColor => out.base_color_texture = Some(image),
        Slot::Normal => out.normal_texture = Some(image),
        Slot::MetallicRoughness => {
            out.metallic_roughness_texture = Some(image);
            out.metallic = 1.0;
            if orm_occlusion {
                out.occlusion_texture = Some(image);
            }
        }
        Slot::SpecularFactor => out.specular_texture = Some(image),
        Slot::Occlusion => out.occlusion_texture = Some(image),
        Slot::Emissive => out.emissive_texture = Some(image),
    }
}

/// How the image of a texture feeding a slot through `remap` is made from it.
fn pack_of(remap: &Remap, path: &str) -> Result<Option<Pack>> {
    let from = if remap.from == RGBA && remap.to == RGB {
        RGB
    } else {
        remap.from
    };
    match from.0 {
        [c] => Ok(Some(Pack::Channel(*c))),
        [0, 1] | [0, 1, 2] => Ok(Some(Pack::Rgb)),
        [0, 1, 2, 3] => Ok(None),
        other => Err(malformed(
            "material",
            format!("{path}: channels {other:?} for a glTF slot"),
        )),
    }
}

/// The occlusion/roughness/metallic image. Only the form Dota's materials take is made: one
/// texture's blue channel as the metalness.
fn orm_image(
    orm: &[(&str, Vec<Remap>)],
    images: &mut Images,
    texture_uri: &dyn Fn(&str) -> String,
) -> Result<usize> {
    let blue = |r: &Remap| channel(r.from) == 2 && channel(r.to) == 2;
    match orm {
        [(path, list)] if list.iter().all(blue) => {
            Ok(images.get(format!("orm:{path}"), || ExportedTexture {
                vtex: (*path).to_owned(),
                uri: orm_uri(&texture_uri(path)),
                pack: Some(Pack::Orm),
                role: Role::Data,
            }))
        }
        _ => {
            let paths: Vec<&str> = orm.iter().map(|(p, _)| *p).collect();
            Err(malformed(
                "material",
                format!("occlusion/roughness/metallic packing of {paths:?}"),
            ))
        }
    }
}

/// The channel an ORM input reads or writes: a single one, else red.
fn channel(c: Channels) -> u8 {
    match c.0 {
        [one] => *one,
        _ => 0,
    }
}

impl GltfMaterial {
    /// The glTF material, its textures given by `texture` from their image's index; factors at
    /// glTF's defaults are left out.
    pub(crate) fn to_json(&self, texture: impl Fn(usize) -> usize) -> Json {
        let slot = |image: Option<usize>| image.map(|i| json!({ "index": texture(i) }));
        let mut pbr = Map::new();
        if self.base_color != [1.0; 4] {
            pbr.insert("baseColorFactor".into(), json!(self.base_color));
        }
        if let Some(t) = slot(self.base_color_texture) {
            pbr.insert("baseColorTexture".into(), t);
        }
        if self.metallic != 1.0 {
            pbr.insert("metallicFactor".into(), json!(self.metallic));
        }
        if let Some(t) = slot(self.metallic_roughness_texture) {
            pbr.insert("metallicRoughnessTexture".into(), t);
        }
        let mut out = json!({ "name": self.name, "pbrMetallicRoughness": pbr });
        for (key, image) in [
            ("normalTexture", self.normal_texture),
            ("occlusionTexture", self.occlusion_texture),
            ("emissiveTexture", self.emissive_texture),
        ] {
            if let Some(t) = slot(image) {
                out[key] = t;
            }
        }
        match self.alpha {
            Alpha::Opaque => {}
            Alpha::Mask => out["alphaMode"] = json!("MASK"),
            Alpha::Blend => out["alphaMode"] = json!("BLEND"),
        }
        if let Some(cutoff) = self.cutoff.filter(|&c| c != 0.5) {
            out["alphaCutoff"] = json!(cutoff);
        }
        if self.double_sided {
            out["doubleSided"] = json!(true);
        }
        let mut extensions = Map::new();
        if let Some(color) = self.emissive {
            let strength = color.into_iter().fold(1.0, f32::max);
            out["emissiveFactor"] = json!(color.map(|channel| channel / strength));
            if strength > 1.0 {
                extensions.insert(
                    "KHR_materials_emissive_strength".into(),
                    json!({ "emissiveStrength": strength }),
                );
            }
        }
        if let Some(color) = self.specular {
            let mut specular = Map::new();
            if color != [1.0; 3] {
                specular.insert("specularColorFactor".into(), json!(color));
            }
            if let Some(t) = slot(self.specular_texture) {
                specular.insert("specularTexture".into(), t);
            }
            extensions.insert("KHR_materials_specular".into(), Json::Object(specular));
        }
        if self.unlit {
            extensions.insert("KHR_materials_unlit".into(), json!({}));
        }
        if !extensions.is_empty() {
            out["extensions"] = Json::Object(extensions);
        }
        let mut extras = Map::new();
        extras.insert("vmat".into(), self.extras.clone());
        if let Some(look) = &self.look {
            extras.insert("look".into(), look.clone());
        }
        out["extras"] = Json::Object(extras);
        out
    }
}

/// The extensions `materials` (glTF JSON) use, for `extensionsUsed`.
pub(crate) fn extensions_used(materials: &[Json]) -> Vec<String> {
    let mut used: Vec<String> = materials
        .iter()
        .filter_map(|m| m.get("extensions")?.as_object())
        .flat_map(|e| e.keys().cloned())
        .collect();
    used.sort();
    used.dedup();
    used
}

#[cfg(test)]
mod tint_tests {
    use super::*;
    use crate::kv3::{Object, Value};

    #[test]
    fn enabled_self_illumination_exports_its_authored_factor() {
        let mut material =
            Material::from_vmat("glow.vmat", &Value::Object(Object::default())).unwrap();
        material.shader = "hero.vfx".into();
        material
            .textures
            .push(("g_tMasks1".into(), "masks.vtex".into()));
        material.ints.push(("F_SELF_ILLUM".into(), 1));
        material
            .vectors
            .push(("g_vSelfIllumTint".into(), [0.5, 1.0, 0.0, 1.0]));
        material.floats.push(("g_flSelfIllumScale".into(), 0.5));
        let mut images = Images::default();
        let gltf = convert(&material, [1.0; 4], &mut images, &str::to_owned).unwrap();
        let json = gltf.to_json(|index| index);
        assert!(json.get("emissiveTexture").is_some());
        let factor = json["emissiveFactor"]
            .as_array()
            .expect("the authored factor");
        for (component, expected) in factor.iter().zip([0.107_020_5, 0.5, 0.0]) {
            assert!((component.as_f64().unwrap() - expected).abs() < 0.000_01);
        }
        material.ints.clear();
        let disabled = convert(&material, [1.0; 4], &mut images, &str::to_owned).unwrap();
        assert!(
            disabled
                .to_json(|index| index)
                .get("emissiveFactor")
                .is_none()
        );
    }

    #[test]
    fn model_tint_weight_blends_the_linear_color_on_both_paths() {
        let mut material =
            Material::from_vmat("surface.vmat", &Value::Object(Object::default())).unwrap();
        material.floats.push(("g_flModelTintAmount".into(), 0.5));
        let stored = [0.5, 0.5, 0.5, 1.0];
        let linear = [0.214_041, 0.214_041, 0.214_041, 1.0];
        let mut images = Images::default();
        let model = convert(&material, stored, &mut images, &str::to_owned).unwrap();
        let world = convert_world(&material, linear, &mut images, &str::to_owned).unwrap();
        for channel in 0..3 {
            assert!((model.base_color[channel] - 0.607_021).abs() < 0.000_01);
            assert!((model.base_color[channel] - world.base_color[channel]).abs() < 0.000_01);
        }
    }

    #[test]
    fn hero_water_and_ground_materials_export_neutral_look_extras_and_reject_non_finite() {
        let mut hero =
            Material::from_vmat("hero_test.vmat", &Value::Object(Object::default())).unwrap();
        hero.shader = "hero.vfx".into();
        hero.floats.extend([
            ("g_flRimLightScale".into(), 10.0),
            ("g_flSpecularExponent".into(), 24.0),
            ("g_flSpecularScale".into(), 5.0),
            ("g_flAmbientScale".into(), 1.25),
            ("g_flSelfIllumScale".into(), 2.0),
            ("g_flMetalnessBlendToFull".into(), 0.5),
        ]);
        hero.vectors.extend([
            ("g_vRimLightColor".into(), [0.5, 0.75, 1.0, 0.0]),
            ("g_vSpecularColor".into(), [0.8, 0.9, 1.0, 0.0]),
            ("g_vSelfIllumTint".into(), [1.0, 0.25, 0.0, 0.0]),
        ]);
        let mut images = Images::default();
        let json = convert(&hero, [1.0; 4], &mut images, &str::to_owned)
            .unwrap()
            .to_json(|i| i);
        let look = &json["extras"]["look"];
        assert_eq!(look["rim"]["scale"], json!(10.0));
        assert_eq!(look["rim"]["color"], json!([0.5, 0.75, 1.0]));
        assert_eq!(look["spec"]["exp"], json!(24.0));
        assert_eq!(look["spec"]["scale"], json!(5.0));
        assert_eq!(look["glow"]["scale"], json!(2.0));
        assert_eq!(look["blend_full"], json!([0.0, 0.0, 0.5, 0.0, 0.0, 0.0]));

        hero.floats[0].1 = f32::NAN;
        assert!(hero.figure_look().is_err());

        let mut water =
            Material::from_vmat("river.vmat", &Value::Object(Object::default())).unwrap();
        water.shader = "water_dota.vfx".into();
        water.floats.push(("g_flWaterDepth".into(), 35.0));
        water
            .vectors
            .push(("g_vRefractionTint".into(), [0.35, 0.66, 0.78, 0.0]));
        let wjson = convert(&water, [1.0; 4], &mut images, &str::to_owned)
            .unwrap()
            .to_json(|i| i);
        assert_eq!(wjson["extras"]["look"]["water"]["depth"], json!(35.0));
        water.floats[0].1 = f32::INFINITY;
        assert!(water.water_look().is_err());

        let mut ground =
            Material::from_vmat("terrain.vmat", &Value::Object(Object::default())).unwrap();
        ground.shader = "multiblend.vfx".into();
        ground.floats.push(("g_flTexCoordScale1".into(), 3.0));
        ground.floats.push(("g_flTexCoordRotate1".into(), 7.0));
        let gjson = convert(&ground, [1.0; 4], &mut images, &str::to_owned)
            .unwrap()
            .to_json(|i| i);
        assert_eq!(
            gjson["extras"]["look"]["ground_layers"][1]["scale"],
            json!(3.0)
        );
        assert_eq!(
            gjson["extras"]["look"]["ground_layers"][1]["rotation"],
            json!(7.0)
        );
        ground.floats.push(("g_flTexCoordScale0".into(), 0.0));
        assert!(ground.ground_layers().is_err());
    }
}
