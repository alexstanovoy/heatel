//! A model's collision shapes (its `PHYS` block): triangle meshes (a map's ground and walls) and
//! convex hulls (a trigger's volume), each naming what it collides as: its collision group and
//! the layers it interacts as (`water`, `playerclip`, ...).
//!
//! Positions are in the model's frame, in the units the block stores them in (the map's own for a
//! map's world). Every key the reader uses is required: a block without one is an error naming
//! it, except that a hull's corners are `m_VertexPositions` or, in an older block, `m_Vertices`
//! (see [`CornerStore`](crate::physics::CornerStore)). The one optional part is the spheres and capsules
//! ([`OPTIONAL_SHAPES`](crate::physics::OPTIONAL_SHAPES)): absent, there are none; present, they are counted toward
//! [`MAX_SHAPES`](crate::physics::MAX_SHAPES), not read.

use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::resource::Resource;

/// Most vertices all shapes of one model may have together.
pub const MAX_VERTICES: usize = 4 << 20;
/// Most triangles all meshes of one model may have together.
pub const MAX_TRIANGLES: usize = 8 << 20;
/// Most shapes of one model together: meshes, hulls, spheres and capsules.
pub const MAX_SHAPES: usize = 1 << 16;
/// The shape lists a block may leave out, each read as empty when absent.
pub const OPTIONAL_SHAPES: [&str; 2] = ["m_spheres", "m_capsules"];
/// Most collision attributes of one model.
pub const MAX_ATTRIBUTES: usize = 1024;
/// Most layers one collision attribute interacts as.
pub const MAX_LAYERS: usize = 64;

const WHAT: &str = "PHYS";

/// What a shape collides as: its group and the layers it interacts as.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Attributes {
    /// The collision group (`default`, `conditionallysolid`, ...).
    pub group: String,
    /// The layers it interacts as (`water`, `npcclip`, `playerclip`, ...).
    pub interacts_as: Vec<String>,
}

/// A triangle mesh in the model's frame.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Mesh {
    /// What it collides as: an index into [`Physics::attributes`].
    pub attributes: usize,
    /// Its corners, finite.
    pub vertices: Vec<[f32; 3]>,
    /// Its triangles, three indices into `vertices` each (every one in range), in the order the
    /// block stores their corners; no winding is promised.
    pub triangles: Vec<[u32; 3]>,
}

/// A convex hull in the model's frame.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Hull {
    /// What it collides as: an index into [`Physics::attributes`].
    pub attributes: usize,
    /// Its corners, finite, as many as its edges and faces make (Euler's `V - E + F = 2`), each
    /// within the hull's stated bounds.
    pub vertices: Vec<[f32; 3]>,
    /// Where the block kept the corners.
    pub stored: CornerStore,
}

/// Where a hull's corners were kept. Which one a block uses is told by its data: a hull of `V`
/// corners (Euler's formula over its half-edges and faces) keeps them in `m_VertexPositions` when
/// it has that key, and then `m_Vertices` holds one byte a vertex; an older hull without it keeps
/// them in `m_Vertices` itself, `12 V` bytes, or, decoded from older data, `V` three-number
/// arrays. `m_Vertices` of any other size, `V` bytes among them, is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CornerStore {
    /// `m_VertexPositions`, three floats a corner.
    Positions,
    /// `m_Vertices` of an older block, three floats a corner.
    Vertices,
    /// `m_Vertices` decoded from older data, three numbers an array a corner.
    VertexVectors,
}

/// A model's collision shapes.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Physics {
    /// What the shapes collide as, each shape naming one by its index.
    pub attributes: Vec<Attributes>,
    /// Every triangle mesh of every part, in file order.
    pub meshes: Vec<Mesh>,
    /// Every convex hull of every part, in file order.
    pub hulls: Vec<Hull>,
    /// The spheres and capsules, which are not read: a caller that needs every shape refuses a
    /// model with any.
    pub unread: usize,
}

impl Physics {
    /// The collision shapes of `resource` (a `vmdl_c`), none when it has no `PHYS` block; the
    /// block decoded within `budget`, and the shapes charged to it before they are made.
    ///
    /// # Errors
    ///
    /// When the block is present but cannot be decoded, lacks a key the reader uses, a triangle names a vertex its
    /// mesh lacks, a vertex is not finite (NaN and infinities are refused), a shape names a
    /// collision attribute the block lacks, the shapes exceed [`MAX_SHAPES`], [`MAX_VERTICES`],
    /// [`MAX_TRIANGLES`], [`MAX_ATTRIBUTES`] or [`MAX_LAYERS`], or `budget` runs out.
    pub fn read(resource: &Resource, budget: &mut Budget) -> Result<Self> {
        match resource.kv3_in(b"PHYS", budget) {
            None => Ok(Self::default()),
            Some(Ok(document)) => Self::from_document_root(&document.root, budget),
            Some(Err(error)) => Err(error),
        }
    }

