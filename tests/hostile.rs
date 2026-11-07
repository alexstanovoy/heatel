//! Files built here that once crashed or blew up a converter, found by the fuzz
//! targets (`fuzz/`) or in their review: models whose draw calls or attributes run past their
//! buffers or copy one large buffer many times, the glTF writer given what glTF cannot say,
//! materials of more parameters than any, a world that lists its files again and again,
//! tooltip scripts that took quadratic time, and portrait framings of hostile numbers, tints and
//! structure. Each is an error now, or converts within bounds.

use std::cell::RefCell;
use std::collections::HashMap;

use heatel::anim::{Clip, Keys, Track};
use heatel::gltf::{GltfOptions, write_glb};
use heatel::material::Material;
use heatel::model::{Bone, Mesh, Primitive};
use heatel::particle::{Operator, System};
use heatel::tooltips::{Wanted, tooltips};
use heatel::vertices::Vertices;
use heatel::world::{Band, WorldOptions, export_world};
use heatel::{Model, Resource, SliceSource, Source, Vpk, kv3};

#[path = "common/kv3.rs"]
mod fixture;
use fixture::{K, kv3_file, s};

/// A compiled resource of `blocks`; each block's offset counts from its own field.
fn resource(blocks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_end = 16 + 12 * blocks.len();
    let size = table_end + blocks.iter().map(|(_, b)| b.len()).sum::<usize>();
    let mut out = (size as u32).to_le_bytes().to_vec();
    out.extend(12u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(8u32.to_le_bytes());
    out.extend((blocks.len() as u32).to_le_bytes());
    let mut at = table_end;
    for (kind, block) in blocks {
        let field = out.len() + 4;
        out.extend(*kind);
        out.extend(((at - field) as u32).to_le_bytes());
        out.extend((block.len() as u32).to_le_bytes());
        at += block.len();
    }
    for (_, block) in blocks {
        out.extend(block);
    }
    out
}

/// A buffer description of a mesh's `CTRL` entry: `count` elements of `stride` bytes in block
/// `block`, with `fields` (semantic, format, offset).
fn buffer(count: u64, stride: u64, block: i64, fields: Vec<(&str, u64, K)>) -> K {
    let fields = fields
        .into_iter()
        .map(|(semantic, format, offset)| {
            K::Obj(vec![
                ("m_pSemanticName", s(semantic)),
                ("m_nSemanticIndex", K::Int(0)),
                ("m_Format", K::UInt(format)),
                ("m_nOffset", offset),
            ])
        })
        .collect();
    K::Obj(vec![
        ("m_nElementCount", K::UInt(count)),
        ("m_nElementSizeInBytes", K::UInt(stride)),
        ("m_nBlockIndex", K::Int(block)),
        ("m_inputLayoutFields", K::Arr(fields)),
    ])
}

/// A draw call of `count` indices from the start of index buffer 0 over vertex buffer
/// `vertices`.
fn draw_call(count: i64, vertices: i64, material: &str) -> K {
    K::Obj(vec![
        ("m_nStartIndex", K::Int(0)),
        ("m_nIndexCount", K::Int(count)),
        ("m_nBaseVertex", K::Int(0)),
        (
            "m_vertexBuffers",
            K::Arr(vec![K::Obj(vec![("m_hBuffer", K::Int(vertices))])]),
        ),
        ("m_indexBuffer", K::Obj(vec![("m_hBuffer", K::Int(0))])),
        ("m_material", s(material)),
    ])
}

/// A model of one mesh named `mesh`: a vertex buffer (block 3) whose POSITION field is at
/// `offset`, `indices` zero indices (block 4), and `calls` draw calls.
fn model(mesh: &str, offset: K, indices: usize, calls: Vec<K>) -> Vec<u8> {
    let ctrl = K::Obj(vec![(
        "embedded_meshes",
        K::Arr(vec![K::Obj(vec![
            ("name", s(mesh)),
            ("mesh_index", K::Int(0)),
            ("data_block", K::Int(2)),
            (
                "m_vertexBuffers",
                K::Arr(vec![buffer(1, 12, 3, vec![("POSITION", 6, offset)])]),
            ),
            (
                "m_indexBuffers",
                K::Arr(vec![buffer(indices as u64, 4, 4, vec![])]),
            ),
        ])]),
    )]);
    let mesh = K::Obj(vec![(
        "m_sceneObjects",
        K::Arr(vec![K::Obj(vec![("m_drawCalls", K::Arr(calls))])]),
    )]);
    resource(&[
        (b"DATA", kv3_file(&K::Obj(vec![]))),
        (b"CTRL", kv3_file(&ctrl)),
        (b"MDAT", kv3_file(&mesh)),
        (b"MVTX", vec![0; 12]),
        (b"MIDX", vec![0; 4 * indices]),
    ])
}

fn read_model(bytes: &[u8]) -> heatel::Result<Model> {
    Model::read(&Resource::parse(bytes)?)
}

/// A draw call over a vertex buffer its mesh lacks read as one, and the `.glb` writer indexed
/// past its list.
#[test]
fn a_draw_call_on_a_vertex_buffer_the_mesh_lacks_is_an_error() {
    let file = model("m", K::UInt(0), 0, vec![draw_call(0, 5, "")]);
    assert!(read_model(&file).is_err());
    let good = model("m", K::UInt(0), 3, vec![draw_call(3, 0, "")]);
    assert_eq!(
        read_model(&good).unwrap().meshes[0].primitives[0].indices,
        [0; 3]
    );
}

/// A field offset near the top of the address space wrapped around the bounds check (and
/// panicked in a debug build).
#[test]
fn a_field_past_the_address_space_is_an_error() {
    let file = model("m", K::UInt(u64::MAX - 3), 3, vec![draw_call(3, 0, "")]);
    assert!(read_model(&file).is_err());
}

/// Each draw call copies its range of the index buffer: a small description of many draw
/// calls over one large buffer took gigabytes.
#[test]
fn draw_calls_copying_one_large_index_range_are_charged_for_every_copy() {
    let indices = 1 << 20;
    let calls = (0..64).map(|_| draw_call(indices as i64, 0, "")).collect();
    let file = model("m", K::UInt(0), indices, calls);
    assert!(read_model(&file).is_err());
    let calls = (0..4).map(|_| draw_call(indices as i64, 0, "")).collect();
    assert!(read_model(&model("m", K::UInt(0), indices, calls)).is_ok());
}

fn options<'a>(
    clips: &'a [Clip],
    materials: &'a HashMap<String, Material>,
    uri: &'a dyn Fn(&str) -> String,
) -> GltfOptions<'a> {
    GltfOptions {
        name: "models/m.vmdl",
        meshes: None,
        skeleton: true,
        materials,
        texture_uri: uri,
        clips,
        generator: "test",
    }
}

