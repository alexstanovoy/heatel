use super::{MaterialParameter, Source2Material};
use crate::kv3::Value;

fn named(name: &str, field: &str, value: impl Into<Value>) -> Value {
    Value::object([("m_name", Value::from(name)), (field, value.into())])
}

#[test]
fn hero_slots_preserve_authored_values_and_numeric_storage() {
    let material = Source2Material::from_vmat(Value::object([
        ("m_shaderName", Value::from("hero.vfx")),
        ("m_materialName", Value::from("materials/test/hero.vmat")),
        (
            "m_textureParams",
            Value::from_array([
                named("g_tFresnelWarp", "m_pValue", "fresnel.vtex"),
                named("g_tFresnelWarpRim", "m_pValue", "rim.vtex"),
                named("custom_slot", "m_pValue", "custom.vtex"),
            ]),
        ),
        (
            "m_intParams",
            Value::from_array([
                named("g_flSpecularScale", "m_nValue", 2),
                named("F_ALPHA_TEST", "m_nValue", 0),
            ]),
        ),
        (
            "m_dynamicParams",
            Value::from_array([named("g_flRimLightScale", "m_value", Value::blob([1]))]),
        ),
    ]))
    .unwrap();

    let hero = material.hero_params().unwrap();
    assert_eq!(material.shader_name(), "hero.vfx");
    assert_eq!(material.material_name(), Some("materials/test/hero.vmat"));
    assert_eq!(hero.specular_scale, Some(2.0));
    assert_eq!(hero.fresnel_warp, Some("fresnel.vtex"));
    assert_eq!(hero.fresnel_warp_rim, Some("rim.vtex"));
    assert_eq!(hero.normal, None);
    assert_eq!(material.flag("F_ALPHA_TEST"), Some(false));
    assert_eq!(material.flag("F_TRANSLUCENT"), None);
    assert_eq!(material.texture("custom_slot"), Some("custom.vtex"));
    assert!(material.parameters().any(|field| matches!(
        field,
        ("g_flRimLightScale", MaterialParameter::Dynamic([1]))
    )));
}

#[test]
fn terrain_keeps_layer_indices_and_last_value_of_duplicate_keys() {
    let material = Source2Material::from_vmat(Value::object([
        ("m_shaderName", Value::from("dota_terrain")),
        (
            "m_textureParams",
            Value::from_array([
                named("g_tColor3", "m_pValue", "first.vtex"),
                named("g_tColor3", "m_pValue", "last.vtex"),
                named("g_tNormal2", "m_pValue", "normal.vtex"),
            ]),
        ),
        (
            "m_vectorParams",
            Value::from_array([named("g_vColorTintB3", "m_value", [1, 2, 3, 4])]),
        ),
    ]))
    .unwrap();

    let terrain = material.terrain_params().unwrap();
    assert_eq!(terrain.layers[3].color, Some("last.vtex"));
    assert_eq!(terrain.layers[2].normal, Some("normal.vtex"));
    assert_eq!(terrain.layers[0].color, None);
    assert_eq!(terrain.layers[3].tint_b, Some([1.0, 2.0, 3.0, 4.0]));
}

#[cfg(any(unix, windows))]
#[test]
#[ignore = "requires build-6997 Dota VPKs"]
fn vpk_material_audit() {
    use crate::{Budget, DirSource, Resource, Vpk, locate::install_in};
    use std::collections::BTreeMap;
    use std::path::Path;

    let path = std::env::var_os("HEATEL_DOTA_PATH").expect("set HEATEL_DOTA_PATH");
    let source = DirSource::new(install_in(Path::new(&path)).expect("Dota install").game);
    let pak = Vpk::open(&source, "pak01_dir.vpk").unwrap();
    let mut shaders = BTreeMap::<String, usize>::new();
    let mut groups = BTreeMap::<&str, [usize; 5]>::new();
    for entry in pak.entries().filter(|entry| entry.ext() == "vmat_c") {
        let path = entry.path();
        let bytes = pak
            .read(&source, entry)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        let resource = Resource::parse(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        let material = Source2Material::read(&resource, &mut Budget::default())
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        let shader = material.shader_name();
        *shaders.entry(shader.to_owned()).or_default() += 1;
        let counts = audit_fields(&material, &path);
        for (group, matches) in [
            ("all", true),
            ("hero shader", material.hero_params().is_some()),
            ("terrain shader", material.terrain_params().is_some()),
            ("water shader", shader.starts_with("water")),
            ("hero path", path.split('/').any(|part| part == "heroes")),
            ("creep path", path.split('/').any(|part| part == "creeps")),
            (
                "particle path",
                path.split('/').any(|part| part == "particles"),
            ),
        ] {
            if matches {
                let total = groups.entry(group).or_default();
                for (total, count) in total.iter_mut().zip(counts) {
                    *total += count;
                }
            }
        }
    }
    assert!(groups["hero shader"][0] > 0);
    assert!(groups["terrain shader"][0] > 0);
    println!("{} distinct shader names", shaders.len());
    println!("groups: [materials, textures, parameters, attributes, render attributes]");
    for (group, counts) in groups {
        println!("{group}: {counts:?}");
    }
}

#[cfg(any(unix, windows))]
fn audit_fields(material: &Source2Material, path: &str) -> [usize; 5] {
    let material_data = material.data();
    let authored_count = |keys: &[&str]| {
        keys.iter()
            .map(|key| material_data.array(key).len())
            .sum::<usize>()
    };
    let expected_params = authored_count(&[
        "m_intParams",
        "m_floatParams",
        "m_vectorParams",
        "m_textureParams",
        "m_dynamicParams",
        "m_dynamicTextureParams",
    ]);
    let expected_attributes = authored_count(&[
        "m_intAttributes",
        "m_floatAttributes",
        "m_vectorAttributes",
        "m_textureAttributes",
        "m_stringAttributes",
    ]);
    let parameters = material.parameters().count();
    let textures = material.textures().count();
    let attributes = material.attributes().count();
    let render_attributes = material.render_attributes_used().count();
    assert_eq!(parameters, expected_params, "{path}: shader parameters");
    assert_eq!(
        textures,
        authored_count(&["m_textureParams"]),
        "{path}: textures"
    );
    assert_eq!(
        attributes, expected_attributes,
        "{path}: material attributes"
    );
    assert_eq!(
        render_attributes,
        authored_count(&["m_renderAttributesUsed"]),
        "{path}: render attributes"
    );
    [1, textures, parameters, attributes, render_attributes]
}
