use std::collections::HashMap;

use super::environment::path_in;
use super::geometry::{WorldGeometry, WorldPropPlacement};
use super::scene::{MAX_SCENE_BYTES, MAX_WORLD_READ_BYTES, compiled, data_in, skin_material};
use super::{GroundLayerLook, WaterLook};
use crate::error::malformed;
use crate::export::read_from_in;
use crate::material::Material;
use crate::{Budget, Error, Model, Resource, Result, Source, Vpk};

/// A draw call's address in a placed Source 2 model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorldDrawRef {
    /// Index in the world's placement list.
    pub placement: usize,
    /// Index in the model's mesh list.
    pub mesh: usize,
    /// Index in that mesh's draw calls.
    pub draw_call: usize,
}

/// A terrain draw with an authored multiblend material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TerrainMeshRef {
    /// Where to find the geometry and its placement.
    pub draw: WorldDrawRef,
    /// Index in the world's terrain material table.
    pub material: usize,
}

/// A water draw with an authored Dota water material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct WaterSurfaceRef {
    /// Where to find the geometry and its placement.
    pub draw: WorldDrawRef,
    /// Index in the world's water material table.
    pub material: usize,
}

/// One material with four splat layers. Weights are in the model's vertex color attributes.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TerrainMaterial {
    /// Full material path.
    pub path: String,
    /// Layers in shader slot and vertex colour channel order.
    pub layers: [TerrainLayer; 4],
    /// Primary tint masks, one layer per RGBA channel.
    pub tint_masks: Option<String>,
    /// Secondary tint masks, one layer per RGBA channel.
    pub secondary_tint_masks: Option<String>,
}

/// A terrain layer's shading and texture references.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TerrainLayer {
    /// Authored tint, UV scale and rotation.
    pub look: GroundLayerLook,
    /// Layer colour texture. Alpha stores the reveal mask on layers 1 through 3.
    pub color: Option<String>,
    /// Layer's packed specular texture.
    pub specular: Option<String>,
}

/// Water shader parameters and texture bindings.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct WaterMaterial {
    /// Full material path.
    pub path: String,
    /// Authored water shading parameters.
    pub look: WaterLook,
    /// Surface normals.
    pub normal: Option<String>,
    /// Flow lookup. The world may also name a world-space flow texture.
    pub flow: Option<String>,
    /// Noise texture.
    pub noise: Option<String>,
}

#[derive(Clone, Copy)]
enum Surface {
    Other,
    Terrain(usize),
    Water(usize),
}

struct ModelDraws {
    meshes: Vec<Vec<String>>,
    skins: Vec<(String, Vec<String>)>,
}

impl ModelDraws {
    fn from_model(model: Model, budget: &mut Budget) -> Result<Self> {
        budget.slots::<Vec<String>>(model.meshes.len())?;
        for mesh in &model.meshes {
            budget.slots::<String>(mesh.primitives.len())?;
            for draw in &mesh.primitives {
                budget.string_of(draw.material.len())?;
            }
        }
        budget.slots::<(String, Vec<String>)>(model.material_groups.len())?;
        for (name, materials) in &model.material_groups {
            budget.string_of(name.len())?;
            budget.slots::<String>(materials.len())?;
            for path in materials {
                budget.string_of(path.len())?;
            }
        }
        Ok(Self {
            meshes: model
                .meshes
                .into_iter()
                .map(|mesh| {
                    mesh.primitives
                        .into_iter()
                        .map(|draw| draw.material)
                        .collect()
                })
                .collect(),
            skins: model.material_groups,
        })
    }

    fn visit(
        &self,
        placement: &WorldPropPlacement,
        index: usize,
        mut visit: impl FnMut(WorldDrawRef, &str) -> Result<()>,
    ) -> Result<()> {
        let skin = placement
            .skin
            .as_deref()
            .and_then(|name| {
                self.skins
                    .iter()
                    .position(|(skin, _)| skin == name)
                    .or_else(|| name.parse::<usize>().ok())
            })
            .filter(|&skin| skin != 0);
        let mut at = |mesh, draw_call, original| {
            let material = placement
                .material_overrides
                .iter()
                .find(|row| row.draw_call.is_none())
                .or_else(|| {
                    placement
                        .material_overrides
                        .iter()
                        .rev()
                        .find(|row| row.mesh == mesh && row.draw_call == Some(draw_call))
                })
                .map_or_else(
                    || skin_material(&self.skins, original, skin),
                    |row| row.material.as_str(),
                );
            visit(
                WorldDrawRef {
                    placement: index,
                    mesh,
                    draw_call,
                },
                material,
            )
        };
        if let Some(draw) = placement.draw_call {
            let material = self
                .meshes
                .first()
                .and_then(|mesh| mesh.get(draw))
                .ok_or_else(|| {
                    malformed(
                        "world",
                        format!(
                            "{}: aggregate draw call {draw} out of range",
                            placement.model
                        ),
                    )
                })?;
            return at(0, draw, material.as_str());
        }
        for (mesh, materials) in self.meshes.iter().enumerate() {
            for (draw, material) in materials.iter().enumerate() {
                at(mesh, draw, material.as_str())?;
            }
        }
        Ok(())
    }
}

struct Reader<'a, S: ?Sized> {
    src: &'a S,
    packages: &'a [&'a Vpk],
    budget: &'a mut Budget,
    bytes_read: u64,
}

