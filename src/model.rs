//! A compiled model (`.vmdl_c`): its skeleton and the render meshes of its most detailed level
//! of detail, in glTF's frame (Y up, metres; Source is Z up, inches).

use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::face::{MorphSet, Target};
use crate::kv3::{self, Value};
use crate::resource::Resource;
use crate::vbib::{self, Buffer};
use crate::vertices::{SCALE, TO_GLTF, Vertices, convert};

/// Most bytes a model's meshes may take as they are read: decoded buffers, converted vertices
/// and draw calls' indices. The largest of the install's models takes ~20 MB.
const MAX_MODEL_BYTES: usize = 128 << 20;

/// A model (`vmdl_c`): its skeleton, render meshes and skins, in glTF's frame (metres, Y up).
pub struct Model {
    /// The skeleton, parents before children.
    pub bones: Vec<Bone>,
    /// The render meshes.
    pub meshes: Vec<Mesh>,
    /// The skins: each a name and the materials it swaps in.
    pub material_groups: Vec<(String, Vec<String>)>,
    /// The attachments: named points the bones carry (`attach_attack1` where a missile leaves,
    /// `attach_hitloc` where one hits), each name once.
    pub attachments: Vec<Attachment>,
}

/// An attachment's rest place, relative to the bone that carries it, in glTF's frame.
pub struct Attachment {
    /// Its name.
    pub name: String,
    /// The bone that carries it (its strongest influence), in [`Model::bones`]; none for the
    /// model's root.
    pub bone: Option<usize>,
    /// Translation from that bone.
    pub translation: [f32; 3],
    /// Rotation from that bone, `[x, y, z, w]`.
    pub rotation: [f32; 4],
}

/// Most attachments a model has; Dota's have up to a few dozen.
const MAX_ATTACHMENTS: usize = 256;

/// A bone's rest pose relative to its parent, in glTF's frame.
pub struct Bone {
    /// The bone's name.
    pub name: String,
    /// Its parent's index in [`Model::bones`]; none for a root.
    pub parent: Option<usize>,
    /// Translation from its parent.
    pub translation: [f32; 3],
    /// Rotation from its parent, `[x, y, z, w]`.
    pub rotation: [f32; 4],
}

/// A render mesh: vertex buffers and the draw calls over them.
pub struct Mesh {
    /// The embedded mesh's name (`pudge_body_lod0`).
    pub name: String,
    /// Its vertex buffers.
    pub vertices: Vec<Vertices>,
    /// Its draw calls.
    pub primitives: Vec<Primitive>,
    /// Its morph set, when it has one (a face).
    pub morph: Option<MorphSet>,
    /// The morph targets of its vertex buffer, out of the morph set's atlas (filled by the
    /// export, which reads the atlas): the flexes that move a vertex.
    pub targets: Vec<Target>,
}

/// One draw call: triangles over one of the mesh's vertex buffers with one material.
pub struct Primitive {
    /// The index of its vertex buffer in [`Mesh::vertices`].
    pub vertices: usize,
    /// Triangle-list indices into that buffer.
    pub indices: Vec<u32>,
    /// The material's path (`materials/.../x.vmat`, without `_c`).
    pub material: String,
    /// The draw call's own tint and opacity, when it has them.
    pub tint: Option<[f32; 4]>,
}

impl Model {
    /// Reads the model of a parsed `vmdl_c`: its embedded meshes (meshopt, zstd or `VBIB` buffers), skeleton and skins. One larger than any of the game's is an error.
    pub fn read(resource: &Resource) -> Result<Self> {
        let data = resource
            .data()
            .ok_or_else(|| malformed("model", "no DATA"))??;
        let ctrl = resource
            .kv3(b"CTRL")
            .ok_or_else(|| malformed("model", "no CTRL"))??
            .root;
        let bones = skeleton(&data)?;
        let masks = integers(data.get("m_refLODGroupMasks"));
        let lowest = masks
            .iter()
            .fold(0, |all, m| all | m)
            .trailing_zeros()
            .min(63);
        let remap = Remap {
            starts: integers(data.get("m_remappingTableStarts")),
            table: integers(data.get("m_remappingTable")),
        };
        let mut meshes = Vec::new();
        let mut attachments = Vec::new();
        let mut budget = Budget::with_limit("model", MAX_MODEL_BYTES);
        for (i, embedded) in array(ctrl.get("embedded_meshes")).iter().enumerate() {
            let index = number(embedded, &["m_nMeshIndex", "mesh_index"]).unwrap_or(i as i64);
            let in_lod = usize::try_from(index)
                .ok()
                .and_then(|i| masks.get(i))
                .is_none_or(|mask| mask >> lowest & 1 != 0);
            if in_lod {
                let joints = remap.of(index, bones.len());
                let (mesh, data) = read_mesh(resource, embedded, joints.as_deref(), &mut budget)?;
                add_attachments(&data, &bones, &mut attachments)?;
                meshes.push(mesh);
            }
        }
        let material_groups = array(data.get("m_materialGroups"))
            .iter()
            .map(|g| {
                let name = g.get("m_name").and_then(Value::as_str).unwrap_or_default();
                let materials = array(g.get("m_materials"))
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                (name.to_owned(), materials)
            })
            .collect();
        Ok(Self {
            bones,
            meshes,
            material_groups,
            attachments,
        })
    }
}

