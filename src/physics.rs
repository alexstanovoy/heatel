//! A model's collision shapes (its `PHYS` block): triangle meshes (a map's ground and walls) and
//! convex hulls (a trigger's volume), each naming what it collides as: its collision group and
//! the layers it interacts as (`water`, `playerclip`, ...).
//!
//! Positions are in the model's frame, in world units (inches), as the block stores them. Every
//! key below that the reader uses is required: a block without one is an error naming it. The
//! one optional part is the spheres and capsules, which are counted, not read.

use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::resource::Resource;

/// Most vertices all shapes of one model may have together; the main Dota map's ground has some
/// 140,000.
pub const MAX_VERTICES: usize = 4 << 20;
/// Most triangles all meshes of one model may have together; the main Dota map's ground has
/// some 290,000.
pub const MAX_TRIANGLES: usize = 8 << 20;
/// Most meshes and hulls of one model together.
pub const MAX_SHAPES: usize = 1 << 16;
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
    /// Its corners, finite.
    pub vertices: Vec<[f32; 3]>,
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
    /// When the block is malformed or lacks a key the reader uses, a triangle names a vertex its
    /// mesh lacks, a vertex is not finite (NaN and infinities are refused), a shape names a
    /// collision attribute the block lacks, the shapes exceed [`MAX_SHAPES`], [`MAX_VERTICES`],
    /// [`MAX_TRIANGLES`], [`MAX_ATTRIBUTES`] or [`MAX_LAYERS`], or `budget` runs out.
    pub fn read(resource: &Resource, budget: &mut Budget) -> Result<Self> {
        match resource.kv3_in(b"PHYS", budget) {
            None => Ok(Self::default()),
            Some(document) => Self::from_document_root(&document?.root, budget),
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
            for key in ["m_spheres", "m_capsules"] {
                if let Some(value) = shape.get(key) {
                    let items = value
                        .as_array()
                        .ok_or_else(|| malformed(WHAT, format!("{key} is not an array")))?;
                    physics.unread = physics.unread.saturating_add(items.len());
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
                let positions = required(required(hull, "m_Hull")?, "m_VertexPositions")?;
                physics.hulls.push(Hull {
                    attributes: attribute_index(hull, physics.attributes.len())?,
                    vertices: points(positions, &mut counts, budget)?,
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

/// The bytes of geometry: a blob, or an array of byte values (as the JSON-like form of a block
/// stores them). Borrowed when a blob; an array's copy is charged first.
fn geometry<'a>(value: &'a Value, budget: &mut Budget) -> Result<std::borrow::Cow<'a, [u8]>> {
    match value {
        Value::Blob(blob) => Ok(std::borrow::Cow::Borrowed(blob)),
        Value::Array(items) => {
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
    let raw = geometry(value, budget)?;
    if raw.len() % 12 != 0 {
        return Err(malformed(WHAT, format!("{} bytes of vertices", raw.len())));
    }
    counts.vertices = add_within(counts.vertices, raw.len() / 12, MAX_VERTICES, "vertices")?;
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

/// Three little-endian `u32` vertex indices each, every one below `count`, counted against
/// [`MAX_TRIANGLES`] and charged before they are made.
fn corners(
    value: &Value,
    count: usize,
    counts: &mut Counts,
    budget: &mut Budget,
) -> Result<Vec<[u32; 3]>> {
    let raw = geometry(value, budget)?;
    if raw.len() % 12 != 0 {
        return Err(malformed(WHAT, format!("{} bytes of triangles", raw.len())));
    }
    counts.triangles = add_within(counts.triangles, raw.len() / 12, MAX_TRIANGLES, "triangles")?;
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

    fn hull(positions: Value) -> Value {
        object(vec![
            ("m_nCollisionAttributeIndex", Value::UInt(0)),
            ("m_Hull", object(vec![("m_VertexPositions", positions)])),
        ])
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
    fn a_triangle_naming_a_missing_vertex_is_an_error() {
        refused(
            &physics(vec![mesh(0, &[0, 1, 3])], vec![]),
            "a triangle [0, 1, 3] of 3 vertices",
        );
    }

    #[test]
    fn a_shape_naming_a_missing_collision_attribute_is_an_error() {
        refused(
            &physics(vec![mesh(2, &[0, 1, 2])], vec![]),
            "collision attribute 2 of 2",
        );
    }

    #[test]
    fn geometry_cut_mid_vertex_or_mid_triangle_is_an_error() {
        refused(
            &physics(vec![], vec![hull(floats(&[1.0, 2.0]))]),
            "8 bytes of vertices",
        );
        let cut = mesh_of(0, floats(&[0.0; 9]), Value::Blob(vec![0; 8]));
        refused(&physics(vec![cut], vec![]), "8 bytes of triangles");
    }

    #[test]
    fn a_vertex_that_is_not_finite_is_an_error() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            refused(
                &physics(vec![], vec![hull(floats(&[1.0, bad, 3.0]))]),
                "a vertex at",
            );
        }
    }

    #[test]
    fn geometry_as_bytes_is_read_and_anything_else_is_an_error() {
        let bytes = |values: &[u64]| Value::Array(values.iter().map(|&b| Value::UInt(b)).collect());
        let corner: Vec<u64> = 1.5f32.to_le_bytes().iter().map(|&b| u64::from(b)).collect();
        let point = bytes(&[corner.clone(), corner.clone(), corner].concat());
        let read = read(&physics(vec![], vec![hull(point)])).unwrap();
        assert_eq!(read.hulls[0].vertices, [[1.5; 3]]);
        refused(
            &physics(vec![], vec![hull(bytes(&[256; 12]))]),
            "a byte array holds a non-byte",
        );
        refused(
            &physics(vec![], vec![hull(Value::Int(1))]),
            "geometry that is neither a blob nor bytes",
        );
    }

    #[test]
    fn every_key_the_reader_uses_is_required() {
        let whole = physics(vec![mesh(0, &[0, 1, 2])], vec![hull(floats(&[1.0; 3]))]);
        for key in ["m_collisionAttributes", "m_parts"] {
            refused(&without(&whole, key), &format!("no {key}"));
        }
        let shape_without = |key: &str| {
            let shape = without(part(vec![], vec![]).get("m_rnShape").unwrap(), key);
            physics_of(vec![object(vec![("m_rnShape", shape)])])
        };
        refused(&physics_of(vec![object(vec![])]), "no m_rnShape");
        refused(&shape_without("m_meshes"), "no m_meshes");
        refused(&shape_without("m_hulls"), "no m_hulls");
        refused(
            &physics(vec![without(&mesh(0, &[0, 1, 2]), "m_Mesh")], vec![]),
            "no m_Mesh",
        );
        refused(
            &physics(
                vec![without(&mesh(0, &[0, 1, 2]), "m_nCollisionAttributeIndex")],
                vec![],
            ),
            "no m_nCollisionAttributeIndex",
        );
        let body_without = |key: &str| {
            let body = without(mesh(0, &[0, 1, 2]).get("m_Mesh").unwrap(), key);
            object(vec![
                ("m_nCollisionAttributeIndex", Value::UInt(0)),
                ("m_Mesh", body),
            ])
        };
        for key in ["m_Vertices", "m_Triangles"] {
            refused(
                &physics(vec![body_without(key)], vec![]),
                &format!("no {key}"),
            );
        }
        let bare_hull = object(vec![
            ("m_nCollisionAttributeIndex", Value::UInt(0)),
            ("m_Hull", object(vec![])),
        ]);
        refused(&physics(vec![], vec![bare_hull]), "no m_VertexPositions");
        let bare_attribute = object(vec![("m_InteractAsStrings", strings(&[]))]);
        let root = object(vec![
            ("m_collisionAttributes", Value::Array(vec![bare_attribute])),
            ("m_parts", Value::Array(vec![])),
        ]);
        refused(&root, "no m_CollisionGroupString");
    }

    #[test]
    fn shapes_beyond_the_limit_are_refused_before_any_is_read() {
        let many = Value::Array(vec![Value::Null; MAX_SHAPES + 1]);
        let shape = object(vec![("m_meshes", many), ("m_hulls", Value::Array(vec![]))]);
        refused(
            &physics_of(vec![object(vec![("m_rnShape", shape)])]),
            "65537 m_meshes, more than 65536",
        );
        let shape = object(vec![
            (
                "m_meshes",
                Value::Array(vec![Value::Null; MAX_SHAPES / 2 + 1]),
            ),
            (
                "m_hulls",
                Value::Array(vec![Value::Null; MAX_SHAPES / 2 + 1]),
            ),
        ]);
        refused(
            &physics_of(vec![object(vec![("m_rnShape", shape)])]),
            "more than 65536 shapes",
        );
    }

    #[test]
    fn vertices_and_triangles_beyond_the_limits_are_refused_before_they_are_made() {
        let vertices = Value::Blob(vec![0; (MAX_VERTICES + 1) * 12]);
        refused(
            &physics(vec![], vec![hull(vertices)]),
            "more than 4194304 vertices",
        );
        let triangles = Value::Blob(vec![0; (MAX_TRIANGLES + 1) * 12]);
        refused(
            &physics(vec![mesh_of(0, floats(&[0.0; 3]), triangles)], vec![]),
            "more than 8388608 triangles",
        );
    }

    #[test]
    fn attributes_and_layers_beyond_the_limits_are_refused() {
        let root = object(vec![
            (
                "m_collisionAttributes",
                Value::Array(vec![attributes("default", &[]); MAX_ATTRIBUTES + 1]),
            ),
            ("m_parts", Value::Array(vec![])),
        ]);
        refused(&root, "1025 m_collisionAttributes, more than 1024");
        let layers = vec!["water"; MAX_LAYERS + 1];
        let root = object(vec![
            (
                "m_collisionAttributes",
                Value::Array(vec![attributes("default", &layers)]),
            ),
            ("m_parts", Value::Array(vec![])),
        ]);
        refused(&root, "65 m_InteractAsStrings, more than 64");
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
}
