//! The whole world as Source2Viewer 20 exported it (`ExportToFile(VWorld)`): every node of the
//! world nodes' scene objects, their aggregates' fragments and the entities' models, in its
//! order; meshes with identical model, skin and tint share geometry when neither placement
//! overrides a draw. Materials are made once per path and tint; images once per texture. Vertex data is not kept: the meshes record
//! where it comes from and its bounds.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::rc::Rc;

use super::entities::EntityLump;
use super::export::SkippedModel;
use super::place::{self, Affine, IDENTITY};
use crate::budget::Budget;
use crate::error::{Error, Result, malformed};
use crate::export::read_from_in;
use crate::kv3::Value;
use crate::material::{GltfMaterial, Images, Material, convert_world, linear_tint};
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
use crate::vertices::Vertices;
use crate::vpk::Vpk;

/// The lift along an overlay's normals keeps it from fighting the terrain for depth.
/// Positions in the mesh are already in metres.
pub(super) const OVERLAY_LIFT: f32 = 0.01;
/// Most nodes, meshes and materials a world may have.
const MAX_ITEMS: usize = 1 << 20;
/// Most entity lumps loaded for one world by [`active_entity_lumps`].
pub const MAX_ENTITY_LUMPS: usize = 64;
/// Most unique entity lump paths inspected by [`active_entity_lumps`].
const MAX_VISITED_LUMPS: usize = MAX_ENTITY_LUMPS * 4;
/// Longest entity lump path accepted by [`active_entity_lumps`].
const MAX_LUMP_PATH: usize = 256;
/// Most bytes the scene's retained state (or any single file read) may take.
pub(super) const MAX_SCENE_BYTES: usize = 256 << 20;
/// Most cumulative bytes read and decoded across an entire world export.
pub(super) const MAX_WORLD_READ_BYTES: u64 = 8 << 30;

/// Reads the active entity lumps of the world at `path` (`maps/.../world.vwrld_c`) from
/// `packages`: every lump listed in `m_entityLumps`, plus any child lump named by an
/// `info_world_layer` entity whose `spawnflags` bit 0 ("start enabled") is set.
///
/// Each lump is read at most once (cycles are ignored), and at most [`MAX_ENTITY_LUMPS`] lumps
/// are loaded within a bounded memory budget.
pub fn active_entity_lumps(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
) -> Result<Vec<EntityLump>> {
    let mut budget = Budget::with_limit("world", MAX_SCENE_BYTES);
    active_entity_lumps_in(src, packages, path, &mut budget)
}

/// [`active_entity_lumps`] within a caller-supplied [`Budget`].
pub fn active_entity_lumps_in(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    path: &str,
    budget: &mut Budget,
) -> Result<Vec<EntityLump>> {
    active_entity_lumps_with(path, budget, &mut |file, budget| {
        data_in(src, packages, file, budget)
    })
}

pub(super) fn data_in(
    src: &(impl Source + ?Sized),
    packages: &[&Vpk],
    file: &str,
    budget: &mut Budget,
) -> Result<Option<Value>> {
    let bytes = match read_from_in(src, packages, file, budget) {
        Ok(bytes) => bytes,
        Err(Error::NotFound(_)) => return Ok(None),
        Err(e) => return Err(e),
    };
    let resource = Resource::parse(&bytes)?;
    let data = resource
        .data_in(budget)
        .ok_or_else(|| malformed("world", format!("{file} has no DATA")))??;
    Ok(Some(data))
}

fn active_entity_lumps_with(
    path: &str,
    budget: &mut Budget,
    load: &mut impl FnMut(&str, &mut Budget) -> Result<Option<Value>>,
) -> Result<Vec<EntityLump>> {
    let world = load(path, budget)?.ok_or_else(|| Error::NotFound(path.to_owned()))?;
    active_entity_lumps_from_world(&world, budget, load)
}

pub(super) fn active_entity_lumps_from_world(
    world: &Value,
    budget: &mut Budget,
    load: &mut impl FnMut(&str, &mut Budget) -> Result<Option<Value>>,
) -> Result<Vec<EntityLump>> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    for lump in world.array("m_entityLumps") {
        let Some(lump_path) = lump.as_str().filter(|p| !p.is_empty()) else {
            continue;
        };
        enqueue_lump(lump_path, &mut visited, &mut queue, budget)?;
    }
    let mut out = Vec::new();
    while let Some(file) = queue.pop_front() {
        let Some(data) = load(&file, budget)? else {
            continue;
        };
        if out.len() >= MAX_ENTITY_LUMPS {
            return Err(malformed(
                "world",
                format!("more than {MAX_ENTITY_LUMPS} entity lumps"),
            ));
        }
        let lump = EntityLump::from_data(&data)?;
        for child in enabled_child_lumps(&lump)? {
            enqueue_lump(child, &mut visited, &mut queue, budget)?;
        }
        out.push(lump);
    }
    Ok(out)
}

fn enqueue_lump(
    lump_path: &str,
    visited: &mut HashSet<String>,
    queue: &mut VecDeque<String>,
    budget: &mut Budget,
) -> Result<()> {
    check_lump_path(lump_path)?;
    let file = compiled(lump_path);
    if visited.contains(&file) {
        return Ok(());
    }
    if visited.len() >= MAX_VISITED_LUMPS {
        return Err(malformed(
            "world",
            format!("more than {MAX_VISITED_LUMPS} entity lump paths"),
        ));
    }
    budget.string_of(file.len())?;
    budget.string_of(file.len())?;
    visited.insert(file.clone());
    queue.push_back(file);
    Ok(())
}

fn check_lump_path(path: &str) -> Result<()> {
    let invalid = path.is_empty()
        || path.len() > MAX_LUMP_PATH
        || path.starts_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    if invalid {
        return Err(malformed(
            "world",
            format!("invalid entity lump path: {path}"),
        ));
    }
    Ok(())
}

pub(super) fn entity_int(entity: &Value, key: &str) -> Option<i64> {
    match entity.get(key)? {
        Value::String(text) => text.trim().parse().ok(),
        other => other.as_i64(),
    }
}

