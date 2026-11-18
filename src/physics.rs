//! A model's collision shapes (its `PHYS` block): triangle meshes (a map's ground and walls) and
//! convex hulls (a trigger's volume), each with the collision group it is in and what it
//! interacts as (`water`, `playerclip`, ...).

use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::resource::Resource;

/// Most vertices all shapes of one model may have together; a Dota map's ground has ~150,000.
const MAX_VERTICES: usize = 4 << 20;
/// Most triangles all meshes of one model may have together; a Dota map's ground has ~300,000.
const MAX_TRIANGLES: usize = 8 << 20;
/// Most shapes of one model.
const MAX_SHAPES: usize = 1 << 16;

/// What a shape collides as: its group and the layers it interacts as.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attributes {
    /// The collision group (`default`, `conditionallysolid`, ...).
    pub group: String,
    /// The layers it interacts as (`water`, `npcclip`, `playerclip`, ...).
    pub interacts_as: Vec<String>,
}

/// A triangle mesh in the model's frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    /// What it collides as.
    pub attributes: Attributes,
    /// Its corners.
    pub vertices: Vec<[f32; 3]>,
    /// Its triangles, three indices into `vertices` each (every one in range).
    pub triangles: Vec<[u32; 3]>,
}

/// A convex hull in the model's frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    /// What it collides as.
    pub attributes: Attributes,
    /// Its corners.
    pub vertices: Vec<[f32; 3]>,
}

/// A model's collision shapes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Physics {
    /// Every triangle mesh of every part, in file order.
    pub meshes: Vec<Mesh>,
    /// Every convex hull of every part, in file order.
    pub hulls: Vec<Hull>,
    /// The spheres and capsules, which are not read: a caller that needs every shape refuses a
    /// model with any.
    pub unread: usize,
}

impl Physics {
    /// The collision shapes of `resource` (a `vmdl_c`), none when it has no `PHYS` block.
    ///
    /// # Errors
    ///
    /// When the block is malformed, a triangle names a vertex the mesh lacks, or the shapes are
    /// larger than any model's.
    pub fn read(resource: &Resource) -> Result<Self> {
        match resource.kv3(b"PHYS") {
            None => Ok(Self::default()),
            Some(document) => Self::from_value(&document?.root),
        }
    }

    /// The collision shapes of a decoded `PHYS` block.
    ///
    /// # Errors
    ///
    /// As [`Physics::read`].
    pub fn from_value(root: &Value) -> Result<Self> {
        let attributes: Vec<Attributes> = list(root, "m_collisionAttributes")
            .iter()
            .map(|a| Attributes {
                group: text(a, "m_CollisionGroupString"),
                interacts_as: list(a, "m_InteractAsStrings")
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            })
            .collect();
        let attributes_of = |shape: &Value| -> Result<Attributes> {
            let index = shape
                .get("m_nCollisionAttributeIndex")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            match attributes.get(usize::try_from(index).unwrap_or(usize::MAX)) {
                Some(found) => Ok(found.clone()),
                None if attributes.is_empty() => Ok(Attributes::default()),
                None => Err(malformed("PHYS", format!("collision attribute {index}"))),
            }
        };
        let mut physics = Self::default();
        let mut vertices = 0;
        let mut triangles = 0;
        for part in list(root, "m_parts") {
            let shape = part
                .get("m_rnShape")
                .ok_or_else(|| malformed("PHYS", "a part without a shape"))?;
            physics.unread += list(shape, "m_spheres").len() + list(shape, "m_capsules").len();
            for mesh in list(shape, "m_meshes") {
                let body = mesh
                    .get("m_Mesh")
                    .ok_or_else(|| malformed("PHYS", "a mesh without its body"))?;
                let points = points(body.get("m_Vertices"), &mut vertices)?;
                let corners = indices(body.get("m_Triangles"), points.len(), &mut triangles)?;
                physics.meshes.push(Mesh {
                    attributes: attributes_of(mesh)?,
                    vertices: points,
                    triangles: corners,
                });
            }
            for hull in list(shape, "m_hulls") {
                let positions = hull.get("m_Hull").and_then(|h| h.get("m_VertexPositions"));
                physics.hulls.push(Hull {
                    attributes: attributes_of(hull)?,
                    vertices: points(positions, &mut vertices)?,
                });
            }
            if physics.meshes.len() + physics.hulls.len() > MAX_SHAPES {
                return Err(malformed("PHYS", "more shapes than any model has"));
            }
        }
        Ok(physics)
    }
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).unwrap_or_default()
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Bytes of a blob, or of an array of byte values (as an older block stores them).
fn bytes(value: Option<&Value>) -> Result<Vec<u8>> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Blob(blob)) => Ok(blob.clone()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_u64()
                    .and_then(|b| u8::try_from(b).ok())
                    .ok_or_else(|| malformed("PHYS", "a byte array holds a non-byte"))
            })
            .collect(),
        Some(_) => Err(malformed(
            "PHYS",
            "geometry that is neither a blob nor bytes",
        )),
    }
}