fn bone(parent: Option<usize>, translation: [f32; 3]) -> Bone {
    Bone {
        name: "b".to_owned(),
        parent,
        translation,
        rotation: [0.0, 0.0, 0.0, 1.0],
    }
}

fn triangle(indices: Vec<u32>, normals: usize) -> Mesh {
    Mesh {
        name: "m".to_owned(),
        vertices: vec![Vertices {
            positions: vec![[0.0; 3]; 3],
            normals: Some(vec![[0.0, 1.0, 0.0]; normals]),
            ..Vertices::default()
        }],
        primitives: vec![Primitive {
            vertices: 0,
            indices,
            material: String::new(),
            tint: None,
        }],
        morph: None,
        targets: Vec::new(),
    }
}

/// The writer takes public types: a bone parented after itself indexed past the node list or
/// made a cycle, and indices, lengths and numbers glTF cannot hold made a file a glTF
/// loader refused.
#[test]
fn the_glb_writer_refuses_what_gltf_cannot_say() {
    let materials = HashMap::new();
    let uri = |v: &str| v.to_owned();
    let write = |bones: Vec<Bone>, mesh: Mesh, clips: &[Clip]| {
        let model = Model {
            bones,
            meshes: vec![mesh],
            material_groups: Vec::new(),
            attachments: Vec::new(),
        };
        write_glb(&model, &options(clips, &materials, &uri)).map(|(glb, _)| glb)
    };
    let fine = || triangle(vec![0, 1, 2], 3);
    let glb = write(
        vec![bone(None, [0.0; 3]), bone(Some(0), [1.0; 3])],
        fine(),
        &[],
    )
    .unwrap();
    assert_eq!(
        u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
        glb.len()
    );
    assert!(write(vec![bone(Some(0), [0.0; 3])], fine(), &[]).is_err());
    assert!(write(vec![bone(None, [f32::NAN; 3])], fine(), &[]).is_err());
    assert!(write(Vec::new(), triangle(vec![0, 1, 3], 3), &[]).is_err());
    assert!(write(Vec::new(), triangle(vec![0, 1, 2], 2), &[]).is_err());
    let colored = |name: &str| {
        let mut mesh = fine();
        mesh.vertices[0].extra.push(heatel::vertices::Attribute {
            name: name.to_owned(),
            width: 4,
            values: vec![[1.0; 4]; 3],
        });
        write(Vec::new(), mesh, &[])
    };
    assert!(colored("COLOR_1").is_ok());
    assert!(
        colored("COLOR_99999999999").is_err(),
        "glTF's set numbers are u32"
    );
    assert!(colored("NORMAL").is_err());
    let keys = || Keys {
        times: vec![0.0],
        values: vec![[0.0; 3]],
    };
    let track = || Track {
        rotation: Keys {
            times: vec![0.0],
            values: vec![[0.0, 0.0, 0.0, 1.0]],
        },
        translation: keys(),
        scale: keys(),
    };
    let clip = |tracks: usize| Clip {
        name: "c".to_owned(),
        tracks: (0..tracks).map(|_| track()).collect(),
    };
    assert!(write(vec![bone(None, [0.0; 3])], fine(), &[clip(1)]).is_ok());
    assert!(write(vec![bone(None, [0.0; 3])], fine(), &[clip(2)]).is_err());
}

/// A material's parameters are kept in file order with each name once, and every glTF
/// material made of it (one per tint) carries them all: ten thousands of them took quadratic
/// time and gigabytes.
#[test]
fn a_material_of_more_parameters_than_any_is_an_error() {
    let params = |n: usize| {
        let list = (0..n)
            .map(|i| {
                K::Obj(vec![
                    ("m_name", K::Str(format!("p{i}"))),
                    ("m_flValue", K::Float(0.5)),
                ])
            })
            .collect();
        kv3::decode(&kv3_file(&K::Obj(vec![("m_floatParams", K::Arr(list))])))
            .unwrap()
            .root
    };
    assert!(Material::from_vmat("materials/m.vmat", &params(20_000)).is_err());
    let fine = Material::from_vmat("materials/m.vmat", &params(30)).unwrap();
    assert_eq!(fine.float("p29"), Some(0.5));
}

/// A particle system definition whose operator list is `operators`, read from its KV3.
fn particle_system(operators: Vec<K>) -> heatel::Result<System> {
    particle_lists(vec![("m_Operators", operators)])
}