fn enabled_child_lumps(lump: &EntityLump) -> Result<Vec<&str>> {
    let mut enabled: HashSet<&str> = HashSet::new();
    for layer in lump
        .entities
        .iter()
        .filter(|e| e.str_field("classname") == Some("info_world_layer"))
    {
        let name = layer
            .str_field("layerName")
            .filter(|n| !n.is_empty())
            .ok_or_else(|| malformed("world", "info_world_layer has no layerName"))?;
        let flags = match layer.get("spawnflags") {
            None => 0,
            Some(_) => entity_int(layer, "spawnflags").ok_or_else(|| {
                malformed("world", format!("layer {name}: spawnflags not a number"))
            })?,
        };
        if flags & 1 != 0 {
            enabled.insert(name);
        }
    }
    if enabled.is_empty() {
        return Ok(Vec::new());
    }
    let mut active = Vec::new();
    for child in &lump.children {
        let Some(layer) = child.rsplit('/').next().and_then(|file| {
            file.strip_suffix(".vents_c")
                .or_else(|| file.strip_suffix(".vents"))
        }) else {
            continue;
        };
        if enabled.contains(layer) {
            active.push(child.as_str());
        }
    }
    Ok(active)
}

/// Smallest and largest corner.
pub(super) type Bounds = [[f32; 3]; 2];

pub(super) struct Scene {
    /// Model paths (`.vmdl`, without `_c`).
    pub models: Vec<String>,
    pub meshes: Vec<SceneMesh>,
    pub nodes: Vec<SceneNode>,
    pub materials: Vec<SceneMaterial>,
    pub images: Images,
    pub skipped_models: Vec<SkippedModel>,
}

pub(super) struct SceneNode {
    pub name: String,
    pub mesh: usize,
    /// glTF node matrix, column major.
    pub matrix: [f32; 16],
}

pub(super) struct SceneMesh {
    pub name: String,
    pub model: usize,
    /// Index into the model's meshes.
    pub mesh: usize,
    /// The vertex accessors its whole-buffer primitives share: one set per mesh made, one per
    /// aggregate for all its fragments.
    pub set: usize,
    pub primitives: Vec<ScenePrimitive>,
    /// Over its primitives' whole vertex buffers (as the exported accessors' bounds).
    pub bounds: Option<Bounds>,
}

pub(super) struct ScenePrimitive {
    /// Index into the model mesh's draw calls.
    pub draw_call: usize,
    pub material: Option<usize>,
    /// Lifted off what it lies on by [`OVERLAY_LIFT`].
    pub overlay: bool,
}

pub(super) struct SceneMaterial {
    pub gltf: GltfMaterial,
    pub shader: String,
    /// The textures it names (`.vtex`, without `_c`).
    pub textures: Vec<String>,
}

/// A mesh to make: draw calls of a model's mesh, tinted, maybe with a skin's material.
struct MeshSpec<'a> {
    model: usize,
    mesh: usize,
    name: String,
    set: usize,
    draw_calls: Range<usize>,
    tint: [f32; 4],
    skin: Option<usize>,
    overrides: Option<&'a PlacementOverrides<'a>>,
}

#[derive(Default)]
struct PlacementOverrides<'a> {
    whole: Option<&'a str>,
    meshes: HashMap<usize, HashMap<usize, &'a str>>,
}

impl<'a> PlacementOverrides<'a> {
    fn affects(&self, mesh: usize) -> bool {
        self.whole.is_some() || self.meshes.contains_key(&mesh)
    }

    fn material(&self, mesh: usize, draw: usize) -> Option<&'a str> {
        self.whole
            .or_else(|| self.meshes.get(&mesh)?.get(&draw).copied())
    }
}

/// A model's meshes, kept without vertex data.
struct ModelInfo {
    meshes: Vec<MeshInfo>,
    groups: Vec<(String, Vec<String>)>,
}

impl ModelInfo {
    /// A skin replaces each default material with the material in the same slot.
    fn material(&self, original: &str, skin: Option<usize>) -> String {
        skin_material(&self.groups, original, skin).to_owned()
    }
}

pub(super) fn skin_material<'a>(
    groups: &'a [(String, Vec<String>)],
    original: &'a str,
    skin: Option<usize>,
) -> &'a str {
    let replacement = skin.and_then(|skin| {
        let slot = groups.first()?.1.iter().position(|path| path == original)?;
        groups.get(skin)?.1.get(slot)
    });
    replacement.map_or(original, String::as_str)
}

struct MeshInfo {
    name: String,
    draw_calls: Vec<DrawCall>,
    /// Per vertex buffer, its bounds as it is and lifted as an overlay.
    buffers: Vec<[Option<Bounds>; 2]>,
}

struct DrawCall {
    vertices: usize,
    material: String,
    tint: Option<[f32; 4]>,
}

#[derive(Clone)]
enum WorldMaterial {
    Visible(Rc<Material>),
    Tool,
    Missing,
}

/// A material path and a tint's bits.
type MaterialKey = (String, [u32; 4]);

/// Reads the world and what it places.
pub(super) struct Builder<'a, S: Source + ?Sized> {
    src: &'a S,
    packages: &'a [&'a Vpk],
    texture_uri: &'a dyn Fn(&str) -> String,
    scene: Scene,
    infos: Vec<ModelInfo>,
    model_index: HashMap<String, Option<usize>>,
    mesh_index: HashMap<(String, [u32; 4]), Option<usize>>,
    material_index: HashMap<MaterialKey, usize>,
    /// Materials read so far by path, each read once whatever tints it is drawn with.
    material_files: HashMap<String, WorldMaterial>,
    sets: usize,
    /// Retained scene state (names, meshes, materials, infos, skipped models).
    budget: Budget,
    /// Cumulative bytes read and decoded across transient files and models.
    total_read: u64,
    max_read: u64,
}