impl Model {
    /// Swaps every draw call's material for the one skin `group` lists where the default skin
    /// (the first group) lists the call's; a call whose material is not in the default skin keeps it.
    pub fn use_material_group(&mut self, group: &str) -> Result<()> {
        let (Some((_, default)), Some((_, skin))) = (
            self.material_groups.first(),
            self.material_groups.iter().find(|(name, _)| name == group),
        ) else {
            return Err(malformed("model", format!("no material group {group:?}")));
        };
        for primitive in self.meshes.iter_mut().flat_map(|m| &mut m.primitives) {
            let place = default.iter().position(|m| *m == primitive.material);
            if let Some(material) = place.and_then(|i| skin.get(i)) {
                primitive.material.clone_from(material);
            }
        }
        Ok(())
    }
}

pub(crate) fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).unwrap_or_default()
}

fn integers(value: Option<&Value>) -> Vec<i64> {
    array(value).iter().filter_map(Value::as_i64).collect()
}

/// The first of `keys` present as an integer (newer and older models name fields differently).
fn number(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|k| value.get(k).and_then(Value::as_i64))
}

/// Per mesh, the model bones its vertex bone indices stand for.
struct Remap {
    starts: Vec<i64>,
    table: Vec<i64>,
}

impl Remap {
    fn of(&self, mesh: i64, bones: usize) -> Option<Vec<u16>> {
        let i = usize::try_from(mesh).ok()?;
        let start = usize::try_from(*self.starts.get(i)?).ok()?;
        let end = self
            .starts
            .get(i + 1)
            .and_then(|&e| usize::try_from(e).ok())
            .unwrap_or(self.table.len());
        let slice = self.table.get(start..end)?;
        // An index the table cannot map clamps to bone 0, as in Valve's reader.
        Some(
            slice
                .iter()
                .map(|&b| {
                    u16::try_from(b)
                        .ok()
                        .filter(|&b| usize::from(b) < bones)
                        .unwrap_or(0)
                })
                .collect(),
        )
    }
}

/// A bone's rest pose relative to its parent as the model stores it (Source's frame, inches).
pub(crate) struct SourceBone {
    pub name: String,
    pub parent: Option<usize>,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// The model skeleton of DATA's `m_modelSkeleton`, in bone order. A parent index that does not
/// come before its bone makes the bone a root.
pub(crate) fn source_skeleton(data: &Value) -> Result<Vec<SourceBone>> {
    let skeleton = data.get("m_modelSkeleton");
    let get = |key| array(skeleton.and_then(|s| s.get(key)));
    let (names, parents) = (get("m_boneName"), get("m_nParent"));
    let (positions, rotations) = (get("m_bonePosParent"), get("m_boneRotParent"));
    let mut bones = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let parent = parents
            .get(i)
            .and_then(Value::as_i64)
            .and_then(|p| usize::try_from(p).ok())
            .filter(|&p| p < i);
        bones.push(SourceBone {
            name: name.as_str().unwrap_or_default().to_owned(),
            parent,
            position: floats::<3>(positions.get(i))?,
            rotation: floats::<4>(rotations.get(i))?,
        });
    }
    Ok(bones)
}

/// A bone-local transform in glTF's frame: the translation scaled to metres and, on a root
/// bone, the frame change baked in (children inherit it through the hierarchy).
pub(crate) fn to_gltf(position: [f32; 3], rotation: [f32; 4], root: bool) -> ([f32; 3], [f32; 4]) {
    let translation = position.map(|v| v * SCALE);
    if root {
        (rotate(TO_GLTF, translation), multiply(TO_GLTF, rotation))
    } else {
        (translation, rotation)
    }
}

fn skeleton(data: &Value) -> Result<Vec<Bone>> {
    Ok(source_skeleton(data)?
        .into_iter()
        .map(|bone| {
            let (translation, rotation) =
                to_gltf(bone.position, bone.rotation, bone.parent.is_none());
            Bone {
                name: bone.name,
                parent: bone.parent,
                translation,
                rotation,
            }
        })
        .collect())
}