/// A particle system definition of the function lists `lists`, read from its KV3.
fn particle_lists(lists: Vec<(&'static str, Vec<K>)>) -> heatel::Result<System> {
    let root = K::Obj(
        [("_class", s("CParticleSystemDefinition"))]
            .into_iter()
            .chain(lists.into_iter().map(|(key, list)| (key, K::Arr(list))))
            .collect(),
    );
    System::from_document(&kv3::decode(&kv3_file(&root)).unwrap())
}

fn operator(class: &str, members: Vec<(&'static str, K)>) -> K {
    K::Obj([("_class", s(class))].into_iter().chain(members).collect())
}

#[test]
fn distance_constraints_read_the_shell_and_its_control_point() {
    let system = particle_lists(vec![(
        "m_Constraints",
        vec![operator(
            "C_OP_ConstrainDistance",
            vec![
                ("m_fMinDistance", K::Float(5.0)),
                ("m_fMaxDistance", K::Float(75.0)),
                (
                    "m_CenterOffset",
                    K::Arr(vec![K::Float(5.0), K::Float(-7.0), K::Float(50.0)]),
                ),
                ("m_nControlPointNumber", K::Int(63)),
                ("m_bGlobalCenter", K::Bool(true)),
            ],
        )],
    )])
    .expect("distance constraint");
    assert!(
        system.unsupported_classes().is_empty(),
        "the shell is simulated"
    );
    #[cfg(feature = "serde")]
    {
        let json =
            serde_json::to_value(&system.constraints[0].class).expect("serialized definition");
        assert_eq!(
            json["Distance"]["min_distance"],
            serde_json::json!({"Literal": 5.0})
        );
        assert_eq!(
            json["Distance"]["max_distance"],
            serde_json::json!({"Literal": 75.0})
        );
        assert_eq!(
            json["Distance"]["transform"]["ControlPoint"]["control_point"],
            63
        );
        assert_eq!(json["Distance"]["global_center"], true);
        assert_eq!(
            json["Distance"]["center_offset"],
            serde_json::json!({"Literal": [5.0, -7.0, 50.0]})
        );
    }
}

#[test]
fn distance_constraints_reject_control_points_outside_the_table() {
    let result = particle_lists(vec![(
        "m_Constraints",
        vec![operator(
            "C_OP_ConstrainDistance",
            vec![("m_nControlPointNumber", K::Int(64))],
        )],
    )]);
    assert_eq!(
        result
            .expect_err("control point 64 is outside the table")
            .to_string(),
        "vpcf: m_nControlPointNumber 64 is outside -1..=63"
    );
}

#[test]
fn distance_constraints_read_collection_inputs_without_losing_the_transform() {
    use heatel::particle::{Constraint, ControlPoint, FloatInput, Transform, VectorInput};
    let system = particle_lists(vec![(
        "m_Constraints",
        vec![operator(
            "C_OP_ConstrainDistance",
            vec![
                (
                    "m_fMaxDistance",
                    K::Obj(vec![("m_flLiteralValue", K::Float(75.0))]),
                ),
                (
                    "m_nControlPointNumber",
                    K::Obj(vec![
                        ("m_nControlPoint", K::Int(7)),
                        ("m_bUseOrientation", K::Bool(false)),
                    ]),
                ),
                (
                    "m_CenterOffset",
                    K::Obj(vec![(
                        "m_vLiteralValue",
                        K::Arr(vec![K::Float(1.0), K::Float(2.0), K::Float(3.0)]),
                    )]),
                ),
            ],
        )],
    )])
    .expect("collection inputs");
    let Constraint::Distance(d) = &system.constraints[0].class else {
        panic!("distance");
    };
    assert_eq!(d.max_distance, FloatInput::Literal(75.0));
    assert_eq!(
        d.transform,
        Transform::ControlPoint {
            control_point: ControlPoint::new(7).expect("CP7"),
            orientation: false,
            range_max: None,
        }
    );
    assert_eq!(d.center_offset, VectorInput::Literal([1.0, 2.0, 3.0]));
}

#[test]
fn center_control_points_read_offsets_averaging_and_parent_scope() {
    let system = particle_lists(vec![(
        "m_PreEmissionOperators",
        vec![operator(
            "C_OP_SetControlPointToCenter",
            vec![
                ("m_nCP1", K::Int(2)),
                (
                    "m_vecCP1Pos",
                    K::Arr(vec![K::Float(1.0), K::Float(-2.0), K::Float(3.0)]),
                ),
                ("m_bUseAvgParticlePos", K::Bool(true)),
                ("m_nSetParent", s("PARTICLE_SET_PARENT_ROOT")),
            ],
        )],
    )])
    .expect("centre control point");
    assert!(
        system.unsupported_classes().is_empty(),
        "the centre setter must be simulated"
    );
    #[cfg(feature = "serde")]
    {
        let json = serde_json::to_value(&system.pre_emission[0].class).expect("serialized setter");
        let value = &json["SetControlPointToCenter"];
        assert_eq!(value["control_point"], 2);
        assert_eq!(value["offset"], serde_json::json!([1.0, -2.0, 3.0]));
        assert_eq!(value["average"], true);
        assert_eq!(value["target"], "Root");
    }
}

#[test]
fn center_control_points_reject_an_output_past_the_table() {
    let result = particle_lists(vec![(
        "m_PreEmissionOperators",
        vec![operator(
            "C_OP_SetControlPointToCenter",
            vec![("m_nCP1", K::Int(64))],
        )],
    )]);
    assert_eq!(
        result.expect_err("CP64").to_string(),
        "vpcf: m_nCP1 64 is outside -1..=63"
    );
}

/// A particle system's lists are bounded (Dota's have up to ~30 functions), and every
/// function's members are checked: a control point past the 64 a system has, a number that is
/// not finite or a member of the wrong kind is an error, not a value the simulation trips on.
#[test]
fn a_particle_system_of_hostile_functions_is_an_error() {
    let movement = || operator("C_OP_BasicMovement", vec![]);
    assert!(particle_system((0..1000).map(|_| movement()).collect()).is_err());
    let lock = |cp: i64| {
        operator(
            "C_OP_PositionLock",
            vec![("m_nControlPointNumber", K::Int(cp))],
        )
    };
    assert!(particle_system(vec![lock(64)]).is_err());
    let fade = |time: f64| {
        operator(
            "C_OP_FadeOutSimple",
            vec![("m_flFadeOutTime", K::Float(time))],
        )
    };
    assert!(particle_system(vec![fade(f64::NAN)]).is_err());
    assert!(particle_system(vec![fade(f64::INFINITY)]).is_err());
    assert!(
        particle_system(vec![operator(
            "C_OP_FadeOutSimple",
            vec![("m_flFadeOutTime", s("long"))]
        )])
        .is_err()
    );
    let fine = particle_system(vec![movement(), lock(63), fade(0.5)]).unwrap();
    assert_eq!(fine.operators.len(), 3);
    assert!(matches!(
        fine.operators[2].class,
        Operator::FadeOutSimple { time: 0.5, .. }
    ));
}

/// The pre-emission operators and the projected renderers' materials are bounded and checked
/// like every other list and member.
#[test]
fn a_particle_system_of_hostile_pre_emission_operators_or_materials_is_an_error() {
    let stop = |duration: K| operator("C_OP_StopAfterCPDuration", vec![("m_flDuration", duration)]);
    let pre = |list: Vec<K>| particle_lists(vec![("m_PreEmissionOperators", list)]);
    assert!(pre((0..1000).map(|_| stop(K::Float(1.0))).collect()).is_err());
    assert!(pre(vec![stop(K::Float(f64::NAN))]).is_err());
    let projected = |path: String| {
        let material = K::Obj(vec![("m_hMaterial", K::Str(path))]);
        operator(
            "C_OP_RenderProjected",
            vec![("m_vecProjectedMaterials", K::Arr(vec![material]))],
        )
    };
    let renderers = |list: Vec<K>| particle_lists(vec![("m_Renderers", list)]);
    assert!(renderers(vec![projected("m/".repeat(1000))]).is_err());
    let fine = pre(vec![stop(K::Float(2.0))]).unwrap();
    assert!(fine.unsupported_classes().is_empty());
    let decal = renderers(vec![projected("materials/d.vmat".to_owned())]).unwrap();
    assert!(decal.materials().any(|m| m == "materials/d.vmat"));
}

/// The model renderers' lists and paths, the hitbox sets of the functions on a model and the
/// constraints are bounded and checked like every other list and member.
#[test]
fn a_particle_system_of_hostile_models_or_constraints_is_an_error() {
    let models = |count: usize, path: String| {
        let entry = || K::Obj(vec![("m_model", K::Str(path.clone()))]);
        operator(
            "C_OP_RenderModels",
            vec![("m_ModelList", K::Arr((0..count).map(|_| entry()).collect()))],
        )
    };
    let renderers = |list: Vec<K>| particle_lists(vec![("m_Renderers", list)]);
    assert!(renderers(vec![models(1000, "models/m.vmdl".to_owned())]).is_err());
    assert!(renderers(vec![models(1, "m/".repeat(1000))]).is_err());
    let on_model = |set: String| {
        operator(
            "C_INIT_CreateOnModel",
            vec![("m_HitboxSetName", K::Str(set))],
        )
    };
    let initializers = |list: Vec<K>| particle_lists(vec![("m_Initializers", list)]);
    assert!(initializers(vec![on_model("s".repeat(1000))]).is_err());
    let plane = |radius: K| operator("C_OP_PlanarConstraint", vec![("m_flRadiusScale", radius)]);
    let constraints = |list: Vec<K>| particle_lists(vec![("m_Constraints", list)]);
    assert!(constraints((0..1000).map(|_| plane(K::Float(1.0))).collect()).is_err());
    assert!(constraints(vec![plane(K::Float(f64::NAN))]).is_err());
    let fine = renderers(vec![models(2, "models/m.vmdl".to_owned())]).unwrap();
    assert_eq!(fine.models().count(), 2);
    assert!(
        constraints(vec![plane(K::Float(1.0))])
            .unwrap()
            .unsupported_classes()
            .is_empty()
    );
    assert!(
        initializers(vec![on_model("default".to_owned())])
            .unwrap()
            .unsupported_classes()
            .is_empty()
    );
}

/// A package (VPK v1) of `files`, their bytes after the directory tree, and where each starts.
fn package<'a>(files: &[(&'a str, Vec<u8>)]) -> (Vec<u8>, HashMap<u64, &'a str>) {
    let (mut tree, mut data, mut starts): (Vec<u8>, Vec<u8>, Vec<(u64, &str)>) =
        (Vec::new(), Vec::new(), Vec::new());
    for (path, bytes) in files {
        let (dir, file) = path.rsplit_once('/').unwrap();
        let (name, ext) = file.rsplit_once('.').unwrap();
        for part in [ext, dir, name] {
            tree.extend(part.as_bytes());
            tree.push(0);
        }
        tree.extend(crc32fast::hash(bytes).to_le_bytes());
        tree.extend([0, 0, 0xFF, 0x7F]);
        tree.extend((data.len() as u32).to_le_bytes());
        tree.extend((bytes.len() as u32).to_le_bytes());
        tree.extend([0xFF, 0xFF, 0, 0]);
        starts.push((data.len() as u64, *path));
        data.extend(bytes);
    }
    tree.push(0);
    let mut out = Vec::new();
    for word in [0x55AA_1234, 1, tree.len() as u32] {
        out.extend(u32::to_le_bytes(word));
    }
    let data_start = (12 + tree.len()) as u64;
    out.extend(tree);
    out.extend(data);
    let starts = starts
        .into_iter()
        .map(|(at, p)| (data_start + at, p))
        .collect();
    (out, starts)
}