impl<'a, S: Source + ?Sized> Builder<'a, S> {
    pub(super) fn new(
        src: &'a S,
        packages: &'a [&'a Vpk],
        texture_uri: &'a dyn Fn(&str) -> String,
    ) -> Self {
        Self {
            src,
            packages,
            texture_uri,
            scene: Scene {
                models: Vec::new(),
                meshes: Vec::new(),
                nodes: Vec::new(),
                materials: Vec::new(),
                images: Images::default(),
                skipped_models: Vec::new(),
            },
            infos: Vec::new(),
            model_index: HashMap::new(),
            mesh_index: HashMap::new(),
            material_index: HashMap::new(),
            material_files: HashMap::new(),
            sets: 0,
            budget: Budget::with_limit("world", MAX_SCENE_BYTES),
            total_read: 0,
            max_read: MAX_WORLD_READ_BYTES,
        }
    }

    fn charge_read(&mut self, bytes: usize) -> Result<()> {
        self.total_read = self.total_read.saturating_add(bytes as u64);
        if self.total_read > self.max_read {
            return Err(malformed(
                "world",
                format!("reading more than {} bytes", self.max_read),
            ));
        }
        Ok(())
    }

    /// The scene of the world at `path` (`maps/.../world.vwrld_c`) and cumulative read bytes.
    pub(super) fn build(mut self, path: &str) -> Result<(Scene, u64)> {
        let world = self
            .data(path)?
            .ok_or_else(|| Error::NotFound(path.to_owned()))?;
        // A node or lump listed again would place the same things again: each is read once.
        let mut read = HashSet::new();
        for node in world.array("m_worldNodes") {
            let Some(prefix) = node.str_field("m_worldNodePrefix") else {
                continue;
            };
            let file = format!("{prefix}.vwnod_c");
            if read.insert(file.clone())
                && let Some(node) = self.data(&file)?
            {
                self.world_node(&node)?;
            }
        }
        for lump in world.array("m_entityLumps") {
            let Some(lump) = lump.as_str() else {
                continue;
            };
            let file = compiled(lump);
            if read.insert(file.clone())
                && let Some(lump) = self.data(&file)?
            {
                self.entities(&lump)?;
            }
        }
        Ok((self.scene, self.total_read))
    }

    /// The decoded `DATA` of file `path`; `None` when no package has it.
    fn data(&mut self, path: &str) -> Result<Option<Value>> {
        let mut file_budget = Budget::with_limit("world", MAX_SCENE_BYTES);
        let bytes = match read_from_in(self.src, self.packages, path, &mut file_budget) {
            Ok(bytes) => bytes,
            Err(Error::NotFound(_)) => return Ok(None),
            Err(e) => {
                self.charge_read(file_budget.spent())?;
                return Err(e);
            }
        };
        let decoded = Resource::parse(&bytes).and_then(|resource| {
            resource
                .data_in(&mut file_budget)
                .ok_or_else(|| malformed("world", format!("{path} has no DATA")))?
        });
        self.charge_read(file_budget.spent())?;
        Ok(Some(decoded?))
    }

    fn world_node(&mut self, node: &Value) -> Result<()> {
        self.budget
            .slots::<(usize, usize, &str)>(node.array("m_materialOverrides").len())?;
        let overrides = world_material_overrides(node)?;
        for (object_index, object) in node.array("m_sceneObjects").iter().enumerate() {
            let Some(model) = object.str_field("m_renderableModel") else {
                continue;
            };
            let Some(model) = self.model(model)? else {
                continue;
            };
            let affine = place::affine(object.get("m_vTransform")).unwrap_or(IDENTITY);
            let mut tint = vector4(object.get("m_vTintColor"));
            if tint == [0.0; 4] {
                tint = [1.0; 4];
            }
            self.place_model(
                model,
                &affine,
                linear_tint(tint),
                object.str_field("m_skin"),
                overrides.get(&object_index),
            )?;
        }
        for object in node.array("m_aggregateSceneObjects") {
            let Some(model) = object.str_field("m_renderableModel") else {
                continue;
            };
            let Some(model) = self.model(model)? else {
                continue;
            };
            let fragments = object.array("m_aggregateMeshes");
            if fragments
                .first()
                .is_some_and(|f| f.get("m_nDrawCallIndex").is_none())
            {
                self.place_model(model, &IDENTITY, [1.0; 4], None, None)?;
            } else {
                self.fragments(model, object, fragments)?;
            }
        }
        Ok(())
    }

    /// An aggregate's fragments: each one draw call of the model's one mesh, a mesh of its own.
    fn fragments(&mut self, model: usize, object: &Value, fragments: &[Value]) -> Result<()> {
        if self.infos[model].meshes.len() != 1 {
            return Err(malformed(
                "world",
                format!(
                    "{}: an aggregate of other than one mesh",
                    self.scene.models[model]
                ),
            ));
        }
        let name = stem(&self.scene.models[model]).to_owned();
        let transforms = object.array("m_fragmentTransforms");
        let (mut next_transform, set) = (0, self.new_set());
        for (id, fragment) in fragments.iter().enumerate() {
            let mut affine = IDENTITY;
            if fragment.get("m_bHasTransform").and_then(Value::as_bool) == Some(true) {
                affine = place::affine(transforms.get(next_transform)).ok_or_else(|| {
                    malformed(
                        "world",
                        format!("{name}: fragment transform {next_transform}"),
                    )
                })?;
                next_transform += 1;
                if place::is_flat(&affine) {
                    continue;
                }
            }
            let tint = match fragment.get("m_vTintColor") {
                Some(t) => {
                    let [r, g, b, _] = vector4(Some(t));
                    linear_tint([r / 255.0, g / 255.0, b / 255.0, 1.0])
                }
                None => [1.0; 4],
            };
            let draw_call = fragment
                .usize_field("m_nDrawCallIndex")
                .filter(|&i| i < self.infos[model].meshes[0].draw_calls.len())
                .ok_or_else(|| malformed("world", format!("{name}: fragment draw call")))?;
            let mesh = self.make_mesh(MeshSpec {
                model,
                mesh: 0,
                name: format!("{name}_fragment{}", id + 1),
                set,
                draw_calls: draw_call..draw_call + 1,
                tint,
                skin: None,
                overrides: None,
            })?;
            if let Some(mesh) = mesh {
                self.push_node(name.clone(), mesh, &affine)?;
            }
        }
        Ok(())
    }

    /// The entities' models (the lump's own entities: the world layers it includes are
    /// switched on by the game, and the export left them out).
    fn entities(&mut self, lump: &Value) -> Result<()> {
        for values in EntityLump::from_data(lump)?.entities {
            let text = |key| values.get(key).and_then(Value::as_str);
            let Some(model) = text("model").filter(|m| !m.is_empty()) else {
                continue;
            };
            if text("classname") == Some("csgo_player_previewmodel") {
                continue;
            }
            let Some(model) = self.model(model)? else {
                continue;
            };
            let skin = text("skin").filter(|s| !matches!(*s, "0" | "default"));
            let affine = place::entity_affine(&values);
            self.place_model(model, &affine, place::entity_tint(&values), skin, None)?;
        }
        Ok(())
    }

    /// The model at `path` (without `_c`), read once; `None` when no package has it.
    fn model(&mut self, path: &str) -> Result<Option<usize>> {
        if let Some(&known) = self.model_index.get(path) {
            return Ok(known);
        }
        self.budget.bytes(path.len())?;
        let mut model_budget = Budget::with_limit("world", MAX_SCENE_BYTES);
        let decoded = read_from_in(self.src, self.packages, &compiled(path), &mut model_budget)
            .and_then(|bytes| Model::read_in(&Resource::parse(&bytes)?, &mut model_budget))
            .and_then(|model| info(&model));
        self.charge_read(model_budget.spent())?;
        let index = match decoded {
            Ok(info) => {
                self.infos.push(info);
                self.scene.models.push(path.to_owned());
                Some(self.infos.len() - 1)
            }
            Err(Error::NotFound(_)) => None,
            Err(error) => {
                let reason = error.to_string();
                self.budget.slots::<SkippedModel>(1)?;
                self.budget.bytes(path.len() + reason.len())?;
                self.scene.skipped_models.push(SkippedModel {
                    path: path.to_owned(),
                    reason,
                });
                None
            }
        };
        self.model_index.insert(path.to_owned(), index);
        Ok(index)
    }

    /// A node per mesh of `model` (`<stem>.<mesh>`, with `.<skin>` when skinned), placed by
    /// `affine`; placements with the same model, skin and tint share a mesh.
    fn place_model(
        &mut self,
        model: usize,
        affine: &Affine,
        tint: [f32; 4],
        skin: Option<&str>,
        overrides: Option<&PlacementOverrides<'_>>,
    ) -> Result<()> {
        let groups = &self.infos[model].groups;
        let skin = skin
            .and_then(|skin| groups.iter().position(|(name, _)| name == skin))
            .filter(|&index| index != 0);
        let stem = stem(&self.scene.models[model]).to_owned();
        self.budget.bytes(self.infos[model].meshes.len())?;
        for m in 0..self.infos[model].meshes.len() {
            let info = &self.infos[model].meshes[m];
            if info.draw_calls.is_empty() {
                continue;
            }
            let mut name = format!("{stem}.{}", info.name);
            if let Some(skin) = skin {
                name = format!("{name}.{}", self.infos[model].groups[skin].0);
            }
            let key = (name.clone(), tint.map(f32::to_bits));
            let shared = overrides.is_none_or(|overrides| !overrides.affects(m));
            let mesh = match shared.then(|| self.mesh_index.get(&key)).flatten() {
                Some(&mesh) => mesh,
                None => {
                    let draw_calls = 0..info.draw_calls.len();
                    let set = self.new_set();
                    let mesh = self.make_mesh(MeshSpec {
                        model,
                        mesh: m,
                        name: name.clone(),
                        set,
                        draw_calls,
                        tint,
                        skin,
                        overrides,
                    })?;
                    if shared {
                        self.mesh_index.insert(key, mesh);
                    }
                    mesh
                }
            };
            if let Some(mesh) = mesh {
                self.push_node(name, mesh, affine)?;
            }
        }
        Ok(())
    }

    fn make_mesh(&mut self, spec: MeshSpec<'_>) -> Result<Option<usize>> {
        let MeshSpec {
            model,
            mesh,
            name,
            set,
            draw_calls,
            tint,
            skin,
            overrides,
        } = spec;
        // Charge even filtered draws. Unique placements can revisit the same source mesh.
        self.budget.slots::<ScenePrimitive>(draw_calls.len())?;
        let mut primitives = Vec::with_capacity(draw_calls.len());
        let mut bounds: Option<Bounds> = None;
        for d in draw_calls {
            let call = &self.infos[model].meshes[mesh].draw_calls[d];
            let path = overrides
                .and_then(|overrides| overrides.material(mesh, d))
                .map_or_else(
                    || self.infos[model].material(&call.material, skin),
                    str::to_owned,
                );
            let tint = match call.tint {
                Some(stored) => {
                    let draw = linear_tint(stored);
                    std::array::from_fn(|i| tint[i] * draw[i])
                }
                None => tint,
            };
            let vertices = call.vertices;
            let material = self.material_file(&path)?;
            let (material, overlay) = match material {
                WorldMaterial::Tool => continue,
                WorldMaterial::Visible(file) => {
                    let overlay = is_overlay(&file);
                    (Some(self.material(&file, tint)?), overlay)
                }
                WorldMaterial::Missing => (None, false),
            };
            let buffer = self.infos[model].meshes[mesh]
                .buffers
                .get(vertices)
                .ok_or_else(|| malformed("world", format!("{name}: vertex buffer {vertices}")))?;
            bounds = union(bounds, buffer[usize::from(overlay)]);
            primitives.push(ScenePrimitive {
                draw_call: d,
                material,
                overlay,
            });
        }
        if primitives.is_empty() {
            return Ok(None);
        }
        check_count(self.scene.meshes.len(), "meshes")?;
        self.budget.bytes(name.len() + size_of::<SceneMesh>())?;
        self.scene.meshes.push(SceneMesh {
            name,
            model,
            mesh,
            set,
            primitives,
            bounds,
        });
        Ok(Some(self.scene.meshes.len() - 1))
    }

    fn push_node(&mut self, name: String, mesh: usize, affine: &Affine) -> Result<()> {
        check_count(self.scene.nodes.len(), "nodes")?;
        self.budget.bytes(name.len() + size_of::<SceneNode>())?;
        self.scene.nodes.push(SceneNode {
            name,
            mesh,
            matrix: place::node_matrix(affine),
        });
        Ok(())
    }

    fn new_set(&mut self) -> usize {
        self.sets += 1;
        self.sets - 1
    }

    /// The render material drawn with `tint`, made once per path and tint.
    fn material(&mut self, file: &Material, tint: [f32; 4]) -> Result<usize> {
        let key = (file.path.clone(), tint.map(f32::to_bits));
        if let Some(&known) = self.material_index.get(&key) {
            return Ok(known);
        }
        let gltf = convert_world(file, tint, &mut self.scene.images, self.texture_uri)?;
        let index = self.scene.materials.len();
        check_count(index, "materials")?;
        self.budget.bytes(file.json_size())?;
        self.scene.materials.push(SceneMaterial {
            gltf,
            textures: file.textures.iter().map(|(_, t)| t.clone()).collect(),
            shader: file.shader.clone(),
        });
        self.material_index.insert(key, index);
        Ok(index)
    }

    /// The material file `path` (without `_c`), read once.
    fn material_file(&mut self, path: &str) -> Result<WorldMaterial> {
        if let Some(known) = self.material_files.get(path) {
            return Ok(known.clone());
        }
        let material = match self.data(&compiled(path))? {
            Some(data) if is_tool_material(&data) => WorldMaterial::Tool,
            Some(data) => WorldMaterial::Visible(Rc::new(Material::from_vmat(path, &data)?)),
            None => WorldMaterial::Missing,
        };
        self.material_files
            .insert(path.to_owned(), material.clone());
        Ok(material)
    }
}

