//! Exported bounds describe drawn geometry; empty meshes do not add the origin.
use heatel::{
    Model,
    gltf::{GltfOptions, write_glb},
    model::{Mesh, Primitive},
    vertices::Vertices,
};
use std::collections::HashMap;

fn mesh(positions: Vec<[f32; 3]>, drawn: bool) -> Mesh {
    Mesh {
        name: "shape".to_owned(),
        vertices: vec![Vertices {
            positions,
            ..Vertices::default()
        }],
        primitives: if drawn {
            vec![Primitive {
                vertices: 0,
                indices: vec![0, 1, 2],
                material: String::new(),
                tint: None,
            }]
        } else {
            Vec::new()
        },
        morph: None,
        targets: Vec::new(),
    }
}
fn export(meshes: Vec<Mesh>) -> serde_json::Value {
    let model = Model {
        bones: Vec::new(),
        meshes,
        material_groups: Vec::new(),
        attachments: Vec::new(),
    };
    let options = GltfOptions {
        name: "shape",
        meshes: None,
        skeleton: false,
        materials: &HashMap::new(),
        texture_uri: &str::to_owned,
        clips: &[],
        generator: "test",
    };
    let (glb, _) = write_glb(&model, &options).expect("GLB");
    let length = u32::from_le_bytes(glb[12..16].try_into().expect("JSON length")) as usize;
    serde_json::from_slice(&glb[20..20 + length]).expect("JSON")
}
#[test]
fn exported_mesh_bounds_remain_available_without_retaining_vertex_buffers() {
    let mut mesh = mesh(vec![[-2.0, -1.0, -3.0], [2.0, 1.0, 3.0], [0.0; 3]], true);
    mesh.vertices.push(Vertices {
        positions: vec![[1000.0; 3]; 3],
        ..Vertices::default()
    });
    let json = export(vec![mesh]);
    assert_eq!(
        json["meshes"][0]["extras"]["render_bounds"],
        serde_json::json!({"min": [-2.0, -1.0, -3.0], "max": [2.0, 1.0, 3.0]})
    );
}
#[test]
fn a_mesh_without_primitives_reports_no_render_bounds() {
    let json = export(vec![
        mesh(vec![[1000.0; 3]; 3], false),
        mesh(
            vec![[10.0, 4.0, 6.0], [12.0, 8.0, 10.0], [11.0, 5.0, 7.0]],
            true,
        ),
    ]);
    assert!(
        json["meshes"][0]["extras"].get("render_bounds").is_none(),
        "empty geometry must not add the origin"
    );
    assert_eq!(
        json["meshes"][1]["extras"]["render_bounds"],
        serde_json::json!({"min": [10.0, 4.0, 6.0], "max": [12.0, 8.0, 10.0]})
    );
}
#[test]
fn an_empty_used_buffer_does_not_expand_the_other_buffers_bounds() {
    let mut mesh = mesh(Vec::new(), false);
    mesh.primitives.push(Primitive {
        vertices: 0,
        indices: Vec::new(),
        material: String::new(),
        tint: None,
    });
    mesh.vertices.push(Vertices {
        positions: vec![[10.0, 4.0, 6.0], [12.0, 8.0, 10.0], [11.0, 5.0, 7.0]],
        ..Vertices::default()
    });
    mesh.primitives.push(Primitive {
        vertices: 1,
        indices: vec![0, 1, 2],
        material: String::new(),
        tint: None,
    });
    let json = export(vec![mesh]);
    assert!(
        json["accessors"]
            .as_array()
            .expect("drawn geometry accessors")
            .iter()
            .all(|accessor| accessor["count"].as_u64().expect("accessor count") > 0),
        "glTF cannot contain a zero-count accessor for an empty buffer"
    );
    assert_eq!(
        json["meshes"][0]["extras"]["render_bounds"],
        serde_json::json!({"min": [10.0, 4.0, 6.0], "max": [12.0, 8.0, 10.0]})
    );
}