pub(super) fn resolve(
    geometry: &mut WorldGeometry,
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    budget: &mut Budget,
) -> Result<()> {
    let mut reader = Reader {
        src,
        packages,
        budget,
        bytes_read: 0,
    };
    let mut models = HashMap::new();
    let mut surfaces = HashMap::new();
    // Keep placement indices stable while the material and draw tables are filled.
    let placements = std::mem::take(&mut geometry.placements);
    for (index, placement) in placements.iter().enumerate() {
        if !models.contains_key(placement.model.as_str()) {
            reader.budget.slots::<(&str, Option<ModelDraws>)>(1)?;
            let model = reader.model(&placement.model, geometry)?;
            models.insert(placement.model.as_str(), model);
        }
        let Some(model) = &models[placement.model.as_str()] else {
            continue;
        };
        model.visit(placement, index, |draw, path| {
            reader.budget.slots::<WorldDrawRef>(1)?;
            if path.is_empty() {
                return Ok(());
            }
            let kind = if let Some(&kind) = surfaces.get(path) {
                kind
            } else {
                let kind = reader.surface(path, geometry)?;
                let key = reader.budget.copy(path)?;
                reader.budget.slots::<(String, Surface)>(1)?;
                surfaces.insert(key, kind);
                kind
            };
            match kind {
                Surface::Terrain(material) => {
                    reader.budget.slots::<TerrainMeshRef>(1)?;
                    geometry.terrain.push(TerrainMeshRef { draw, material });
                }
                Surface::Water(material) => {
                    reader.budget.slots::<WaterSurfaceRef>(1)?;
                    geometry.water.push(WaterSurfaceRef { draw, material });
                }
                Surface::Other => {}
            }
            Ok(())
        })?;
    }
    geometry.placements = placements;
    Ok(())
}

impl<S: Source + ?Sized> Reader<'_, S> {
    fn model(&mut self, path: &str, geometry: &mut WorldGeometry) -> Result<Option<ModelDraws>> {
        let path = compiled(path);
        let mut scratch = Budget::with_limit("world model", MAX_SCENE_BYTES);
        let parsed = read_from_in(self.src, self.packages, &path, &mut scratch)
            .and_then(|bytes| Model::read_in(&Resource::parse(&bytes)?, &mut scratch));
        self.charge(scratch.spent())?;
        match parsed {
            Ok(model) => Ok(Some(ModelDraws::from_model(model, self.budget)?)),
            Err(Error::NotFound(_)) => {
                self.record_missing(path, geometry)?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    fn surface(&mut self, path: &str, geometry: &mut WorldGeometry) -> Result<Surface> {
        let file = compiled(path);
        let mut scratch = Budget::with_limit("world material", MAX_SCENE_BYTES);
        let data = data_in(self.src, self.packages, &file, &mut scratch)?;
        self.charge(scratch.spent())?;
        let Some(data) = data else {
            self.record_missing(file, geometry)?;
            return Ok(Surface::Other);
        };
        let shader = data.str_field("m_shaderName").unwrap_or_default();
        let shader = shader.rsplit_once('.').map_or(shader, |(stem, _)| stem);
        if !matches!(shader, "multiblend" | "water_dota") {
            return Ok(Surface::Other);
        }
        let material = Material::from_vmat(path, &data)?;
        if let Some(look) = material.water_look()? {
            let index = geometry.water_materials.len();
            self.budget.slots::<WaterMaterial>(1)?;
            geometry
                .water_materials
                .push(water_material(&material, look, self.budget)?);
            return Ok(Surface::Water(index));
        }
        if let Some(looks) = material.ground_layers()? {
            let index = geometry.terrain_materials.len();
            self.budget.slots::<TerrainMaterial>(1)?;
            geometry
                .terrain_materials
                .push(terrain_material(&material, looks, self.budget)?);
            return Ok(Surface::Terrain(index));
        }
        Ok(Surface::Other)
    }

    fn record_missing(&mut self, path: String, geometry: &mut WorldGeometry) -> Result<()> {
        self.budget.slots::<String>(1)?;
        self.budget.string_of(path.len())?;
        geometry.missing_resources.push(path);
        Ok(())
    }

    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.bytes_read = self.bytes_read.saturating_add(bytes as u64);
        if self.bytes_read > MAX_WORLD_READ_BYTES {
            return Err(malformed(
                "world",
                format!("reading more than {MAX_WORLD_READ_BYTES} bytes"),
            ));
        }
        Ok(())
    }
}

fn texture(material: &Material, key: &str, budget: &mut Budget) -> Result<Option<String>> {
    let path = material
        .textures
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, path)| path.as_str())
        .filter(|path| !path.is_empty());
    path_in(path, budget)
}

fn water_material(
    material: &Material,
    look: WaterLook,
    budget: &mut Budget,
) -> Result<WaterMaterial> {
    Ok(WaterMaterial {
        path: budget.copy(&material.path)?,
        look,
        normal: texture(material, "g_tNormal", budget)?,
        flow: texture(material, "g_tFlow", budget)?,
        noise: texture(material, "g_tNoise", budget)?,
    })
}

fn terrain_material(
    material: &Material,
    looks: [GroundLayerLook; 4],
    budget: &mut Budget,
) -> Result<TerrainMaterial> {
    let mut layers = looks.map(|look| TerrainLayer {
        look,
        color: None,
        specular: None,
    });
    for (index, layer) in layers.iter_mut().enumerate() {
        layer.color = texture(material, &format!("g_tColor{index}"), budget)?;
        layer.specular = texture(material, &format!("g_tSpecular{index}"), budget)?;
    }
    Ok(TerrainMaterial {
        path: budget.copy(&material.path)?,
        layers,
        tint_masks: texture(material, "g_tTintMasks", budget)?,
        secondary_tint_masks: texture(material, "g_tTint2Masks", budget)?,
    })
}