/// The editor marks navigation surfaces, triggers and other tool geometry with this attribute.
fn is_tool_material(data: &Value) -> bool {
    data.array("m_intAttributes")
        .iter()
        .any(|attribute| attribute.str_field("m_name") == Some("tools.toolsmaterial"))
}

/// What the scene needs of a model.
fn info(model: &Model) -> Result<ModelInfo> {
    let meshes = model
        .meshes
        .iter()
        .map(|mesh| {
            Ok(MeshInfo {
                name: mesh.name.clone(),
                draw_calls: mesh
                    .primitives
                    .iter()
                    .map(|p| DrawCall {
                        vertices: p.vertices,
                        material: p.material.clone(),
                        tint: p.tint,
                    })
                    .collect(),
                buffers: mesh
                    .vertices
                    .iter()
                    .map(|v| {
                        let lifted = overlay_positions(v)?;
                        Ok([bounds_of(&v.positions), bounds_of(&lifted)])
                    })
                    .collect::<Result<_>>()?,
            })
        })
        .collect::<Result<_>>()?;
    Ok(ModelInfo {
        meshes,
        groups: model.material_groups.clone(),
    })
}

/// The positions of `vertices` lifted along their normals, as an overlay's are.
pub(super) fn overlay_positions(vertices: &Vertices) -> Result<Vec<[f32; 3]>> {
    let Some(normals) = &vertices.normals else {
        return Ok(vertices.positions.clone());
    };
    if normals.len() != vertices.positions.len() {
        return Err(malformed(
            "world",
            "overlay normals and positions have different counts",
        ));
    }
    Ok(vertices
        .positions
        .iter()
        .zip(normals)
        .map(|(p, n)| {
            std::array::from_fn(|i| {
                let v = p[i] + n[i] * OVERLAY_LIFT;
                if v.is_finite() { v } else { 0.0 }
            })
        })
        .collect())
}