    /// The collision shapes of a decoded `PHYS` block's root, charged to `budget`.
    ///
    /// # Errors
    ///
    /// As [`Physics::read`].
    pub fn from_document_root(root: &Value, budget: &mut Budget) -> Result<Self> {
        let attributes = list(root, "m_collisionAttributes", MAX_ATTRIBUTES)?;
        budget.slots::<Attributes>(attributes.len())?;
        let mut physics = Self {
            attributes: attributes
                .iter()
                .map(|a| attributes_of(a, budget))
                .collect::<Result<_>>()?,
            ..Self::default()
        };
        let mut counts = Counts::default();
        for part in list(root, "m_parts", MAX_SHAPES)? {
            let shape = required(part, "m_rnShape")?;
            for key in OPTIONAL_SHAPES {
                if shape.get(key).is_some() {
                    let unread = list(shape, key, MAX_SHAPES)?.len();
                    counts.shapes(unread)?;
                    physics.unread += unread;
                }
            }
            let meshes = list(shape, "m_meshes", MAX_SHAPES)?;
            let hulls = list(shape, "m_hulls", MAX_SHAPES)?;
            counts.shapes(meshes.len() + hulls.len())?;
            budget.slots::<Mesh>(meshes.len())?;
            budget.slots::<Hull>(hulls.len())?;
            for mesh in meshes {
                let body = required(mesh, "m_Mesh")?;
                let vertices = points(required(body, "m_Vertices")?, &mut counts, budget)?;
                let triangles = corners(
                    required(body, "m_Triangles")?,
                    vertices.len(),
                    &mut counts,
                    budget,
                )?;
                physics.meshes.push(Mesh {
                    attributes: attribute_index(mesh, physics.attributes.len())?,
                    vertices,
                    triangles,
                });
            }
            for hull in hulls {
                let attributes = attribute_index(hull, physics.attributes.len())?;
                let (vertices, stored) =
                    hull_corners(required(hull, "m_Hull")?, &mut counts, budget)?;
                physics.hulls.push(Hull {
                    attributes,
                    vertices,
                    stored,
                });
            }
        }
        Ok(physics)
    }
}

/// What the shapes read so far add up to, each checked against its limit before the shape is
/// made.
#[derive(Default)]
struct Counts {
    shapes: usize,
    vertices: usize,
    triangles: usize,
}

impl Counts {
    fn shapes(&mut self, more: usize) -> Result<()> {
        self.shapes = add_within(self.shapes, more, MAX_SHAPES, "shapes")?;
        Ok(())
    }
}

fn add_within(total: usize, more: usize, limit: usize, what: &str) -> Result<usize> {
    match total.checked_add(more).filter(|&sum| sum <= limit) {
        Some(sum) => Ok(sum),
        None => Err(malformed(WHAT, format!("more than {limit} {what}"))),
    }
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .ok_or_else(|| malformed(WHAT, format!("no {key}")))
}

/// The array of `key`, refused when absent, not an array or longer than `max`.
fn list<'a>(value: &'a Value, key: &str, max: usize) -> Result<&'a [Value]> {
    let items = required(value, key)?
        .as_array()
        .ok_or_else(|| malformed(WHAT, format!("{key} is not an array")))?;
    if items.len() > max {
        return Err(malformed(
            WHAT,
            format!("{} {key}, more than {max}", items.len()),
        ));
    }
    Ok(items)
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    required(value, key)?
        .as_str()
        .ok_or_else(|| malformed(WHAT, format!("{key} is not a string")))
}

fn attributes_of(attribute: &Value, budget: &mut Budget) -> Result<Attributes> {
    let group = budget.copy(text(attribute, "m_CollisionGroupString")?)?;
    let layers = list(attribute, "m_InteractAsStrings", MAX_LAYERS)?;
    budget.slots::<String>(layers.len())?;
    let interacts_as = layers
        .iter()
        .map(|layer| {
            let name = layer
                .as_str()
                .ok_or_else(|| malformed(WHAT, "an interaction layer is not a string"))?;
            budget.copy(name)
        })
        .collect::<Result<_>>()?;
    Ok(Attributes {
        group,
        interacts_as,
    })
}

