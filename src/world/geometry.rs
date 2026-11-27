use std::collections::{HashMap, HashSet};

use super::environment::{boolean, copy_in, path_in, text, vector};
use super::place::{self, IDENTITY};
use super::scene::{active_entity_lumps_from_world, data_in, entity_int, vector4};
use super::surface::{self, TerrainMaterial, TerrainMeshRef, WaterMaterial, WaterSurfaceRef};
use super::{EntityLump, WorldEnvironment};
use crate::error::malformed;
use crate::kv3::Value;
use crate::material::linear_tint;
use crate::{Budget, Error, Result, Source, Vpk};

/// Renderer-neutral descriptors for a world at startup.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct WorldDescriptors {
    /// Lighting, fog-of-war and colour grading.
    pub environment: WorldEnvironment,
    /// Startup-active model placements and typed surfaces.
    pub geometry: WorldGeometry,
    /// Particle effects authored in the active entity lumps.
    pub particles: Vec<WorldParticle>,
}

/// Models placed by the world and terrain and water draws that address those placements.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct WorldGeometry {
    /// Mapped world nodes in file order.
    pub nodes: Vec<WorldNodeRef>,
    /// Model placements from active world nodes and entity lumps, including terrain and water.
    pub placements: Vec<WorldPropPlacement>,
    /// Terrain draws using the Dota multiblend shader.
    pub terrain: Vec<TerrainMeshRef>,
    /// Water draws using the Dota water shader.
    pub water: Vec<WaterSurfaceRef>,
    /// Terrain material table addressed by the terrain draws.
    pub terrain_materials: Vec<TerrainMaterial>,
    /// Water material table addressed by the water draws.
    pub water_materials: Vec<WaterMaterial>,
    /// Model or material paths that could not be resolved in the provided packages.
    pub missing_resources: Vec<String>,
}

/// A node and its associated grass data.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct WorldNodeRef {
    /// Compiled world-node path.
    pub path: String,
    /// Authored .vgrass path, when present.
    pub grass: Option<String>,
}

/// The source of one model placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlacementSource {
    /// A regular world-node scene object.
    SceneObject {
        /// Index in the geometry's node table.
        node: usize,
        /// Index in the node's unfiltered scene-object array.
        object: usize,
        /// The object's overlay flag.
        overlay: bool,
    },
    /// A world-node aggregate, possibly split into draws.
    Aggregate {
        /// Index in the geometry's node table.
        node: usize,
        /// Index in the node's unfiltered aggregate array.
        object: usize,
        /// Fragment index when the aggregate has individually addressed draws.
        fragment: Option<usize>,
    },
    /// A model-naming entity.
    Entity {
        /// Index in active entity lump order.
        lump: usize,
        /// Index within that lump.
        entity: usize,
    },
}

/// A static scene object, aggregate fragment or entity that places a model.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct WorldPropPlacement {
    /// Full model path as authored, with normalized separators.
    pub model: String,
    /// Authored source location.
    pub source: PlacementSource,
    /// Three rows of a Source-space affine transform. The last column is translation in inches.
    pub transform: [[f32; 4]; 3],
    /// Linear RGBA tint and opacity.
    pub tint: [f32; 4],
    /// Skin name or index, when set.
    pub skin: Option<String>,
    /// World-node layer name, when recorded.
    pub layer: Option<String>,
    /// Entity classname, when this placement comes from an entity.
    pub classname: Option<String>,
    /// Draw index in mesh zero for an aggregate fragment, otherwise the full model.
    pub draw_call: Option<usize>,
    /// Placement-specific material replacements.
    pub material_overrides: Vec<WorldMaterialOverride>,
}

impl WorldPropPlacement {
    /// Column-major matrix in the coordinate system of the decoded model vertices.
    pub fn gltf_matrix(&self) -> [f32; 16] {
        place::node_matrix(&self.transform)
    }
}