pub(crate) fn floats<const N: usize>(value: Option<&Value>) -> Result<[f32; N]> {
    let items = array(value);
    let mut out = [0.0; N];
    if items.len() != N {
        return Err(malformed(
            "model",
            format!("a bone vector of {}", items.len()),
        ));
    }
    for (o, v) in out.iter_mut().zip(items) {
        *o = finite(v.as_f64().unwrap_or(0.0) as f32);
    }
    Ok(out)
}

/// A number as glTF may hold it: NaN and infinities (only a damaged file has them) are 0.
pub(crate) fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// Rotates `v` by the unit quaternion `q` (x, y, z, w).
pub(crate) fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// The quaternion product `a * b` (apply `b`, then `a`).
pub(crate) fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// The attachments of a mesh's data block (`m_attachments`) that `out` lacks, carried by the
/// strongest influence of each.
fn add_attachments(data: &Value, bones: &[Bone], out: &mut Vec<Attachment>) -> Result<()> {
    for entry in array(data.get("m_attachments")) {
        let value = entry.get("value").unwrap_or(entry);
        let name = value
            .get("m_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.is_empty() || out.iter().any(|a| a.name == name) {
            continue;
        }
        if out.len() == MAX_ATTACHMENTS {
            return Err(malformed("model", "more attachments than any model has"));
        }
        let names = array(value.get("m_influenceNames"));
        let weights = array(value.get("m_influenceWeights"));
        let count = value
            .get("m_nInfluences")
            .and_then(Value::as_u64)
            .unwrap_or(1) as usize;
        let strongest = (0..count.min(names.len()).max(1))
            .max_by(|&a, &b| {
                let weight = |i: usize| weights.get(i).and_then(Value::as_f64).unwrap_or(1.0);
                weight(a).total_cmp(&weight(b)).then(b.cmp(&a))
            })
            .unwrap_or(0);
        let bone_name = names
            .get(strongest)
            .and_then(Value::as_str)
            .unwrap_or_default();
        let bone = bones
            .iter()
            .position(|b| !bone_name.is_empty() && b.name == bone_name);
        let offset = floats::<3>(array(value.get("m_vInfluenceOffsets")).get(strongest))?;
        let turn = floats::<4>(array(value.get("m_vInfluenceRotations")).get(strongest))?;
        let (translation, rotation) = to_gltf(offset, turn, bone.is_none());
        out.push(Attachment {
            name: name.to_owned(),
            bone,
            translation,
            rotation,
        });
    }
    Ok(())
}