/// A hull's corners and where they were kept ([`CornerStore`]), checked against its edges and
/// faces (their count) and its bounds (each corner within them).
fn hull_corners(
    hull: &Value,
    counts: &mut Counts,
    budget: &mut Budget,
) -> Result<(Vec<[f32; 3]>, CornerStore)> {
    let half_edges = elements(required(hull, "m_Edges")?, 4, "m_Edges")?;
    let faces = elements(required(hull, "m_Faces")?, 1, "m_Faces")?;
    let corners = match half_edges % 2 {
        0 => (half_edges / 2 + 2).checked_sub(faces),
        _ => None,
    }
    .ok_or_else(|| {
        malformed(
            WHAT,
            format!("a hull of {half_edges} half-edges and {faces} faces"),
        )
    })?;
    let (read, stored) = match hull.get("m_VertexPositions") {
        Some(positions) => (points(positions, counts, budget)?, CornerStore::Positions),
        None => {
            let vertices = required(hull, "m_Vertices")?;
            let as_vectors = vertices.as_array().is_some_and(|items| {
                items
                    .first()
                    .is_some_and(|first| first.as_array().is_some())
            });
            let stored = if as_vectors {
                CornerStore::VertexVectors
            } else {
                let bytes = elements(vertices, 1, "m_Vertices")?;
                if bytes != corners * 12 {
                    return Err(malformed(
                        WHAT,
                        format!(
                            "a hull of {corners} corners keeps {bytes} bytes of m_Vertices and no \
                             m_VertexPositions"
                        ),
                    ));
                }
                CornerStore::Vertices
            };
            (points(vertices, counts, budget)?, stored)
        }
    };
    if read.len() != corners {
        return Err(malformed(
            WHAT,
            format!("a hull of {corners} corners has {} positions", read.len()),
        ));
    }
    let bounds = required(hull, "m_Bounds")?;
    let bound = |key| {
        let axes = required(bounds, key)?.as_array().unwrap_or_default();
        if let [x, y, z] = axes
            && let [Some(x), Some(y), Some(z)] = [x, y, z].map(Value::as_f64)
            && [x, y, z].into_iter().all(f64::is_finite)
        {
            return Ok([x, y, z]);
        }
        Err(malformed(WHAT, format!("{key} is not three numbers")))
    };
    let low = bound("m_vMinBounds")?;
    let high = bound("m_vMaxBounds")?;
    if (0..3).any(|axis| low[axis] > high[axis]) {
        return Err(malformed(
            WHAT,
            "a hull's minimum bound exceeds its maximum",
        ));
    }
    let slack =
        std::array::from_fn::<_, 3, _>(|axis| (1e-3 * high[axis] - 1e-3 * low[axis]).max(1e-3));
    for corner in &read {
        let inside = (0..3).all(|axis| {
            let at = f64::from(corner[axis]);
            low[axis] - slack[axis] <= at && at <= high[axis] + slack[axis]
        });
        if !inside {
            return Err(malformed(
                WHAT,
                format!("a hull's corner {corner:?} outside its bounds {low:?}..{high:?}"),
            ));
        }
    }
    Ok((read, stored))
}

/// How many elements of `stride` bytes a list holds: a blob's or a byte array's bytes over the
/// stride, or the items of an array of structs (as data decoded from older blocks has them).
fn elements(value: &Value, stride: usize, key: &str) -> Result<usize> {
    let bytes = match value {
        Value::Blob(blob) => blob.len(),
        Value::Array(items)
            if items.iter().all(|i| i.as_object().is_some()) && !items.is_empty() =>
        {
            return Ok(items.len());
        }
        Value::Array(items) => items.len(),
        _ => {
            return Err(malformed(
                WHAT,
                format!("{key} is neither a list nor bytes"),
            ));
        }
    };
    match bytes % stride {
        0 => Ok(bytes / stride),
        _ => Err(malformed(WHAT, format!("{bytes} bytes of {key}"))),
    }
}

fn attribute_index(shape: &Value, count: usize) -> Result<usize> {
    let index = required(shape, "m_nCollisionAttributeIndex")?
        .as_u64()
        .ok_or_else(|| malformed(WHAT, "m_nCollisionAttributeIndex is not an index"))?;
    match usize::try_from(index).ok().filter(|&i| i < count) {
        Some(index) => Ok(index),
        None => Err(malformed(
            WHAT,
            format!("collision attribute {index} of {count}"),
        )),
    }
}

/// The bytes of geometry: a blob, or an array of byte values. Their count is checked by `check`
/// before an array's bytes are copied (and charged); a blob is borrowed.
fn geometry<'a>(
    value: &'a Value,
    check: impl FnOnce(usize) -> Result<()>,
    budget: &mut Budget,
) -> Result<std::borrow::Cow<'a, [u8]>> {
    match value {
        Value::Blob(blob) => {
            check(blob.len())?;
            Ok(std::borrow::Cow::Borrowed(blob))
        }
        Value::Array(items) => {
            check(items.len())?;
            budget.bytes(items.len())?;
            items
                .iter()
                .map(|item| {
                    item.as_u64()
                        .and_then(|b| u8::try_from(b).ok())
                        .ok_or_else(|| malformed(WHAT, "a byte array holds a non-byte"))
                })
                .collect::<Result<Vec<u8>>>()
                .map(std::borrow::Cow::Owned)
        }
        _ => Err(malformed(WHAT, "geometry that is neither a blob nor bytes")),
    }
}