/// A world node's per-placement material replacement.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct WorldMaterialOverride {
    /// Mesh index for a single-draw override.
    pub mesh: usize,
    /// Draw index, or None for a whole-model override.
    pub draw_call: Option<usize>,
    /// Replacement material path.
    pub material: String,
    /// The override row's authored linear RGB tint, when present.
    pub linear_tint: Option<[f32; 3]>,
}

/// A map-authored particle spawn and its visibility flags.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct WorldParticle {
    /// Effect resource path.
    pub effect: String,
    /// World position in Source units.
    pub origin: [f64; 3],
    /// Source pitch, yaw and roll in degrees.
    pub angles: [f64; 3],
    /// Whether it starts playing at map load.
    pub start_active: bool,
    /// Visibility during day and night, respectively.
    pub day_night: [bool; 2],
    /// Visibility through fog of war.
    pub show_in_fog: bool,
    /// Initial data control-point index and value, when authored.
    pub control_point: Option<(u32, [f64; 3])>,
}

impl WorldDescriptors {
    /// Reads the world, startup-active entity lumps and node layers, and resolves terrain and
    /// water shaders. Pass the map package first for map-local resources to take precedence.
    pub fn read(src: &(impl Source + ?Sized), packages: &[&Vpk], world: &str) -> Result<Self> {
        Self::read_in(
            src,
            packages,
            world,
            &mut Budget::named("world descriptors"),
        )
    }

    /// Like read, charging scene and descriptor storage to the supplied budget. Model and
    /// material reads use bounded per-file scratch budgets and a cumulative read limit.
    pub fn read_in(
        src: &(impl Source + ?Sized),
        packages: &[&Vpk],
        path: &str,
        budget: &mut Budget,
    ) -> Result<Self> {
        let mut load = |file: &str, budget: &mut Budget| data_in(src, packages, file, budget);
        let world = load(path, budget)?.ok_or_else(|| Error::NotFound(path.to_owned()))?;
        let lumps = active_entity_lumps_from_world(&world, budget, &mut load)?;
        let environment = WorldEnvironment::from_lumps_in(&lumps, budget)?;
        let mut geometry = WorldGeometry::default();
        geometry.read_nodes(&world, &lumps, &mut load, budget)?;
        let particles = geometry.read_entities(&lumps, budget)?;
        surface::resolve(&mut geometry, src, packages, budget)?;
        Ok(Self {
            environment,
            geometry,
            particles,
        })
    }
}

impl WorldGeometry {
    fn read_nodes(
        &mut self,
        world: &Value,
        lumps: &[EntityLump],
        load: &mut impl FnMut(&str, &mut Budget) -> Result<Option<Value>>,
        budget: &mut Budget,
    ) -> Result<()> {
        let layers = layer_states(lumps, budget)?;
        let mut seen = HashSet::new();
        for row in world.array("m_worldNodes") {
            let Some(prefix) = text(row, "m_worldNodePrefix") else {
                continue;
            };
            budget.string_of(prefix.len().saturating_add(8))?;
            let path = format!("{}.vwnod_c", prefix.replace('\\', "/"));
            if !seen.insert(budget.copy(&path)?) {
                continue;
            }
            let node = load(&path, budget)?.ok_or_else(|| Error::NotFound(path.clone()))?;
            self.read_node(&node, &layers, budget)?;
            budget.slots::<WorldNodeRef>(1)?;
            self.nodes.push(WorldNodeRef {
                path,
                grass: path_in(text(&node, "m_grassFileName"), budget)?,
            });
        }
        Ok(())
    }