/// One embedded mesh: its buffers (from `CTRL`, or an older binary `VBIB` block) and the draw
/// calls of its `MDAT`/`DATA` block, what they take charged to `budget`; and that block.
fn read_mesh(
    resource: &Resource,
    embedded: &Value,
    joints: Option<&[u16]>,
    budget: &mut Budget,
) -> Result<(Mesh, Value)> {
    let name = ["m_Name", "name"]
        .iter()
        .find_map(|k| embedded.get(k).and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned();
    let block = |keys: &[&str]| {
        number(embedded, keys)
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| resource.blocks().get(i))
    };
    let (vertex_buffers, index_buffers) = match block(&["m_nVBIBBlock", "vbib_block"]) {
        Some(vbib) => vbib::from_vbib(resource.bytes(vbib), budget)?,
        None => (
            buffers(resource, embedded.get("m_vertexBuffers"), true, budget)?,
            buffers(resource, embedded.get("m_indexBuffers"), false, budget)?,
        ),
    };
    let data = block(&["m_nDataBlock", "data_block"])
        .ok_or_else(|| malformed("model", format!("mesh {name} has no data block")))?;
    let data = kv3::decode(resource.bytes(data))?.root;
    let weight_count = data
        .get("m_skeleton")
        .and_then(|s| s.get("m_nBoneWeightCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let vertices = vertex_buffers
        .iter()
        .map(|b| convert(b, joints.filter(|_| weight_count > 0), weight_count, budget))
        .collect::<Result<_>>()?;
    let index_lists = index_buffers
        .iter()
        .map(|b| {
            budget.bytes(b.count.saturating_mul(4))?;
            b.indices()
        })
        .collect::<Result<Vec<_>>>()?;
    let mut primitives = Vec::new();
    for object in array(data.get("m_sceneObjects")) {
        for call in array(object.get("m_drawCalls")) {
            primitives.push(draw_call(call, &index_lists, &vertex_buffers, budget)?);
        }
    }
    let atlas = embedded.get("morph_texture").and_then(Value::as_str);
    let morph = match (block(&["m_nMorphBlock"]), atlas) {
        (Some(set), Some(atlas)) => Some(MorphSet::read(
            &kv3::decode(resource.bytes(set))?.root,
            atlas,
        )?),
        _ => None,
    };
    Ok((
        Mesh {
            name,
            vertices,
            primitives,
            morph,
            targets: Vec::new(),
        },
        data,
    ))
}

fn buffers(
    resource: &Resource,
    list: Option<&Value>,
    vertex: bool,
    budget: &mut Budget,
) -> Result<Vec<Buffer>> {
    array(list)
        .iter()
        .map(|desc| vbib::from_ctrl(resource, desc, vertex, budget))
        .collect()
}

/// A draw call: its indices (a copy of its range of the index buffer, charged to `budget`),
/// vertex buffer, material and tint.
fn draw_call(
    call: &Value,
    index_lists: &[Vec<u32>],
    vertex_buffers: &[Buffer],
    budget: &mut Budget,
) -> Result<Primitive> {
    let int = |v: Option<&Value>| {
        v.and_then(Value::as_i64)
            .and_then(|v| usize::try_from(v).ok())
    };
    let field = |key| int(call.get(key)).ok_or_else(|| malformed("draw call", format!("no {key}")));
    let handle = |key: &str| {
        let slot = call.get(key);
        let slot = slot
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .or(slot);
        int(slot.and_then(|s| s.get("m_hBuffer"))).unwrap_or(0)
    };
    if let Some(kind) = call.get("m_nPrimitiveType").and_then(Value::as_str)
        && kind != "RENDER_PRIM_TRIANGLES"
    {
        return Err(malformed("draw call", format!("primitive {kind}")));
    }
    let (start, count, base) = (
        field("m_nStartIndex")?,
        field("m_nIndexCount")?,
        field("m_nBaseVertex")?,
    );
    let vertices = handle("m_vertexBuffers");
    let vertex_count = vertex_buffers
        .get(vertices)
        .ok_or_else(|| malformed("draw call", format!("no vertex buffer {vertices}")))?
        .count;
    let all = index_lists
        .get(handle("m_indexBuffer"))
        .ok_or_else(|| malformed("draw call", "no index buffer"))?;
    let range = all
        .get(start..start.saturating_add(count))
        .ok_or_else(|| malformed("draw call", "indices past the buffer"))?;
    budget.bytes(range.len() * 4)?;
    let mut indices = Vec::with_capacity(range.len());
    for &i in range {
        let vertex = base.checked_add(i as usize).filter(|&v| v < vertex_count);
        let vertex = vertex.ok_or_else(|| {
            malformed(
                "draw call",
                format!("vertex {i} + {base} of {vertex_count}"),
            )
        })?;
        indices.push(vertex as u32);
    }
    let material = ["m_material", "m_pMaterial"]
        .iter()
        .find_map(|k| call.get(k).and_then(Value::as_str))
        .unwrap_or_default();
    let tint = call.get("m_vTintColor").map(|t| {
        let rgb = array(Some(t));
        let c = |i: usize| finite(rgb.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32);
        let alpha = call.get("m_flAlpha").and_then(Value::as_f64).unwrap_or(1.0);
        [c(0), c(1), c(2), finite(alpha as f32)]
    });
    Ok(Primitive {
        vertices,
        indices,
        material: material.to_owned(),
        tint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        let call = |material: &str| Primitive {
            vertices: 0,
            indices: Vec::new(),
            material: material.to_owned(),
            tint: None,
        };
        Model {
            bones: Vec::new(),
            meshes: vec![Mesh {
                name: "m".to_owned(),
                vertices: Vec::new(),
                primitives: vec![call("a.vmat"), call("kept.vmat")],
                morph: None,
                targets: Vec::new(),
            }],
            material_groups: vec![
                ("default".to_owned(), vec!["a.vmat".to_owned()]),
                ("1".to_owned(), vec!["b.vmat".to_owned()]),
            ],
            attachments: Vec::new(),
        }
    }

    #[test]
    fn a_skin_swaps_the_materials_the_default_skin_lists_and_no_other() {
        let mut model = model();
        model.use_material_group("1").unwrap();
        let used: Vec<&str> = model.meshes[0]
            .primitives
            .iter()
            .map(|p| p.material.as_str())
            .collect();
        assert_eq!(used, ["b.vmat", "kept.vmat"]);
    }

    #[test]
    fn a_skin_the_model_lacks_is_an_error() {
        assert!(model().use_material_group("9").is_err());
    }
}