/// Three little-endian floats each, counted against [`MAX_VERTICES`] and charged before they are
/// made.
fn points(value: &Value, counts: &mut Counts, budget: &mut Budget) -> Result<Vec<[f32; 3]>> {
    if let Some(vectors) = value.as_array().filter(|items| {
        items
            .first()
            .is_some_and(|first| first.as_array().is_some())
    }) {
        return vectors_of(vectors, counts, budget);
    }
    let raw = geometry(
        value,
        |len| {
            if len % 12 != 0 {
                return Err(malformed(WHAT, format!("{len} bytes of vertices")));
            }
            counts.vertices = add_within(counts.vertices, len / 12, MAX_VERTICES, "vertices")?;
            Ok(())
        },
        budget,
    )?;
    budget.slots::<[f32; 3]>(raw.len() / 12)?;
    raw.as_chunks::<12>()
        .0
        .iter()
        .map(|chunk| {
            let point: [f32; 3] = std::array::from_fn(|axis| {
                let at = axis * 4;
                f32::from_le_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]])
            });
            match point.iter().all(|v| v.is_finite()) {
                true => Ok(point),
                false => Err(malformed(WHAT, format!("a vertex at {point:?}"))),
            }
        })
        .collect()
}

/// Positions stored as an array of three-number arrays, as a block decoded from older
/// (NTRO) data has them: counted and charged as [`points`] does.
fn vectors_of(items: &[Value], counts: &mut Counts, budget: &mut Budget) -> Result<Vec<[f32; 3]>> {
    counts.vertices = add_within(counts.vertices, items.len(), MAX_VERTICES, "vertices")?;
    budget.slots::<[f32; 3]>(items.len())?;
    items
        .iter()
        .map(|item| {
            let point = match item.as_array() {
                Some([x, y, z]) => [x, y, z].map(|v| v.as_f64().map(|v| v as f32)),
                _ => return Err(malformed(WHAT, "a vertex that is not three numbers")),
            };
            match point {
                [Some(x), Some(y), Some(z)] if [x, y, z].iter().all(|v| v.is_finite()) => {
                    Ok([x, y, z])
                }
                _ => Err(malformed(WHAT, format!("a vertex at {point:?}"))),
            }
        })
        .collect()
}