/// An install that counts the reads at each offset of its one package.
struct Counted {
    src: SliceSource<Vec<u8>>,
    reads: RefCell<HashMap<u64, usize>>,
}

impl Source for Counted {
    fn size(&self, path: &str) -> heatel::Result<u64> {
        self.src.size(path)
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> heatel::Result<Vec<u8>> {
        *self.reads.borrow_mut().entry(offset).or_default() += 1;
        self.src.read_at(path, offset, len)
    }
}

fn data_file(data: &K) -> Vec<u8> {
    resource(&[(b"DATA", kv3_file(data))])
}

/// A world, its node, an entity lump placing `models/m.vmdl` `placed` times, the model (its
/// mesh named `mesh`) and its material.
fn world(nodes: usize, placed: usize, mesh: &str) -> Vec<(&'static str, Vec<u8>)> {
    let node = || K::Obj(vec![("m_worldNodePrefix", s("maps/w/node"))]);
    let world = K::Obj(vec![
        ("m_worldNodes", K::Arr((0..nodes).map(|_| node()).collect())),
        ("m_entityLumps", K::Arr(vec![s("maps/w/lump.vents")])),
    ]);
    // Each placement its own tint: one glTF material each, of the one material file.
    let object = |i: usize| {
        let tint = (0..4).map(|c| K::Float(if c == 0 { i as f64 / 1e4 } else { 1.0 }));
        K::Obj(vec![
            ("m_renderableModel", s("models/m.vmdl")),
            ("m_vTintColor", K::Arr(tint.collect())),
        ])
    };
    let entity = || {
        K::Obj(vec![(
            "keyValues3Data",
            K::Obj(vec![(
                "values",
                K::Obj(vec![("model", s("models/m.vmdl"))]),
            )]),
        )])
    };
    let lump = K::Obj(vec![(
        "m_entityKeyValues",
        K::Arr((0..placed).map(|_| entity()).collect()),
    )]);
    let material = K::Obj(vec![("m_shaderName", s("hero.vfx"))]);
    vec![
        ("maps/w/world.vwrld_c", data_file(&world)),
        (
            "maps/w/node.vwnod_c",
            data_file(&K::Obj(vec![(
                "m_sceneObjects",
                K::Arr((0..100).map(object).collect()),
            )])),
        ),
        ("maps/w/lump.vents_c", data_file(&lump)),
        (
            "models/m.vmdl_c",
            model(
                mesh,
                K::UInt(0),
                3,
                vec![draw_call(3, 0, "materials/m.vmat")],
            ),
        ),
        ("materials/m.vmat_c", data_file(&material)),
    ]
}

/// The world exported, and how often each file was read.
fn export<'a>(files: &[(&'a str, Vec<u8>)]) -> (heatel::Result<()>, HashMap<&'a str, usize>) {
    let (bytes, starts) = package(files);
    let mut src = SliceSource::default();
    src.insert("maps/w.vpk", bytes);
    let src = Counted {
        src,
        reads: RefCell::new(HashMap::new()),
    };
    let vpk = Vpk::open(&src, "maps/w.vpk").unwrap();
    src.reads.borrow_mut().clear();
    let band = Band {
        half_width: 1e9,
        v_min: -1e9,
        v_max: 1e9,
    };
    let uri = |v: &str| v.to_owned();
    let options = WorldOptions {
        band: Some(band),
        exclude: &[],
        scene: "w",
        bin_uri: "w.bin",
        texture_uri: &uri,
        generator: "test",
    };
    let result = export_world(&src, &[&vpk], "maps/w/world.vwrld_c", &options);
    let reads = src.reads.borrow();
    let reads = starts
        .iter()
        .map(|(at, path)| (*path, reads.get(at).copied().unwrap_or(0)))
        .collect();
    (result.map(drop), reads)
}