/// Whether the material is drawn over other geometry (decals, path borders).
fn is_overlay(material: &Material) -> bool {
    material.int("F_OVERLAY") == 1
        || material.int("F_DEPTHBIAS") == 1
        || material.int("F_DEPTH_BIAS") == 1
        || material.shader.ends_with("static_overlay.vfx")
        || material.shader == "citadel_overlay.vfx"
}

pub(super) fn bounds_of(points: &[[f32; 3]]) -> Option<Bounds> {
    let mut out: Option<Bounds> = None;
    for p in points {
        let b = out.get_or_insert([*p, *p]);
        for i in 0..3 {
            b[0][i] = b[0][i].min(p[i]);
            b[1][i] = b[1][i].max(p[i]);
        }
    }
    out
}

fn union(a: Option<Bounds>, b: Option<Bounds>) -> Option<Bounds> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            std::array::from_fn(|i| a[0][i].min(b[0][i])),
            std::array::from_fn(|i| a[1][i].max(b[1][i])),
        ]),
        (a, b) => a.or(b),
    }
}

fn check_count(count: usize, what: &str) -> Result<()> {
    if count >= MAX_ITEMS {
        return Err(malformed("world", format!("more than {MAX_ITEMS} {what}")));
    }
    Ok(())
}

pub(super) fn vector4(value: Option<&Value>) -> [f32; 4] {
    let items = value.map_or(&[][..], Value::slice);
    std::array::from_fn(|i| items.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32)
}

/// The scene object index addresses the regular placements, separately from aggregates.
fn world_material_overrides(node: &Value) -> Result<HashMap<usize, PlacementOverrides<'_>>> {
    let rows = node.array("m_materialOverrides");
    check_count(rows.len(), "material overrides")?;
    let mut by_object = HashMap::<usize, PlacementOverrides<'_>>::new();
    for row in rows {
        let (object, mesh, draw, material) = material_override_row(row)?;
        let overrides = by_object.entry(object).or_default();
        if let Some(draw) = draw {
            overrides
                .meshes
                .entry(mesh)
                .or_default()
                .insert(draw, material);
        } else {
            overrides.whole.get_or_insert(material);
        }
    }
    Ok(by_object)
}