    fn read_node(
        &mut self,
        node: &Value,
        layers: &HashMap<&str, bool>,
        budget: &mut Budget,
    ) -> Result<()> {
        let node_index = self.nodes.len();
        let mut overrides = material_overrides(node, budget)?;
        let names = node.array("m_layerNames");
        let indices = node.array("m_sceneObjectLayerIndices");
        let layer_name =
            |index: Option<usize>| index.and_then(|i| names.get(i)).and_then(Value::as_str);
        let active =
            |name: Option<&str>| name.and_then(|name| layers.get(name)).copied() != Some(false);
        for (object, row) in node.array("m_sceneObjects").iter().enumerate() {
            let layer = layer_name(indices.get(object).and_then(Value::as_usize));
            let Some(model) = text(row, "m_renderableModel").filter(|_| active(layer)) else {
                continue;
            };
            let source = PlacementSource::SceneObject {
                node: node_index,
                object,
                overlay: text(row, "m_nObjectTypeFlags")
                    .is_some_and(|flags| flags.contains("OBJECT_TYPE_OVERLAY")),
            };
            let mut placement = new_placement(model, source, layer, budget)?;
            placement.transform = node_transform(row)?;
            let tint = vector4(row.get("m_vTintColor"));
            placement.tint = if tint == [0.0; 4] {
                [1.0; 4]
            } else {
                linear_tint(tint)
            };
            placement.skin = copy_in(text(row, "m_skin"), budget)?;
            placement.material_overrides = overrides.remove(&object).unwrap_or_default();
            self.placements.push(placement);
        }
        for (object, row) in node.array("m_aggregateSceneObjects").iter().enumerate() {
            let layer = layer_name(row.usize_field("m_nLayer"));
            if let Some(model) = text(row, "m_renderableModel").filter(|_| active(layer)) {
                self.read_aggregate(row, model, (node_index, object), layer, budget)?;
            }
        }
        Ok(())
    }

    fn read_aggregate(
        &mut self,
        row: &Value,
        model: &str,
        (node, object): (usize, usize),
        layer: Option<&str>,
        budget: &mut Budget,
    ) -> Result<()> {
        let fragments = row.array("m_aggregateMeshes");
        let source = |fragment| PlacementSource::Aggregate {
            node,
            object,
            fragment,
        };
        if fragments
            .first()
            .is_none_or(|f| f.get("m_nDrawCallIndex").is_none())
        {
            self.placements
                .push(new_placement(model, source(None), layer, budget)?);
            return Ok(());
        }
        let mut transforms = row.array("m_fragmentTransforms").iter();
        for (index, fragment) in fragments.iter().enumerate() {
            let transform = if boolean(fragment, "m_bHasTransform") == Some(true) {
                finite_transform(transforms.next()).ok_or_else(|| {
                    malformed("world", format!("{model}: fragment {index} transform"))
                })?
            } else {
                IDENTITY
            };
            if place::is_flat(&transform) {
                continue;
            }
            let mut placement = new_placement(model, source(Some(index)), layer, budget)?;
            placement.transform = transform;
            placement.draw_call =
                Some(fragment.usize_field("m_nDrawCallIndex").ok_or_else(|| {
                    malformed("world", format!("{model}: fragment {index} draw call"))
                })?);
            if let Some(tint) = fragment.get("m_vTintColor") {
                let [r, g, b, _] = vector4(Some(tint));
                placement.tint = linear_tint([r / 255.0, g / 255.0, b / 255.0, 1.0]);
            }
            self.placements.push(placement);
        }
        Ok(())
    }

    fn read_entities(
        &mut self,
        lumps: &[EntityLump],
        budget: &mut Budget,
    ) -> Result<Vec<WorldParticle>> {
        let mut particles = Vec::new();
        for (lump, rows) in lumps.iter().enumerate() {
            for (entity, row) in rows.entities.iter().enumerate() {
                if let Some(particle) = world_particle(row, budget)? {
                    budget.slots::<WorldParticle>(1)?;
                    particles.push(particle);
                }
                let classname = text(row, "classname");
                let Some(model) =
                    text(row, "model").filter(|p| p.ends_with(".vmdl") || p.ends_with(".vmdl_c"))
                else {
                    continue;
                };
                if classname.is_some_and(|class| {
                    class.starts_with("trigger_")
                        || class.starts_with("filter_")
                        || class == "csgo_player_previewmodel"
                }) {
                    continue;
                }
                let mut placement = new_placement(
                    model,
                    PlacementSource::Entity { lump, entity },
                    None,
                    budget,
                )?;
                placement.classname = copy_in(classname, budget)?;
                placement.transform = place::entity_affine(row);
                if !placement
                    .transform
                    .iter()
                    .flatten()
                    .all(|axis| axis.is_finite())
                {
                    return Err(malformed(
                        "world",
                        format!("{model}: non-finite entity transform"),
                    ));
                }
                placement.tint = place::entity_tint(row);
                placement.skin = copy_in(
                    text(row, "skin").filter(|skin| !matches!(*skin, "0" | "default")),
                    budget,
                )?;
                self.placements.push(placement);
            }
        }
        Ok(particles)
    }
}