/// A world that lists its node a thousand times read and placed it a thousand times, and read
/// its material again for every tint it is drawn with: each file is read once now.
#[test]
fn a_world_reads_each_of_its_files_once() {
    let (result, reads) = export(&world(1000, 1, "m"));
    result.unwrap();
    assert_eq!(reads["maps/w/node.vwnod_c"], 1);
    assert_eq!(reads["materials/m.vmat_c"], 1);
}

/// Every placement of a model names a node after its mesh: a long mesh name placed again and
/// again took gigabytes.
#[test]
fn names_placed_again_and_again_are_charged_for_every_copy() {
    let long = "m".repeat(64 << 10);
    assert!(export(&world(1, 8000, &long)).0.is_err());
    assert!(export(&world(1, 8000, "m")).0.is_ok());
}

/// Each key of a script block was looked up among all before it, and each unclosed `/*` was
/// searched to the end of the text: quadratic in the file's size.
#[test]
fn tooltip_scripts_read_in_linear_time() {
    let wanted = Wanted {
        names: vec!["item_boots".to_owned()],
        heroes: Vec::new(),
    };
    let localization = b"\"DOTA_Tooltip_ability_item_boots\" \"Boots\"\n";
    let mut script = String::from("\"DOTAAbilities\" {\n");
    for i in 0..300_000 {
        script += &format!("\"k{i}\" \"v\"\n");
    }
    script += &"/* ".repeat(300_000);
    script += "\"item_boots\" { \"ItemCost\" \"500\" } }";
    let found = tooltips(&wanted, localization, &[script.as_bytes()]).unwrap();
    assert_eq!(found["item_boots"].name, "Boots");
    assert_eq!(found["item_boots"].cost.as_deref(), Some("500"));
    let huge = vec![b' '; 17 << 20];
    assert!(tooltips(&wanted, localization, &[&huge]).is_err());
}

/// A quote that never closes, followed by escaped quotes only, had each of those quotes searched
/// to the end of the text again: quadratic in the file's size.
#[test]
fn scripts_with_an_unclosed_quote_read_in_linear_time() {
    let mut script =
        String::from("\"DOTAAbilities\" { \"item_blink\" { \"ItemCost\" \"2250\" } }\n\"");
    script += &"\\\"".repeat(1 << 20);
    let found = heatel::scripts::entries(script.as_bytes()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "item_blink");
}

/// Each quote of `portraits.txt` was searched to the end of its line for the line's end: a
/// Portraits block on one line of many keys and values took quadratic time (23 s at 2 MiB).
/// A file of the largest size read, on one line, reads in well under a second; 10 s leaves room
/// for a loaded machine, and a quadratic scan takes some half an hour.
#[test]
fn a_portraits_file_on_one_line_reads_in_linear_time() {
    use heatel::portraits::framings;
    let pairs = "\"k\" \"v\" ".repeat((16 << 20) / 8 - 64);
    let file = format!(
        "\"Portraits\" {{ \"models/a.vmdl\" {{ {pairs} }} \"models/b.vmdl\" {{ \"k\" \"v\" }} }}"
    );
    assert!(file.len() <= 16 << 20, "{}", file.len());
    let started = std::time::Instant::now();
    let read = framings(file.as_bytes(), &[("b", "models/b.vmdl")]).expect("a file of Portraits");
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(10), "{took:?}");
    assert_eq!(
        read.refused.len(),
        1,
        "b's entry lacks its numbers: {:?}",
        read.refused
    );
}

#[test]
fn flagbearer_particle_operators_read_synthetic_orientation_and_object_scale() {
    use heatel::particle::PreEmission;
    let definition = synthetic_flagbearer_orientation();
    let PreEmission::SetControlPointFromObjectScale { input, output } =
        &definition.pre_emission[0].class
    else {
        panic!("object scale")
    };
    assert_eq!((input.index(), output.index()), (62, 63));
    let PreEmission::SetControlPointFromObjectScale { input, output } =
        &definition.pre_emission[1].class
    else {
        panic!("default object scale")
    };
    assert_eq!((input.index(), output.index()), (0, 1));
    let Operator::RemapOrientationToRotations {
        transform,
        rotation_offset,
        use_quaternion,
        write_normal,
    } = &definition.operators[0].class
    else {
        panic!("legacy orientation")
    };
    assert_eq!(transform.control_point().expect("control point").index(), 7);
    assert_eq!(*rotation_offset, [10.0, 20.0, 30.0]);
    assert!(*use_quaternion);
    assert!(*write_normal);
    let Operator::RemapOrientationToRotations {
        transform,
        rotation_offset,
        use_quaternion,
        write_normal,
    } = &definition.operators[1].class
    else {
        panic!("transform orientation")
    };
    assert_eq!(transform.control_point().expect("control point").index(), 3);
    assert_eq!(*rotation_offset, [0.0; 3]);
    assert!(!*use_quaternion);
    assert!(!*write_normal);
}