/// Three little-endian `u32` vertex indices each, every one below `count`, counted against
/// [`MAX_TRIANGLES`] and charged before they are made.
fn corners(
    value: &Value,
    count: usize,
    counts: &mut Counts,
    budget: &mut Budget,
) -> Result<Vec<[u32; 3]>> {
    let raw = geometry(
        value,
        |len| {
            if len % 12 != 0 {
                return Err(malformed(WHAT, format!("{len} bytes of triangles")));
            }
            counts.triangles = add_within(counts.triangles, len / 12, MAX_TRIANGLES, "triangles")?;
            Ok(())
        },
        budget,
    )?;
    budget.slots::<[u32; 3]>(raw.len() / 12)?;
    raw.as_chunks::<12>()
        .0
        .iter()
        .map(|chunk| {
            let corner: [u32; 3] = std::array::from_fn(|i| {
                let at = i * 4;
                u32::from_le_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]])
            });
            match corner.iter().all(|&i| (i as usize) < count) {
                true => Ok(corner),
                false => Err(malformed(
                    WHAT,
                    format!("a triangle {corner:?} of {count} vertices"),
                )),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv3::Object;

    fn object(members: Vec<(&str, Value)>) -> Value {
        let mut out = Object::with_capacity(members.len());
        for (key, value) in members {
            out.push(key.to_owned(), value);
        }
        Value::Object(out)
    }

    fn floats(values: &[f32]) -> Value {
        Value::Blob(values.iter().flat_map(|v| v.to_le_bytes()).collect())
    }

    fn words(values: &[u32]) -> Value {
        Value::Blob(values.iter().flat_map(|v| v.to_le_bytes()).collect())
    }

    fn strings(values: &[&str]) -> Value {
        Value::Array(values.iter().map(|s| Value::String((*s).into())).collect())
    }

    fn attributes(group: &str, interacts_as: &[&str]) -> Value {
        object(vec![
            ("m_CollisionGroupString", Value::String(group.into())),
            ("m_InteractAsStrings", strings(interacts_as)),
        ])
    }

    fn part(meshes: Vec<Value>, hulls: Vec<Value>) -> Value {
        object(vec![(
            "m_rnShape",
            object(vec![
                ("m_meshes", Value::Array(meshes)),
                ("m_hulls", Value::Array(hulls)),
                ("m_spheres", Value::Array(vec![Value::Null])),
            ]),
        )])
    }

    fn physics_of(parts: Vec<Value>) -> Value {
        object(vec![
            (
                "m_collisionAttributes",
                Value::Array(vec![
                    attributes("default", &[]),
                    attributes("conditionallysolid", &["water"]),
                ]),
            ),
            ("m_parts", Value::Array(parts)),
        ])
    }

    fn physics(meshes: Vec<Value>, hulls: Vec<Value>) -> Value {
        physics_of(vec![part(meshes, hulls)])
    }

    fn mesh_of(attribute: u64, vertices: Value, triangles: Value) -> Value {
        object(vec![
            ("m_nCollisionAttributeIndex", Value::UInt(attribute)),
            (
                "m_Mesh",
                object(vec![("m_Vertices", vertices), ("m_Triangles", triangles)]),
            ),
        ])
    }

    fn mesh(attribute: u64, triangles: &[u32]) -> Value {
        let corners = floats(&[0.0, 0.0, 1.0, 64.0, 0.0, 2.0, 0.0, 64.0, 3.0]);
        mesh_of(attribute, corners, words(triangles))
    }

    /// A hull's own block: `corners` corners by its edges and faces (one face, so `2 (corners -
    /// 1)` half-edges), wide bounds, and `stores` (key and value) holding them.
    fn hull_body(corners: usize, stores: Vec<(&str, Value)>) -> Value {
        let vector = |v: f64| Value::Array(vec![Value::Float(v); 3]);
        let mut members = vec![
            (
                "m_Edges",
                Value::Blob(vec![0; 8 * corners.saturating_sub(1)]),
            ),
            ("m_Faces", Value::Blob(vec![0])),
            (
                "m_Bounds",
                object(vec![
                    ("m_vMinBounds", vector(-1e6)),
                    ("m_vMaxBounds", vector(1e6)),
                ]),
            ),
        ];
        members.extend(stores);
        object(members)
    }

    /// A hull of the given corner store, its corner count read off the store.
    fn hull_with(stores: Vec<(&str, Value)>) -> Value {
        let corners = match &stores.last().expect("a store").1 {
            Value::Blob(bytes) => bytes.len() / 12,
            Value::Array(items) if items.first().is_some_and(|i| i.as_array().is_some()) => {
                items.len()
            }
            Value::Array(items) => items.len() / 12,
            _ => 1,
        };
        object(vec![
            ("m_nCollisionAttributeIndex", Value::UInt(0)),
            ("m_Hull", hull_body(corners.max(1), stores)),
        ])
    }

    fn hull(positions: Value) -> Value {
        hull_with(vec![("m_VertexPositions", positions)])
    }

    fn vector(v: [f64; 3]) -> Value {
        Value::Array(v.iter().map(|&c| Value::Float(c)).collect())
    }

    fn read(root: &Value) -> Result<Physics> {
        Physics::from_document_root(root, &mut Budget::default())
    }

    fn refused(root: &Value, message: &str) {
        let err = read(root).unwrap_err().to_string();
        assert!(err.contains(message), "{err}");
    }

    /// `value` without its member `key`.
    fn without(value: &Value, key: &str) -> Value {
        let Value::Object(members) = value else {
            panic!("not an object")
        };
        let mut out = Object::with_capacity(members.len());
        for (k, v) in members.iter().filter(|(k, _)| *k != key) {
            out.push(k.to_owned(), v.clone());
        }
        Value::Object(out)
    }

    fn hull_obj(body: Value) -> Value {
        object(vec![
            ("m_nCollisionAttributeIndex", Value::UInt(0)),
            ("m_Hull", body),
        ])
    }

    fn attr_root(attrs: Vec<Value>) -> Value {
        object(vec![
            ("m_collisionAttributes", Value::Array(attrs)),
            ("m_parts", Value::Array(vec![])),
        ])
    }

    fn shape_root(shape: Value) -> Value {
        physics_of(vec![object(vec![("m_rnShape", shape)])])
    }

    fn one_hull(h: Value) -> Value {
        physics(vec![], vec![h])
    }

    fn one_mesh(m: Value) -> Value {
        physics(vec![m], vec![])
    }

    fn hull_verts(v: Value) -> Value {
        one_hull(hull_with(vec![("m_Vertices", v)]))
    }

    #[test]
    fn meshes_and_hulls_name_what_they_collide_as() {
        let read = read(&physics(
            vec![mesh(1, &[0, 1, 2])],
            vec![hull(floats(&[1.0, 2.0, 3.0]))],
        ))
        .unwrap();
        assert_eq!(read.attributes[1].interacts_as, ["water"]);
        assert_eq!(read.meshes[0].attributes, 1);
        assert_eq!(read.meshes[0].vertices[2], [0.0, 64.0, 3.0]);
        assert_eq!(read.meshes[0].triangles, [[0, 1, 2]]);
        assert_eq!(read.hulls[0].attributes, 0);
        assert_eq!(read.attributes[read.hulls[0].attributes].group, "default");
        assert_eq!(read.hulls[0].vertices, [[1.0, 2.0, 3.0]]);
        assert_eq!(read.unread, 1);
    }

    #[test]
    fn bad_mesh_indices_attributes_cuts_and_nonfinite_vertices_are_errors() {
        let cut = mesh_of(0, floats(&[0.0; 9]), Value::Blob(vec![0; 8]));
        for (root, msg) in [
            (
                one_mesh(mesh(0, &[0, 1, 3])),
                "a triangle [0, 1, 3] of 3 vertices",
            ),
            (one_mesh(mesh(2, &[0, 1, 2])), "collision attribute 2 of 2"),
            (one_hull(hull(floats(&[1.0, 2.0]))), "8 bytes of vertices"),
            (one_mesh(cut), "8 bytes of triangles"),
        ] {
            refused(&root, msg);
        }
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            refused(&one_hull(hull(floats(&[1.0, bad, 3.0]))), "a vertex at");
        }
    }

    #[test]
    fn geometry_as_bytes_is_read_and_anything_else_is_an_error() {
        let bytes = |values: &[u64]| Value::Array(values.iter().map(|&b| Value::UInt(b)).collect());
        let corner: Vec<u64> = 1.5f32.to_le_bytes().iter().map(|&b| u64::from(b)).collect();
        let point = bytes(&[corner.clone(), corner.clone(), corner].concat());
        let read = read(&one_hull(hull(point))).unwrap();
        assert_eq!(read.hulls[0].vertices, [[1.5; 3]]);
        refused(
            &one_hull(hull(bytes(&[256; 12]))),
            "a byte array holds a non-byte",
        );
        refused(
            &one_hull(hull(Value::Int(1))),
            "geometry that is neither a blob nor bytes",
        );
    }

    #[test]
    fn every_key_the_reader_uses_is_required() {
        let whole = physics(vec![mesh(0, &[0, 1, 2])], vec![hull(floats(&[1.0; 3]))]);
        for key in ["m_collisionAttributes", "m_parts"] {
            refused(&without(&whole, key), &format!("no {key}"));
        }
        refused(&physics_of(vec![object(vec![])]), "no m_rnShape");
        for key in ["m_meshes", "m_hulls"] {
            let shape = without(part(vec![], vec![]).get("m_rnShape").unwrap(), key);
            refused(&shape_root(shape), &format!("no {key}"));
        }
        for key in ["m_Mesh", "m_nCollisionAttributeIndex"] {
            refused(
                &one_mesh(without(&mesh(0, &[0, 1, 2]), key)),
                &format!("no {key}"),
            );
        }
        for key in ["m_Vertices", "m_Triangles"] {
            let body = without(mesh(0, &[0, 1, 2]).get("m_Mesh").unwrap(), key);
            let m = object(vec![
                ("m_nCollisionAttributeIndex", Value::UInt(0)),
                ("m_Mesh", body),
            ]);
            refused(&one_mesh(m), &format!("no {key}"));
        }
        refused(&one_hull(hull_obj(hull_body(1, vec![]))), "no m_Vertices");
        let body = hull_body(1, vec![("m_VertexPositions", floats(&[1.0; 3]))]);
        for key in ["m_Edges", "m_Faces", "m_Bounds"] {
            refused(
                &one_hull(hull_obj(without(&body, key))),
                &format!("no {key}"),
            );
        }
        for key in ["m_Hull", "m_nCollisionAttributeIndex"] {
            refused(
                &one_hull(without(&hull(floats(&[1.0; 3])), key)),
                &format!("no {key}"),
            );
        }
        for key in ["m_CollisionGroupString", "m_InteractAsStrings"] {
            refused(
                &attr_root(vec![without(&attributes("default", &[]), key)]),
                &format!("no {key}"),
            );
        }
    }

    #[test]
    fn shapes_vertices_triangles_attributes_layers_and_parts_beyond_limits_are_refused() {
        let nulls = |n| Value::Array(vec![Value::Null; n]);
        let shape_many = object(vec![
            ("m_meshes", nulls(MAX_SHAPES + 1)),
            ("m_hulls", nulls(0)),
        ]);
        let shape_split = object(vec![
            ("m_meshes", nulls(MAX_SHAPES / 2 + 1)),
            ("m_hulls", nulls(MAX_SHAPES / 2 + 1)),
        ]);
        let shape_caps = object(vec![
            ("m_meshes", nulls(0)),
            ("m_hulls", nulls(MAX_SHAPES / 2)),
            ("m_capsules", nulls(MAX_SHAPES / 2 + 1)),
        ]);
        let layers = vec!["water"; MAX_LAYERS + 1];
        for (root, msg) in [
            (shape_root(shape_many), "65537 m_meshes, more than 65536"),
            (shape_root(shape_split), "more than 65536 shapes"),
            (shape_root(shape_caps), "more than 65536 shapes"),
            (
                physics_of(vec![Value::Null; MAX_SHAPES + 1]),
                "65537 m_parts, more than 65536",
            ),
            (
                one_mesh(mesh_of(
                    0,
                    Value::Blob(vec![0; (MAX_VERTICES + 1) * 12]),
                    words(&[]),
                )),
                "more than 4194304 vertices",
            ),
            (
                one_mesh(mesh_of(
                    0,
                    floats(&[0.0; 3]),
                    Value::Blob(vec![0; (MAX_TRIANGLES + 1) * 12]),
                )),
                "more than 8388608 triangles",
            ),
            (
                attr_root(vec![attributes("default", &[]); MAX_ATTRIBUTES + 1]),
                "1025 m_collisionAttributes, more than 1024",
            ),
            (
                attr_root(vec![attributes("default", &layers)]),
                "65 m_InteractAsStrings, more than 64",
            ),
        ] {
            refused(&root, msg);
        }
        let bare = object(vec![("m_meshes", nulls(0)), ("m_hulls", nulls(0))]);
        assert_eq!(read(&shape_root(bare)).unwrap().unread, 0);
    }

    #[test]
    fn shapes_sharing_a_long_attribute_copy_it_once_and_a_small_budget_runs_out() {
        let layer = "x".repeat(1000);
        let layers = vec![layer.as_str(); MAX_LAYERS];
        let hulls = vec![hull(floats(&[1.0; 3])); 1000];
        let root = object(vec![
            (
                "m_collisionAttributes",
                Value::Array(vec![attributes("default", &layers)]),
            ),
            ("m_parts", Value::Array(vec![part(vec![], hulls)])),
        ]);
        let mut budget = Budget::new(1 << 20);
        let read = Physics::from_document_root(&root, &mut budget).unwrap();
        assert_eq!(read.attributes.len(), 1);
        assert!(budget.spent() < 200_000, "{} bytes", budget.spent());
        let err = Physics::from_document_root(&root, &mut Budget::new(10_000)).unwrap_err();
        assert!(err.to_string().contains("more than it may"), "{err}");
    }

    #[test]
    fn a_resource_without_a_collision_block_has_no_shapes() {
        let mut bytes = 16u32.to_le_bytes().to_vec();
        bytes.extend(12u16.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(8u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        let resource = Resource::parse(&bytes).unwrap();
        assert_eq!(
            Physics::read(&resource, &mut Budget::default()).unwrap(),
            Physics::default()
        );
    }

    #[test]
    fn hull_corner_stores_and_validation() {
        let r = read(&hull_verts(floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]))).unwrap();
        assert_eq!(r.hulls[0].vertices, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(r.hulls[0].stored, CornerStore::Vertices);

        let newer = hull_with(vec![
            ("m_Vertices", Value::Blob(vec![0])),
            ("m_VertexPositions", floats(&[4.0, 5.0, 6.0])),
        ]);
        let r = read(&one_hull(newer)).unwrap();
        assert_eq!(r.hulls[0].vertices, [[4.0, 5.0, 6.0]]);
        assert_eq!(r.hulls[0].stored, CornerStore::Positions);

        let odd = hull_obj(object(vec![
            ("m_Edges", Value::Blob(vec![0; 4])),
            ("m_Faces", Value::Blob(vec![0])),
            ("m_VertexPositions", floats(&[1.0; 3])),
        ]));
        for (h, msg) in [
            (
                hull_obj(hull_body(
                    12,
                    vec![("m_Vertices", Value::Blob(vec![0; 12]))],
                )),
                "a hull of 12 corners keeps 12 bytes of m_Vertices and no m_VertexPositions",
            ),
            (
                hull_with(vec![
                    ("m_Vertices", floats(&[1.0, 2.0, 3.0])),
                    ("m_VertexPositions", Value::Int(7)),
                ]),
                "geometry that is neither a blob nor bytes",
            ),
            (
                hull_obj(hull_body(1, vec![("m_Vertices", Value::Int(7))])),
                "m_Vertices is neither a list nor bytes",
            ),
            (
                hull_obj(hull_body(2, vec![("m_VertexPositions", floats(&[1.0; 3]))])),
                "a hull of 2 corners has 1 positions",
            ),
            (odd, "a hull of 1 half-edges and 1 faces"),
            (hull(floats(&[1.0, 2.0, 3e6])), "outside its bounds"),
        ] {
            refused(&one_hull(h), msg);
        }

        let with_bounds = |low, high| {
            one_hull(hull_obj(object(vec![
                ("m_Edges", Value::Blob(vec![])),
                ("m_Faces", Value::Blob(vec![0])),
                ("m_VertexPositions", floats(&[0.0; 3])),
                (
                    "m_Bounds",
                    object(vec![("m_vMinBounds", low), ("m_vMaxBounds", high)]),
                ),
            ])))
        };
        let four = Value::Array(vec![
            Value::Null,
            Value::Float(-1.0),
            Value::Float(-1.0),
            Value::Float(-1.0),
        ]);
        for low in [four, vector([f64::NEG_INFINITY, -1.0, -1.0])] {
            refused(
                &with_bounds(low, vector([1.0; 3])),
                "m_vMinBounds is not three numbers",
            );
        }
        refused(
            &with_bounds(vector([1.0; 3]), vector([0.0; 3])),
            "a hull's minimum bound exceeds its maximum",
        );
    }

    #[test]
    fn corners_stored_as_vectors_are_read_and_checked() {
        let corners = Value::Array(vec![vector([1.0, 2.0, 3.0]), vector([4.0, 5.0, 6.0])]);
        let r = read(&hull_verts(corners)).unwrap();
        assert_eq!(r.hulls[0].vertices, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(r.hulls[0].stored, CornerStore::VertexVectors);
        for n in [1, 2, 4] {
            let bad = Value::Array(vec![Value::Array(vec![Value::Float(1.0); n])]);
            refused(&hull_verts(bad), "a vertex that is not three numbers");
        }
        let text = Value::Array(vec![Value::Array(vec![
            Value::Float(1.0),
            Value::String("2".into()),
            Value::Float(3.0),
        ])]);
        refused(
            &hull_verts(text),
            "a vertex at [Some(1.0), None, Some(3.0)]",
        );
        for bad in [f64::INFINITY, f64::NAN, 1e300] {
            refused(
                &hull_verts(Value::Array(vec![vector([1.0, bad, 3.0])])),
                "a vertex at",
            );
        }
        let mixed = Value::Array(vec![vector([1.0, 2.0, 3.0]), Value::Float(4.0)]);
        refused(&hull_verts(mixed), "a vertex that is not three numbers");
    }

    #[test]
    fn corners_and_shapes_count_toward_limits_and_budget() {
        let filled = mesh_of(0, Value::Blob(vec![0; MAX_VERTICES * 12]), words(&[]));
        for store in [
            floats(&[1.0; 3]),
            Value::Array(vec![vector([1.0, 2.0, 3.0])]),
        ] {
            refused(
                &physics(
                    vec![filled.clone()],
                    vec![hull_with(vec![("m_Vertices", store)])],
                ),
                "more than 4194304 vertices",
            );
        }
        let charged = |root: &Value| {
            let mut budget = Budget::default();
            Physics::from_document_root(root, &mut budget).unwrap();
            budget.spent()
        };
        let one_vec = charged(&hull_verts(Value::Array(vec![vector([1.0, 2.0, 3.0])])));
        let two_vec = charged(&hull_verts(Value::Array(vec![vector([1.0, 2.0, 3.0]); 2])));
        assert_eq!(two_vec - one_vec, size_of::<[f32; 3]>());

        let none = charged(&physics(vec![], vec![]));
        let one = charged(&one_hull(hull(floats(&[1.0; 3]))));
        let two = charged(&physics(vec![], vec![hull(floats(&[1.0; 3])); 2]));
        let size = size_of::<Hull>() + size_of::<[f32; 3]>();
        assert_eq!(one - none, size);
        assert_eq!(two - one, size);
        let as_bytes = charged(&one_hull(hull(Value::Array(vec![Value::UInt(0); 12]))));
        assert_eq!(as_bytes - one, 12, "the copied bytes are charged too");
    }

    #[test]
    fn a_key_of_the_wrong_type_is_an_error_naming_it() {
        let with = |value: &Value, key: &str, replacement: Value| {
            let mut out = without(value, key);
            let Value::Object(members) = &mut out else {
                unreachable!()
            };
            members.push(key.to_owned(), replacement);
            out
        };
        let shape_bad_spheres = object(vec![
            ("m_meshes", Value::Array(vec![])),
            ("m_hulls", Value::Array(vec![])),
            ("m_spheres", Value::Int(3)),
        ]);
        for (root, msg) in [
            (
                with(&physics(vec![], vec![]), "m_parts", Value::Int(1)),
                "m_parts is not an array",
            ),
            (
                attr_root(vec![with(
                    &attributes("default", &[]),
                    "m_CollisionGroupString",
                    Value::Int(1),
                )]),
                "m_CollisionGroupString is not a string",
            ),
            (
                attr_root(vec![with(
                    &attributes("default", &[]),
                    "m_InteractAsStrings",
                    Value::Array(vec![Value::Int(1)]),
                )]),
                "an interaction layer is not a string",
            ),
            (
                one_hull(with(
                    &hull(floats(&[1.0; 3])),
                    "m_nCollisionAttributeIndex",
                    Value::Int(-1),
                )),
                "m_nCollisionAttributeIndex is not an index",
            ),
            (shape_root(shape_bad_spheres), "m_spheres is not an array"),
        ] {
            refused(&root, msg);
        }
    }
}
