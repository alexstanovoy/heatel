//! Files built here that once crashed or blew up a converter, found by the fuzz
//! targets (`fuzz/`) or in their review: models whose draw calls or attributes run past their
//! buffers or copy one large buffer many times, the glTF writer given what glTF cannot say,
//! materials of more parameters than any, a world that lists its files again and again, and
//! tooltip scripts that took quadratic time. Each is an error now, or converts within bounds.

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

/// A KV3 value to write.
enum K {
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Arr(Vec<K>),
    Obj(Vec<(&'static str, K)>),
}

fn s(text: &str) -> K {
    K::Str(text.to_owned())
}

/// Legacy binary KV3 (`VKV\x03`), stored as is: a string table, then the values.
fn kv3_file(root: &K) -> Vec<u8> {
    fn strings<'a>(v: &'a K, out: &mut Vec<&'a str>) {
        match v {
            K::Str(text) => out.push(text),
            K::Arr(items) => items.iter().for_each(|i| strings(i, out)),
            K::Obj(members) => members.iter().for_each(|(k, v)| {
                out.push(k);
                strings(v, out);
            }),
            _ => {}
        }
    }
    fn value(v: &K, ids: &HashMap<&str, i32>, out: &mut Vec<u8>) {
        match v {
            K::Int(n) => out.extend([3].into_iter().chain(n.to_le_bytes())),
            K::UInt(n) => out.extend([4].into_iter().chain(n.to_le_bytes())),
            K::Float(f) => out.extend([5].into_iter().chain(f.to_le_bytes())),
            K::Str(text) => out.extend([6].into_iter().chain(ids[text.as_str()].to_le_bytes())),
            K::Arr(items) => {
                out.extend([8].into_iter().chain((items.len() as i32).to_le_bytes()));
                items.iter().for_each(|i| value(i, ids, out));
            }
            K::Obj(members) => {
                out.extend([9].into_iter().chain((members.len() as i32).to_le_bytes()));
                for (k, v) in members {
                    out.extend(ids[k].to_le_bytes());
                    value(v, ids, out);
                }
            }
        }
    }
    let mut table = Vec::new();
    strings(root, &mut table);
    let mut ids = HashMap::new();
    let mut unique = Vec::new();
    for text in table {
        ids.entry(text).or_insert_with(|| {
            unique.push(text);
            unique.len() as i32 - 1
        });
    }
    let mut out = b"VKV\x03".to_vec();
    out.extend([
        0x00, 0x05, 0x86, 0x1B, 0xD8, 0xF7, 0xC1, 0x40, 0xAD, 0x82, 0x75, 0xA4, 0x82, 0x67, 0xE7,
        0x14,
    ]);
    out.extend([0; 16]);
    out.extend((unique.len() as u32).to_le_bytes());
    for text in unique {
        out.extend(text.as_bytes());
        out.push(0);
    }
    value(root, &ids, &mut out);
    out.extend(u32::MAX.to_le_bytes());
    out
}

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