#[test]
fn flagbearer_particle_operators_read_synthetic_snap_and_normal_lock() {
    let definition = particle_system(vec![
        operator(
            "C_OP_SetToCP",
            vec![
                ("m_nControlPointNumber", K::Int(4)),
                ("m_vecOffset", particle_vector([1.0, 2.0, 3.0])),
                ("m_bOffsetLocal", K::Int(1)),
            ],
        ),
        operator("C_OP_SetToCP", vec![]),
        operator(
            "C_OP_NormalLock",
            vec![("m_nControlPointNumber", K::Int(5))],
        ),
        operator("C_OP_NormalLock", vec![]),
    ])
    .expect("synthetic KV3 definitions");
    let Operator::SetToControlPoint {
        control_point,
        offset,
        local,
    } = &definition.operators[0].class
    else {
        panic!("snap")
    };
    assert_eq!(control_point.index(), 4);
    assert_eq!(*offset, [1.0, 2.0, 3.0]);
    assert!(*local);
    let Operator::SetToControlPoint {
        control_point,
        offset,
        local,
    } = &definition.operators[1].class
    else {
        panic!("default snap")
    };
    assert_eq!(control_point.index(), 0);
    assert_eq!(*offset, [0.0; 3]);
    assert!(!*local);
    let Operator::NormalLock { control_point } = &definition.operators[2].class else {
        panic!("normal lock")
    };
    assert_eq!(control_point.index(), 5);
    let Operator::NormalLock { control_point } = &definition.operators[3].class else {
        panic!("default normal lock")
    };
    assert_eq!(control_point.index(), 0);
}

#[test]
fn flagbearer_particle_operators_refuse_synthetic_control_points_beyond_the_schema_bounds() {
    for (class, key, list) in [
        (
            "C_OP_SetControlPointFromObjectScale",
            "m_nCPInput",
            "m_PreEmissionOperators",
        ),
        (
            "C_OP_SetControlPointFromObjectScale",
            "m_nCPOutput",
            "m_PreEmissionOperators",
        ),
        ("C_OP_RemapCPOrientationToRotations", "m_nCP", "m_Operators"),
        ("C_OP_SetToCP", "m_nControlPointNumber", "m_Operators"),
        ("C_OP_NormalLock", "m_nControlPointNumber", "m_Operators"),
    ] {
        for bad in [-2, 64] {
            let error = particle_lists(vec![(
                list,
                vec![operator(class, vec![(key, K::Int(bad))])],
            )])
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("vpcf: {key} {bad} is outside -1..=63")
            );
        }
    }
}

fn particle_vector(values: [f64; 3]) -> K {
    K::Arr(values.into_iter().map(K::Float).collect())
}

fn synthetic_flagbearer_orientation() -> System {
    let scale = operator(
        "C_OP_SetControlPointFromObjectScale",
        vec![("m_nCPInput", K::Int(62)), ("m_nCPOutput", K::Int(63))],
    );
    let legacy = operator(
        "C_OP_RemapCPOrientationToRotations",
        vec![
            ("m_nCP", K::Int(7)),
            ("m_vecRotation", particle_vector([10.0, 20.0, 30.0])),
            ("m_bUseQuat", K::Int(1)),
            ("m_bWriteNormal", K::Int(1)),
        ],
    );
    let modern = operator(
        "C_OP_RemapTransformOrientationToRotations",
        vec![(
            "m_TransformInput",
            K::Obj(vec![
                ("m_nType", s("PT_TYPE_CONTROL_POINT")),
                ("m_nControlPoint", K::Int(3)),
            ]),
        )],
    );
    particle_lists(vec![
        (
            "m_PreEmissionOperators",
            vec![
                scale,
                operator("C_OP_SetControlPointFromObjectScale", vec![]),
            ],
        ),
        ("m_Operators", vec![legacy, modern]),
    ])
    .expect("synthetic KV3 definitions")
}

#[test]
fn response_rules_reject_a_non_array_response_group_list() {
    let data = K::Obj(vec![("m_ResponseGroups", s("not an array"))]);
    let file = resource(&[(b"DATA", kv3_file(&data))]);
    let resource = Resource::parse(&file).expect("synthetic resource");
    let error =
        heatel::talker::read_response_rules(&resource, &mut heatel::Budget::default()).unwrap_err();
    assert_eq!(error.to_string(), "vrr: m_ResponseGroups is not an array");
}

#[test]
fn response_rules_read_a_line_respeak_delay() {
    let data = K::Obj(vec![(
        "m_ResponseGroups",
        K::Arr(vec![K::Obj(vec![(
            "m_responses",
            K::Arr(vec![K::Obj(vec![
                ("m_type", s("SPEAK")),
                ("m_value", s("example")),
                (
                    "m_params",
                    K::Obj(vec![("m_respeakdelay.start", K::Float(42.0))]),
                ),
            ])]),
        )])]),
    )]);
    let file = resource(&[(b"DATA", kv3_file(&data))]);
    let resource = Resource::parse(&file).expect("synthetic resource");
    let rules = heatel::talker::read_response_rules(&resource, &mut heatel::Budget::default())
        .expect("response rules");
    assert_eq!(rules.groups[0].responses[0].respeak_delay, Some(42.0));
}

#[test]
fn response_rules_read_nested_delay_ranges() {
    let data = K::Obj(vec![(
        "m_ResponseGroups",
        K::Arr(vec![K::Obj(vec![(
            "m_responses",
            K::Arr(vec![K::Obj(vec![
                ("m_type", s("SPEAK")),
                (
                    "m_params",
                    K::Obj(vec![(
                        "m_respeakdelay",
                        K::Obj(vec![("start", K::Float(42.0))]),
                    )]),
                ),
            ])]),
        )])]),
    )]);
    let file = resource(&[(b"DATA", kv3_file(&data))]);
    let resource = Resource::parse(&file).expect("synthetic resource");
    let rules = heatel::talker::read_response_rules(&resource, &mut heatel::Budget::default())
        .expect("response rules");
    assert_eq!(rules.groups[0].responses[0].respeak_delay, Some(42.0));
}