pub(super) fn material_override_row(row: &Value) -> Result<(usize, usize, Option<usize>, &str)> {
    let index = |key| {
        row.get(key)
            .and_then(Value::as_u64)
            .and_then(|number| u32::try_from(number).ok())
            .ok_or_else(|| malformed("world", format!("material override has no valid {key}")))
    };
    let material = row
        .str_field("m_pMaterial")
        .filter(|path| !path.is_empty())
        .ok_or_else(|| malformed("world", "material override has no material"))?;
    let object = index("m_nSceneObjectIndex")? as usize;
    let mesh = index("m_nSubSceneObject")? as usize;
    let draw = index("m_nDrawCallIndex")?;
    Ok((
        object,
        mesh,
        (draw != u32::MAX).then_some(draw as usize),
        material,
    ))
}

/// The compiled file of resource `path` (`x.vmdl` is `x.vmdl_c`).
pub(super) fn compiled(path: &str) -> String {
    if path.ends_with("_c") {
        path.to_owned()
    } else {
        format!("{path}_c")
    }
}

/// A path's file name without its extension.
fn stem(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv3::Object;
    use crate::source::SliceSource;

    fn mesh_info(name: &str, draw_calls: Vec<DrawCall>) -> ModelInfo {
        ModelInfo {
            meshes: vec![MeshInfo {
                name: name.into(),
                draw_calls,
                buffers: vec![[None, None]],
            }],
            groups: Vec::new(),
        }
    }

    fn mesh_spec<'a>(
        name: &str,
        draw_calls: std::ops::Range<usize>,
        tint: [f32; 4],
        overrides: Option<&'a PlacementOverrides<'a>>,
    ) -> MeshSpec<'a> {
        MeshSpec {
            model: 0,
            mesh: 0,
            name: name.into(),
            set: 0,
            draw_calls,
            tint,
            skin: None,
            overrides,
        }
    }

    #[test]
    fn world_meshes_keep_visible_draw_calls_and_omit_editor_surfaces() {
        let data = override_row(&[(
            "m_intAttributes",
            Value::Array(vec![override_row(&[
                ("m_name", Value::String("tools.toolsmaterial".into())),
                ("m_nValue", Value::Int(1)),
            ])]),
        )]);
        assert!(is_tool_material(&data));
        let source = SliceSource::<Vec<u8>>::default();
        let mut builder = Builder::new(&source, &[], &str::to_owned);
        builder
            .material_files
            .insert("tool.vmat".into(), WorldMaterial::Tool);
        builder.infos.push(mesh_info(
            "surface",
            ["tool.vmat", "visible.vmat"]
                .map(|m| DrawCall {
                    vertices: 0,
                    material: m.into(),
                    tint: None,
                })
                .into(),
        ));
        assert!(
            builder
                .make_mesh(mesh_spec("surface", 0..1, [1.0; 4], None))
                .unwrap()
                .is_none()
        );
        let kept = builder
            .make_mesh(mesh_spec("surface", 0..2, [1.0; 4], None))
            .unwrap()
            .expect("a visible surface");
        let draws = &builder.scene.meshes[kept].primitives;
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].draw_call, 1);
    }

    #[test]
    fn a_world_skin_keeps_the_material_slots_of_a_multi_material_mesh() {
        let model = ModelInfo {
            meshes: Vec::new(),
            groups: vec![
                ("0".into(), vec!["base".into(), "trim".into()]),
                ("1".into(), vec!["dark_base".into(), "dark_trim".into()]),
            ],
        };
        assert_eq!(model.material("base", Some(1)), "dark_base");
        assert_eq!(model.material("trim", Some(1)), "dark_trim");
        assert_eq!(model.material("overlay", Some(1)), "overlay");
    }

    #[test]
    fn an_overlay_is_lifted_one_centimetre_above_its_surface() {
        let vertices = Vertices {
            positions: vec![[15.0, 0.0, 25.0]],
            normals: Some(vec![[0.0, 1.0, 0.0]]),
            ..Vertices::default()
        };
        assert_eq!(overlay_positions(&vertices).unwrap(), [[15.0, 0.01, 25.0]]);
    }

    fn override_row(entries: &[(&str, Value)]) -> Value {
        let mut object = Object::default();
        for (key, value) in entries {
            object.push((*key).to_owned(), value.clone());
        }
        Value::Object(object)
    }

    fn override_table(rows: Vec<Value>) -> Value {
        override_row(&[("m_materialOverrides", Value::Array(rows))])
    }

    #[test]
    fn override_rows_require_nonnegative_indices_and_a_material() {
        let valid = [
            ("m_nSceneObjectIndex", Value::UInt(0)),
            ("m_nSubSceneObject", Value::UInt(0)),
            ("m_nDrawCallIndex", Value::UInt(0)),
            ("m_pMaterial", Value::String("custom.vmat".into())),
        ];
        for omitted in 0..valid.len() {
            let mut fields = valid.to_vec();
            fields.remove(omitted);
            assert!(
                world_material_overrides(&override_table(vec![override_row(&fields)])).is_err()
            );
        }
        for (index, invalid) in [(2, Value::Int(-1)), (3, Value::String(String::new()))] {
            let mut fields = valid.clone();
            fields[index].1 = invalid;
            assert!(
                world_material_overrides(&override_table(vec![override_row(&fields)])).is_err()
            );
        }
        assert!(
            world_material_overrides(&override_table(vec![Value::Null; MAX_ITEMS + 1])).is_err()
        );
    }

    #[test]
    fn large_override_tables_charge_for_filtered_draws_before_exporting_them() {
        const DRAWS: usize = 16_384;
        let table = override_table(
            (0..DRAWS)
                .map(|draw| {
                    override_row(&[
                        ("m_nSceneObjectIndex", Value::UInt(0)),
                        ("m_nSubSceneObject", Value::UInt(0)),
                        ("m_nDrawCallIndex", Value::UInt(draw as u64)),
                        ("m_pMaterial", Value::String("tool.vmat".into())),
                    ])
                })
                .collect(),
        );
        let overrides = world_material_overrides(&table).unwrap();
        let indexed = &overrides[&0];
        for draw in (0..DRAWS).rev() {
            assert_eq!(indexed.material(0, draw), Some("tool.vmat"));
        }
        assert_eq!(indexed.material(0, DRAWS), None);
        assert_eq!(indexed.material(1, 0), None);
        let source = SliceSource::<Vec<u8>>::default();
        let mut builder = Builder::new(&source, &[], &str::to_owned);
        builder.budget = Budget::new(128);
        builder
            .material_files
            .insert("tool.vmat".into(), WorldMaterial::Tool);
        builder.infos.push(mesh_info(
            "large",
            (0..DRAWS)
                .map(|_| DrawCall {
                    vertices: 0,
                    material: "tool.vmat".into(),
                    tint: None,
                })
                .collect(),
        ));
        let result = builder.make_mesh(mesh_spec("large", 0..DRAWS, [1.0; 4], overrides.get(&0)));
        assert!(result.unwrap_err().to_string().contains("more than it may"));
    }

    #[test]
    fn world_draw_and_placement_tints_multiply_in_linear_space() {
        let source = SliceSource::<Vec<u8>>::default();
        let mut builder = Builder::new(&source, &[], &str::to_owned);
        let material =
            Material::from_vmat("surface.vmat", &Value::Object(Object::default())).unwrap();
        builder.material_files.insert(
            "surface.vmat".into(),
            WorldMaterial::Visible(Rc::new(material)),
        );
        builder.infos.push(mesh_info(
            "surface",
            vec![DrawCall {
                vertices: 0,
                material: "surface.vmat".into(),
                tint: Some([0.5; 4]),
            }],
        ));
        builder
            .make_mesh(mesh_spec("surface", 0..1, linear_tint([0.5; 4]), None))
            .unwrap();
        let color = builder.scene.materials[0].gltf.base_color;
        for channel in &color[..3] {
            assert!((*channel - 0.045_814).abs() < 0.000_01, "{color:?}");
        }
        assert_eq!(color[3], 0.25);
    }

    #[test]
    fn a_short_normal_array_cannot_drop_an_overlay_vertex() {
        let vertices = Vertices {
            positions: vec![[0.0; 3]; 2],
            normals: Some(vec![[0.0, 1.0, 0.0]]),
            ..Vertices::default()
        };
        assert!(
            overlay_positions(&vertices)
                .unwrap_err()
                .to_string()
                .contains("different counts")
        );
    }

    #[test]
    fn world_read_budget_is_separate_from_scene_budget_and_never_swallowed() {
        let mut tree = Vec::new();
        let mut data = Vec::new();
        for i in 0..5 {
            let payload = [0u8; 1024];
            tree.extend(b"vmdl_c\0m\0");
            tree.extend(format!("m{i}\0").as_bytes());
            tree.extend(0u32.to_le_bytes());
            tree.extend(0u16.to_le_bytes());
            tree.extend(0x7FFFu16.to_le_bytes());
            tree.extend((data.len() as u32).to_le_bytes());
            tree.extend((payload.len() as u32).to_le_bytes());
            tree.extend(0xFFFFu16.to_le_bytes());
            tree.extend(b"\0\0");
            data.extend(payload);
        }
        tree.push(0);
        let mut vpk_bytes = Vec::new();
        for w in [
            0x55AA_1234u32,
            2,
            tree.len() as u32,
            data.len() as u32,
            0,
            0,
            0,
        ] {
            vpk_bytes.extend(w.to_le_bytes());
        }
        vpk_bytes.extend(tree);
        vpk_bytes.extend(data);
        let mut src = SliceSource::default();
        src.insert("p_dir.vpk", vpk_bytes);
        let vpk = Vpk::open(&src, "p_dir.vpk").unwrap();
        let pkgs = [&vpk];
        let mut builder = Builder::new(&src, &pkgs, &str::to_owned);
        builder.budget = Budget::with_limit("world", 2048);
        builder.max_read = 5000;
        for i in 0..4 {
            assert!(builder.model(&format!("m/m{i}.vmdl")).unwrap().is_none());
        }
        assert!(builder.total_read > 2048 && builder.scene.skipped_models.len() == 4);
        let err = builder.model("m/m4.vmdl").unwrap_err().to_string();
        assert!(err.contains("reading more than 5000 bytes"), "{err}");
        assert_eq!(builder.scene.skipped_models.len(), 4);
    }

    fn lump_data(entities: Vec<Value>, children: &[&str]) -> Value {
        let entity_kvs: Vec<Value> = entities
            .into_iter()
            .map(|values| Value::object([("keyValues3Data", Value::object([("values", values)]))]))
            .collect();
        Value::object([
            ("m_entityKeyValues", Value::Array(entity_kvs)),
            ("m_childLumps", Value::from_array(children.iter().copied())),
        ])
    }

    #[test]
    fn active_entity_lumps_loads_enabled_layers_and_skips_disabled_and_cycles() {
        let mut files: HashMap<String, Value> = HashMap::new();
        files.insert(
            "maps/test/world.vwrld_c".into(),
            Value::object([(
                "m_entityLumps",
                Value::from_array(["maps/test/entities/default_ents.vents"]),
            )]),
        );
        files.insert(
            "maps/test/entities/default_ents.vents_c".into(),
            lump_data(
                vec![
                    Value::object([
                        ("classname", Value::from("info_world_layer")),
                        ("layerName", Value::from("enabled_int")),
                        ("spawnflags", Value::from(1)),
                    ]),
                    Value::object([
                        ("classname", Value::from("info_world_layer")),
                        ("layerName", Value::from("enabled_str")),
                        ("spawnflags", Value::from("3")),
                    ]),
                    Value::object([
                        ("classname", Value::from("info_world_layer")),
                        ("layerName", Value::from("disabled_absent")),
                    ]),
                    Value::object([
                        ("classname", Value::from("info_world_layer")),
                        ("layerName", Value::from("disabled_zero")),
                        ("spawnflags", Value::from(2)),
                    ]),
                ],
                &[
                    "maps/test/entities/enabled_int.vents",
                    "maps/test/entities/enabled_str.vents",
                    "maps/test/entities/disabled_absent.vents",
                    "maps/test/entities/disabled_zero.vents",
                ],
            ),
        );
        // enabled_int points back to default_ents (cycle!) and has its own entity.
        files.insert(
            "maps/test/entities/enabled_int.vents_c".into(),
            lump_data(
                vec![
                    Value::object([("classname", Value::from("prop_dynamic"))]),
                    Value::object([
                        ("classname", Value::from("info_world_layer")),
                        ("layerName", Value::from("default_ents")),
                        ("spawnflags", Value::from(1)),
                    ]),
                ],
                &["maps/test/entities/default_ents.vents"],
            ),
        );
        files.insert(
            "maps/test/entities/enabled_str.vents_c".into(),
            lump_data(
                vec![Value::object([(
                    "classname",
                    Value::from("env_soundscape"),
                )])],
                &[],
            ),
        );
        files.insert(
            "maps/test/entities/disabled_absent.vents_c".into(),
            lump_data(
                vec![Value::object([(
                    "classname",
                    Value::from("should_not_load"),
                )])],
                &[],
            ),
        );
        let mut load = |p: &str, _: &mut Budget| Ok(files.get(p).cloned());
        let mut budget = Budget::with_limit("world", MAX_SCENE_BYTES);
        let lumps =
            active_entity_lumps_with("maps/test/world.vwrld_c", &mut budget, &mut load).unwrap();
        assert_eq!(lumps.len(), 3);
        assert_eq!(
            lumps[1].entities[0].str_field("classname"),
            Some("prop_dynamic")
        );
        assert_eq!(
            lumps[2].entities[0].str_field("classname"),
            Some("env_soundscape")
        );
    }

    #[test]
    fn active_entity_lumps_rejects_bad_spawnflags_traversal_and_lump_overflow() {
        let mut files: HashMap<String, Value> = HashMap::new();
        files.insert(
            "maps/test/world.vwrld_c".into(),
            Value::object([(
                "m_entityLumps",
                Value::from_array(["maps/test/entities/default_ents.vents"]),
            )]),
        );
        files.insert(
            "maps/test/entities/default_ents.vents_c".into(),
            lump_data(
                vec![Value::object([
                    ("classname", Value::from("info_world_layer")),
                    ("layerName", Value::from("bad")),
                    ("spawnflags", Value::from("not_a_number")),
                ])],
                &[],
            ),
        );
        let mut load = |p: &str, _: &mut Budget| Ok(files.get(p).cloned());
        let mut budget = Budget::with_limit("world", MAX_SCENE_BYTES);
        assert!(
            active_entity_lumps_with("maps/test/world.vwrld_c", &mut budget, &mut load).is_err()
        );

        // Path traversal, backslash, leading slash, dot, empty segment, and control chars rejected.
        for bad_path in [
            "maps/../escape.vents",
            "maps\\..\\escape.vents",
            "maps\\test\\a.vents",
            "/maps/test/a.vents",
            "maps/./a.vents",
            "maps//a.vents",
            "maps/a.vents/",
            "maps/a\0.vents",
            "maps/a\n.vents",
        ] {
            let mut bad: HashMap<String, Value> = HashMap::new();
            bad.insert(
                "maps/test/world.vwrld_c".into(),
                Value::object([("m_entityLumps", Value::from_array([bad_path]))]),
            );
            let mut load = |p: &str, _: &mut Budget| Ok(bad.get(p).cloned());
            let mut budget = Budget::with_limit("world", MAX_SCENE_BYTES);
            assert!(
                active_entity_lumps_with("maps/test/world.vwrld_c", &mut budget, &mut load)
                    .is_err(),
                "expected {bad_path:?} to be rejected"
            );
        }

        // Count limit enforced on loaded lumps.
        let mut many: HashMap<String, Value> = HashMap::new();
        let names: Vec<String> = (0..=MAX_ENTITY_LUMPS)
            .map(|i| format!("maps/test/entities/l{i}.vents"))
            .collect();
        many.insert(
            "maps/test/world.vwrld_c".into(),
            Value::object([(
                "m_entityLumps",
                Value::from_array(names.iter().map(String::as_str)),
            )]),
        );
        for name in &names {
            many.insert(format!("{name}_c"), lump_data(vec![], &[]));
        }
        let mut load = |p: &str, _: &mut Budget| Ok(many.get(p).cloned());
        let mut budget = Budget::with_limit("world", MAX_SCENE_BYTES);
        assert!(
            active_entity_lumps_with("maps/test/world.vwrld_c", &mut budget, &mut load).is_err()
        );

        // Visited path cap enforced even when all referenced lumps are missing.
        let missing_names: Vec<String> = (0..=MAX_VISITED_LUMPS)
            .map(|i| format!("maps/test/entities/missing_{i}.vents"))
            .collect();
        let mut missing_world: HashMap<String, Value> = HashMap::new();
        missing_world.insert(
            "maps/test/world.vwrld_c".into(),
            Value::object([(
                "m_entityLumps",
                Value::from_array(missing_names.iter().map(String::as_str)),
            )]),
        );
        let mut load = |p: &str, _: &mut Budget| Ok(missing_world.get(p).cloned());
        let mut budget = Budget::with_limit("world", MAX_SCENE_BYTES);
        let err = active_entity_lumps_with("maps/test/world.vwrld_c", &mut budget, &mut load)
            .err()
            .expect("visited cap exceeded")
            .to_string();
        assert!(err.contains("entity lump paths"), "{err}");

        // Tight budget rejects queueing lump paths before inserting.
        let mut tiny_budget = Budget::with_limit("world", 32);
        let err = active_entity_lumps_with("maps/test/world.vwrld_c", &mut tiny_budget, &mut load)
            .err()
            .expect("budget exceeded")
            .to_string();
        assert!(err.contains("more than it may"), "{err}");
    }
}
