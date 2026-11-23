//! Synthetic `vmdl_c` model tests: embedded mesh field variants without a local Dota install.

use heatel::{Model, Resource};

#[path = "common/kv3.rs"]
mod fixture;
#[path = "support/wire_builders.rs"]
mod wire;
use fixture::{K, kv3_file, s};
use wire::resource;

fn model_with_morph(morph_block_key: &'static str, morph_block_index: i64, atlas: &str) -> Vec<u8> {
    let ctrl = K::Obj(vec![(
        "embedded_meshes",
        K::Arr(vec![K::Obj(vec![
            ("name", s("face_lod0")),
            ("data_block", K::Int(2)),
            (morph_block_key, K::Int(morph_block_index)),
            ("morph_texture", s(atlas)),
        ])]),
    )]);
    let data = K::Obj(vec![("m_modelSkeleton", K::Obj(vec![]))]);
    let mdat = K::Obj(vec![("m_sceneObjects", K::Arr(vec![]))]);
    let mrph = K::Obj(vec![
        ("m_nWidth", K::UInt(4)),
        ("m_nHeight", K::UInt(4)),
        (
            "m_bundleTypes",
            K::Arr(vec![s("MORPH_BUNDLE_TYPE_POSITION_SPEED")]),
        ),
        (
            "m_morphDatas",
            K::Arr(vec![K::Obj(vec![("m_morphRectDatas", K::Arr(vec![]))])]),
        ),
        (
            "m_FlexDesc",
            K::Arr(vec![K::Obj(vec![("m_szFacs", s("jawOpen"))])]),
        ),
        (
            "m_FlexControllers",
            K::Arr(vec![K::Obj(vec![
                ("m_szName", s("jawOpen")),
                ("min", K::Float(0.0)),
                ("max", K::Float(1.0)),
            ])]),
        ),
        ("m_FlexRules", K::Arr(vec![])),
    ]);
    resource(&[
        (b"CTRL", kv3_file(&ctrl)),
        (b"DATA", kv3_file(&data)),
        (b"MDAT", kv3_file(&mdat)),
        (b"MRPH", kv3_file(&mrph)),
    ])
}

#[test]
fn embedded_mesh_discovers_morph_block_under_both_camel_and_snake_keys() {
    let atlas = "models/heroes/lion/lion/lion_base_vmorf.vtex";
    for key in ["m_nMorphBlock", "morph_block"] {
        let bytes = model_with_morph(key, 3, atlas);
        let res = Resource::parse(&bytes).expect("valid synthetic vmdl_c");
        let model = Model::read(&res).expect("model should read");
        assert_eq!(model.meshes.len(), 1, "key={key}");
        let morph = model.meshes[0]
            .morph
            .as_ref()
            .unwrap_or_else(|| panic!("expected morph set for key={key}"));
        assert_eq!(morph.atlas, atlas, "key={key}");
        assert_eq!(morph.flex.descs, vec!["jawOpen"], "key={key}");
        assert_eq!(morph.flex.controllers.len(), 1, "key={key}");
        assert_eq!(morph.flex.controllers[0].name, "jawOpen", "key={key}");
    }
}

#[test]
fn negative_morph_block_or_empty_morph_texture_yields_no_morph_set() {
    for (index, atlas) in [
        (-1, "models/heroes/lion/lion/lion_base_vmorf.vtex"),
        (3, ""),
        (-1, ""),
    ] {
        let bytes = model_with_morph("morph_block", index, atlas);
        let res = Resource::parse(&bytes).expect("valid synthetic vmdl_c");
        let model = Model::read(&res).expect("model should read");
        assert_eq!(model.meshes.len(), 1);
        assert!(
            model.meshes[0].morph.is_none(),
            "expected no morph set for index={index}, atlas={atlas:?}"
        );
    }
}