/// Three little-endian floats each, counted against the model's vertex total.
fn points(value: Option<&Value>, total: &mut usize) -> Result<Vec<[f32; 3]>> {
    let raw = bytes(value)?;
    if raw.len() % 12 != 0 {
        return Err(malformed(
            "PHYS",
            format!("{} bytes of vertices", raw.len()),
        ));
    }
    *total += raw.len() / 12;
    if *total > MAX_VERTICES {
        return Err(malformed("PHYS", "more vertices than any model has"));
    }
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
                false => Err(malformed("PHYS", format!("a vertex at {point:?}"))),
            }
        })
        .collect()
}

/// Three little-endian `u32` vertex indices each, every one below `count`.
fn indices(value: Option<&Value>, count: usize, total: &mut usize) -> Result<Vec<[u32; 3]>> {
    let raw = bytes(value)?;
    if raw.len() % 12 != 0 {
        return Err(malformed(
            "PHYS",
            format!("{} bytes of triangles", raw.len()),
        ));
    }
    *total += raw.len() / 12;
    if *total > MAX_TRIANGLES {
        return Err(malformed("PHYS", "more triangles than any model has"));
    }
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
                    "PHYS",
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

    fn attributes(group: &str, interacts_as: &[&str]) -> Value {
        object(vec![
            ("m_CollisionGroupString", Value::String(group.into())),
            (
                "m_InteractAsStrings",
                Value::Array(
                    interacts_as
                        .iter()
                        .map(|s| Value::String((*s).into()))
                        .collect(),
                ),
            ),
        ])
    }

    fn physics(meshes: Vec<Value>, hulls: Vec<Value>) -> Value {
        let shape = object(vec![
            ("m_meshes", Value::Array(meshes)),
            ("m_hulls", Value::Array(hulls)),
            ("m_spheres", Value::Array(vec![Value::Null])),
        ]);
        object(vec![
            (
                "m_collisionAttributes",
                Value::Array(vec![
                    attributes("default", &[]),
                    attributes("conditionallysolid", &["water"]),
                ]),
            ),
            (
                "m_parts",
                Value::Array(vec![object(vec![("m_rnShape", shape)])]),
            ),
        ])
    }

    fn mesh(attribute: u64, triangles: &[u32]) -> Value {
        object(vec![
            ("m_nCollisionAttributeIndex", Value::UInt(attribute)),
            (
                "m_Mesh",
                object(vec![
                    (
                        "m_Vertices",
                        floats(&[0.0, 0.0, 1.0, 64.0, 0.0, 2.0, 0.0, 64.0, 3.0]),
                    ),
                    ("m_Triangles", words(triangles)),
                ]),
            ),
        ])
    }

    #[test]
    fn meshes_and_hulls_come_with_what_they_collide_as() {
        let hull = object(vec![
            ("m_nCollisionAttributeIndex", Value::UInt(0)),
            (
                "m_Hull",
                object(vec![("m_VertexPositions", floats(&[1.0, 2.0, 3.0]))]),
            ),
        ]);
        let read = Physics::from_value(&physics(vec![mesh(1, &[0, 1, 2])], vec![hull])).unwrap();
        assert_eq!(read.meshes.len(), 1);
        assert_eq!(read.meshes[0].attributes.interacts_as, ["water"]);
        assert_eq!(read.meshes[0].vertices[2], [0.0, 64.0, 3.0]);
        assert_eq!(read.meshes[0].triangles, [[0, 1, 2]]);
        assert_eq!(read.hulls[0].attributes.group, "default");
        assert_eq!(read.hulls[0].vertices, [[1.0, 2.0, 3.0]]);
        assert_eq!(read.unread, 1);
    }

    #[test]
    fn a_triangle_naming_a_missing_vertex_is_an_error() {
        let err = Physics::from_value(&physics(vec![mesh(0, &[0, 1, 3])], vec![])).unwrap_err();
        assert!(
            err.to_string()
                .contains("a triangle [0, 1, 3] of 3 vertices"),
            "{err}"
        );
    }

    #[test]
    fn a_shape_naming_a_missing_collision_attribute_is_an_error() {
        let err = Physics::from_value(&physics(vec![mesh(2, &[0, 1, 2])], vec![])).unwrap_err();
        assert!(err.to_string().contains("collision attribute 2"), "{err}");
    }

    #[test]
    fn geometry_cut_mid_vertex_is_an_error() {
        let hull = object(vec![(
            "m_Hull",
            object(vec![("m_VertexPositions", floats(&[1.0, 2.0]))]),
        )]);
        let err = Physics::from_value(&physics(vec![], vec![hull])).unwrap_err();
        assert!(err.to_string().contains("8 bytes of vertices"), "{err}");
    }
}