/// A response rules file of one group whose embedded rule is `rule` and whose responses are
/// `responses`.
fn response_rules_of(rule: K, responses: K) -> heatel::Result<heatel::talker::ResponseRules> {
    let data = K::Obj(vec![(
        "m_ResponseGroups",
        K::Arr(vec![K::Obj(vec![
            ("m_name", s("group")),
            ("m_responses", responses),
            ("m_pEmbeddedRule", rule),
        ])]),
    )]);
    let file = resource(&[(b"DATA", kv3_file(&data))]);
    heatel::talker::read_response_rules(
        &Resource::parse(&file).expect("synthetic resource"),
        &mut heatel::Budget::default(),
    )
}

fn one_line() -> K {
    K::Arr(vec![K::Obj(vec![
        ("m_type", s("SPEAK")),
        ("m_value", s("line")),
    ])])
}

/// A rule whose requirements do not read would match every situation: it is an error, not a
/// rule without requirements.
#[test]
fn response_rules_reject_requirements_that_are_not_a_list() {
    let rule = K::Obj(vec![("m_name", s("rule")), ("m_Requirements", s("Move"))]);
    let error = response_rules_of(rule, one_line()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "vrr: group's m_Requirements is not an array"
    );
}

/// A requirement that is not a name is an error, not a requirement dropped.
#[test]
fn response_rules_reject_a_requirement_that_is_not_a_name() {
    let rule = K::Obj(vec![
        ("m_name", s("rule")),
        ("m_Requirements", K::Arr(vec![s("Move"), K::Int(7)])),
    ]);
    let error = response_rules_of(rule, one_line()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "vrr: group's m_Requirements holds something that is not a name"
    );
}

#[test]
fn response_rules_reject_responses_that_are_not_a_list() {
    let rule = K::Obj(vec![
        ("m_name", s("rule")),
        ("m_Requirements", K::Arr(vec![s("Move")])),
    ]);
    let error = response_rules_of(rule, s("line")).unwrap_err();
    assert_eq!(
        error.to_string(),
        "vrr: group's m_responses is not an array"
    );
}

/// A legacy KV3 compressed with LZ4: a table of `count` empty strings, then an empty object.
/// A byte of the file for every 255 strings of the table, and a slot of the table for each.
fn empty_strings_kv3(count: u32) -> Vec<u8> {
    let mut data = count.to_le_bytes().to_vec();
    data.resize(4 + count as usize, 0);
    data.push(9);
    data.extend(0u32.to_le_bytes());
    data.extend(u32::MAX.to_le_bytes());
    let mut out = b"VKV\x03".to_vec();
    out.extend([
        0x8A, 0x34, 0x47, 0x68, 0xA1, 0x63, 0x5C, 0x4F, 0xA1, 0x97, 0x53, 0x80, 0x6F, 0xD9, 0xB1,
        0x19,
    ]);
    out.extend([0; 16]);
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(lz4_flex::block::compress(&data));
    out
}

/// A string table of more slots than the decoder's budget, cheap in the file, is refused
/// before its slots are allocated.
#[test]
fn a_string_table_past_the_budget_is_refused_before_it_is_allocated() {
    let file = empty_strings_kv3(16 << 20);
    assert!(file.len() < 1 << 17, "{} bytes", file.len());
    let error = kv3::decode(&file).unwrap_err();
    assert!(error.to_string().contains("more than it may"), "{error}");
}
/// A document decoded within a budget tells what it took: its buffers, its table and its
/// tree; within less than that, it is refused.
#[test]
fn a_document_decoded_within_a_budget_tells_what_it_took() {
    let file = empty_strings_kv3(1 << 10);
    let mut budget = heatel::Budget::new(1 << 20);
    kv3::decode_in(&file, &mut budget).expect("within a MiB");
    let spent = budget.spent();
    assert!(spent > (1 << 10) * std::mem::size_of::<String>(), "{spent}");
    let error = kv3::decode_in(&file, &mut heatel::Budget::new(spent - 1)).unwrap_err();
    assert!(error.to_string().contains("more than it may"), "{error}");
}

/// A version 2 KV3 compressed with zstd whose buffer is one byte: one frame of no stated
/// content size and a window of `2^window_log` bytes, of `blocks` RLE blocks of 128 KiB zeros
/// each. Some hundred bytes of the file for every 32 MiB the frame makes.
fn wide_window_kv3(window_log: u8, blocks: u32) -> Vec<u8> {
    let mut frame = 0xFD2F_B528u32.to_le_bytes().to_vec();
    // No single segment, no content size, no checksum.
    frame.extend([0x00, (window_log - 10) << 3]);
    for n in 0..blocks {
        let last = u32::from(n + 1 == blocks);
        let header = (128 << 10) << 3 | 1 << 1 | last;
        frame.extend(&header.to_le_bytes()[..3]);
        frame.push(0);
    }
    let mut out = (0x4B56_3300u32 | 2).to_le_bytes().to_vec();
    out.extend([0; 16]);
    out.extend(2u32.to_le_bytes());
    out.extend([0; 4]);
    // The 1-, 4- and 8-byte lanes and the type stream: empty.
    out.extend([0; 16]);
    out.extend([0; 4]);
    // The buffer's length, the compressed length, no blobs.
    out.extend(1u32.to_le_bytes());
    out.extend((frame.len() as u32).to_le_bytes());
    out.extend([0; 8]);
    out.extend(frame);
    out
}

/// The window a zstd frame declares is the decoder's to keep: it is charged to the document's
/// budget before the decoder is made, and a frame wider than the buffer needs is refused.
#[test]
fn a_zstd_window_past_the_budget_is_refused_before_the_decoder_keeps_it() {
    let file = wide_window_kv3(27, 256);
    assert!(file.len() < 2048, "{} bytes", file.len());
    let error = kv3::decode_in(&file, &mut heatel::Budget::new(1)).unwrap_err();
    assert!(error.to_string().contains("more than it may"), "{error}");
}

/// A zstd decoder's ring buffer grows past its window, and holds its old buffer while it makes
/// the new one: what a 16 MiB window makes it allocate is more than 18 MiB, refused before it
/// is made.
#[test]
fn a_zstd_decoder_is_charged_with_its_buffers_growth() {
    let file = wide_window_kv3(24, 256);
    let error = kv3::decode_in(&file, &mut heatel::Budget::new(18 << 20)).unwrap_err();
    assert!(error.to_string().contains("more than it may"), "{error}");
}