fn material_overrides(
    node: &Value,
    budget: &mut Budget,
) -> Result<HashMap<usize, Vec<WorldMaterialOverride>>> {
    let mut overrides = HashMap::<_, Vec<_>>::new();
    for row in node.array("m_materialOverrides") {
        let (object, mesh, draw_call, material) = super::scene::material_override_row(row)?;
        budget.slots::<WorldMaterialOverride>(1)?;
        overrides
            .entry(object)
            .or_default()
            .push(WorldMaterialOverride {
                mesh,
                draw_call,
                material: normalized_path(material, budget)?,
                linear_tint: row.get("m_vLinearTintColor").and_then(Value::f32_array),
            });
    }
    Ok(overrides)
}

fn layer_states<'a>(
    lumps: &'a [EntityLump],
    budget: &mut Budget,
) -> Result<HashMap<&'a str, bool>> {
    let mut layers = HashMap::new();
    for entity in lumps.iter().flat_map(|lump| &lump.entities) {
        if entity.str_field("classname") == Some("info_world_layer")
            && let Some(name) = text(entity, "layerName")
        {
            budget.slots::<(&str, bool)>(1)?;
            let enabled = entity_int(entity, "spawnflags").unwrap_or_default() & 1 != 0;
            layers
                .entry(name)
                .and_modify(|on| *on |= enabled)
                .or_insert(enabled);
        }
    }
    Ok(layers)
}

fn new_placement(
    model: &str,
    source: PlacementSource,
    layer: Option<&str>,
    budget: &mut Budget,
) -> Result<WorldPropPlacement> {
    budget.slots::<WorldPropPlacement>(1)?;
    Ok(WorldPropPlacement {
        model: normalized_path(model, budget)?,
        source,
        transform: IDENTITY,
        tint: [1.0; 4],
        skin: None,
        layer: copy_in(layer, budget)?,
        classname: None,
        draw_call: None,
        material_overrides: Vec::new(),
    })
}

fn normalized_path(path: &str, budget: &mut Budget) -> Result<String> {
    budget.string_of(path.len())?;
    Ok(path.replace('\\', "/"))
}

fn finite_transform(value: Option<&Value>) -> Option<[[f32; 4]; 3]> {
    let transform = place::affine(value)?;
    transform
        .iter()
        .flatten()
        .all(|axis| axis.is_finite())
        .then_some(transform)
}

fn node_transform(row: &Value) -> Result<[[f32; 4]; 3]> {
    let transform = row.get("m_vTransform");
    if transform.is_none() {
        return Ok(IDENTITY);
    }
    finite_transform(transform).ok_or_else(|| malformed("world", "invalid scene object transform"))
}

fn world_particle(row: &Value, budget: &mut Budget) -> Result<Option<WorldParticle>> {
    if !matches!(
        row.str_field("classname"),
        Some("dota_world_particle_system" | "info_particle_system")
    ) {
        return Ok(None);
    }
    let Some(effect) = path_in(text(row, "effect_name"), budget)? else {
        return Ok(None);
    };
    let control_point = entity_int(row, "data_cp")
        .and_then(|index| Some((u32::try_from(index).ok()?, vector(row, "data_cp_value")?)));
    Ok(Some(WorldParticle {
        effect,
        origin: vector(row, "origin").unwrap_or_default(),
        angles: vector(row, "angles").unwrap_or_default(),
        start_active: boolean(row, "start_active").unwrap_or(true),
        day_night: ["showatday", "showatnight"].map(|key| boolean(row, key).unwrap_or(true)),
        show_in_fog: boolean(row, "show_in_fog").unwrap_or(true),
        control_point,
    }))
}