/// A sound's `CTRL` block, a small table of millions of strings, is decoded within the budget
/// the caller gives, for its sound and for its lip sync.
#[test]
fn a_sounds_ctrl_block_is_decoded_within_the_callers_budget() {
    let file = resource(&[(b"CTRL", empty_strings_kv3(1 << 20))]);
    let Err(error) = heatel::Sound::parse(&file, &mut heatel::Budget::new(16 << 20)) else {
        panic!("a sound read past its budget");
    };
    assert!(error.to_string().contains("more than it may"), "{error}");
    let lipsync = heatel::sound::Lipsync::parse(&file, &mut heatel::Budget::new(16 << 20));
    let error = lipsync.unwrap_err();
    assert!(error.to_string().contains("more than it may"), "{error}");
}

/// A stored legacy KV3 (as [`kv3_file`] writes it) compressed with LZ4.
fn lz4_kv3(stored: Vec<u8>) -> Vec<u8> {
    let body = &stored[36..];
    let mut out = stored[..4].to_vec();
    out.extend([
        0x8A, 0x34, 0x47, 0x68, 0xA1, 0x63, 0x5C, 0x4F, 0xA1, 0x97, 0x53, 0x80, 0x6F, 0xD9, 0xB1,
        0x19,
    ]);
    out.extend(&stored[20..36]);
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(lz4_flex::block::compress(body));
    out
}

/// A response that is not an object says nothing a rule can play: refused, not read as an
/// empty response.
#[test]
fn response_rules_reject_a_response_that_is_not_an_object() {
    let rule = K::Obj(vec![
        ("m_name", s("rule")),
        ("m_Requirements", K::Arr(vec![s("Move")])),
    ]);
    let error = response_rules_of(rule, K::Arr(vec![K::Int(0)])).unwrap_err();
    assert_eq!(
        error.to_string(),
        "vrr: group's m_responses holds something that is not a response"
    );
}

/// The rules' own lists are charged as they are built: a small file of a million empty
/// responses (each a few bytes of the document, a whole response of the rules) is refused by
/// the budget it is read within.
#[test]
fn response_rules_typed_past_the_budget_are_refused() {
    let group = || {
        K::Obj(vec![
            ("m_name", s("g")),
            (
                "m_responses",
                K::Arr((0..4096).map(|_| K::Obj(vec![])).collect()),
            ),
        ])
    };
    let data = K::Obj(vec![(
        "m_ResponseGroups",
        K::Arr((0..256).map(|_| group()).collect()),
    )]);
    let file = resource(&[(b"DATA", lz4_kv3(kv3_file(&data)))]);
    assert!(file.len() < 1 << 16, "{} bytes", file.len());
    let resource = Resource::parse(&file).expect("synthetic resource");
    let read = heatel::talker::read_response_rules(&resource, &mut heatel::Budget::new(128 << 20));
    let error = read.unwrap_err();
    assert!(error.to_string().contains("more than it may"), "{error}");
}

/// A key copied while it waits for its value is charged as it is copied: one of 8 MiB with no
/// value after it (left at the end, as KeyValues readers leave one) is refused by a budget of
/// a KiB, not copied unseen.
#[test]
fn a_dangling_keyvalues_key_is_charged_as_it_is_copied() {
    let text = format!("\"{}\"", "k".repeat(8 << 20));
    let mut budget = heatel::Budget::new(1 << 10);
    let error = heatel::keyvalues::parse_kv_in(&text, &mut budget).unwrap_err();
    assert!(error.to_string().contains("more than it may"), "{error}");
}

/// A portrait entry the reviews found taken or the whole file refused for: a tint of 2 (Dota's
/// own, which the fuzz target once held to 1), a far plane at the near floor, an entry that is no
/// KeyValues among good ones, an error repeating megabytes of the file, more entries than any
/// file has. Each framing read holds every number within its bounds; refusals are short.
#[test]
fn hostile_portrait_entries_are_refused_alone_and_bounded() {
    use heatel::portraits::{MAX_TINT, MIN_FAR, framings};
    let good = r#""PortraitLightPosition" "1 2 3" "PortraitLightAngles" "1 2 0" "PortraitLightFOV" "90"
        "PortraitLightColor" "255 255 255" "PortraitAmbientColor" "9 9 9"
        "PortraitAmbientDirection" "-1 -1 -1"
        "PortraitBackgroundTexture" "materials/a.vmat" "PortraitBackgroundColor4" "2 2 .5"
        "cameras" { "default" { "PortraitPosition" "100 0 100" "PortraitAngles" "0 180 0" "PortraitFOV" "25" } }"#;
    let huge = "9".repeat(4 << 20);
    let entry = |name: &str, extra: &str| format!("\"models/{name}.vmdl\" {{ {good} {extra} }}\n");
    let near = good.replacen(
        "\"PortraitFOV\" \"25\"",
        "\"PortraitFOV\" \"25\" \"PortraitFar\" \"8\"",
        1,
    );
    let ok = entry("ok", "");
    let long = entry("long", &format!("\"PortraitAmbientScale\" \"{huge}\""));
    let broken = "\"models/broken.vmdl\" { \"PortraitLightColor\" }";
    let file =
        format!("\"Portraits\" {{\n{ok}\"models/near.vmdl\" {{ {near} }}\n{broken}\n{long}}}");
    let wanted = [
        ("ok", "models/ok.vmdl"),
        ("near", "models/near.vmdl"),
        ("broken", "models/broken.vmdl"),
        ("long", "models/long.vmdl"),
    ];
    let read = framings(file.as_bytes(), &wanted).expect("a file of Portraits");
    let labels: Vec<&str> = read.framings.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(labels, ["ok"]);
    let refused: Vec<&str> = read.refused.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(refused.len(), 3, "{:?}", read.refused);
    assert!(
        read.refused.iter().all(|r| r.why.len() < 200),
        "{:?}",
        refused
    );
    let f = &read.framings[0];
    assert!(f.camera.far > MIN_FAR);
    let tints = f.background.as_ref().expect("a background").tints;
    assert!(
        tints
            .as_flattened()
            .iter()
            .all(|t| (0.0..=MAX_TINT).contains(t))
    );
    assert_eq!(tints[3], [2.0, 2.0, 0.5]);
    let many = format!("\"Portraits\" {{ {} }}", "\"m\" { }".repeat(70_000));
    let why = framings(many.as_bytes(), &[])
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(why.contains("more entries than any file has"), "{why}");
}
