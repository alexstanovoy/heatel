//! Defaults and enum meanings used by imported particle programs, from synthetic KV3.
#[path = "common/kv3.rs"]
mod fixture;
use fixture::{K, kv3_file, s};
use heatel::{
    kv3,
    particle::{Constraint, FloatInput, ParentSet, PreEmission, System, Transform, VectorInput},
};

fn system(fields: Vec<(&'static str, K)>) -> System {
    let root = K::Obj(
        [("_class", s("CParticleSystemDefinition"))]
            .into_iter()
            .chain(fields)
            .collect(),
    );
    System::from_document(&kv3::decode(&kv3_file(&root)).expect("synthetic KV3"))
        .expect("particle system")
}
fn operator(class: &str, fields: Vec<(&'static str, K)>) -> K {
    K::Obj([("_class", s(class))].into_iter().chain(fields).collect())
}
fn distance(fields: Vec<(&'static str, K)>) -> heatel::particle::Distance {
    let system = system(vec![(
        "m_Constraints",
        K::Arr(vec![operator("C_OP_ConstrainDistance", fields)]),
    )]);
    let Constraint::Distance(distance) = &system.constraints[0].class else {
        panic!("distance class")
    };
    *distance.clone()
}

#[test]
fn infinite_bounds_preserves_explicit_values_and_the_finite_default() {
    assert!(!system(Vec::new()).infinite_bounds);
    for value in [false, true] {
        assert_eq!(
            system(vec![("m_bInfiniteBounds", K::Bool(value))]).infinite_bounds,
            value
        );
    }
}

#[test]
fn distance_defaults_keep_the_inner_outer_and_oriented_cp0_contract() {
    let distance = distance(Vec::new());
    assert_eq!(distance.min_distance, FloatInput::Literal(0.0));
    assert_eq!(distance.max_distance, FloatInput::Literal(100.0));
    let Transform::ControlPoint {
        control_point,
        orientation,
        range_max,
    } = distance.transform
    else {
        panic!("CP0 transform")
    };
    assert_eq!(control_point.index(), 0);
    assert!(
        orientation,
        "offsets rotate with the default control-point frame"
    );
    assert_eq!(range_max, None);
    assert_eq!(distance.center_offset, VectorInput::Literal([0.0; 3]));
    assert!(!distance.global_center);
}

#[test]
fn distance_literal_outer_radius_is_at_least_the_inner_radius() {
    for inner in [-10.0, 10.0] {
        let shell = distance(vec![
            ("m_fMinDistance", K::Float(inner)),
            ("m_fMaxDistance", K::Float(5.0)),
        ]);
        assert_eq!(shell.max_distance, FloatInput::Literal(10.0));
    }
    let disabled = self::distance(vec![
        ("m_fMinDistance", K::Float(10.0)),
        ("m_fMaxDistance", K::Float(-1.0)),
    ]);
    assert_eq!(
        disabled.max_distance,
        FloatInput::Literal(-1.0),
        "negative remains the disabled-outer sentinel"
    );
}

#[test]
fn distance_transform_preserves_explicit_orientation_and_control_point() {
    for orientation in [false, true] {
        let shell = distance(vec![(
            "m_nControlPointNumber",
            K::Obj(vec![
                ("m_nControlPoint", K::Int(3)),
                ("m_bUseOrientation", K::Bool(orientation)),
            ]),
        )]);
        let Transform::ControlPoint {
            control_point,
            orientation: actual,
            ..
        } = shell.transform
        else {
            panic!("control-point frame")
        };
        assert_eq!(control_point.index(), 3);
        assert_eq!(actual, orientation);
    }
}

#[test]
fn center_setter_default_is_cp1_in_its_own_tree_without_an_offset_or_average() {
    let system = system(vec![(
        "m_PreEmissionOperators",
        K::Arr(vec![operator("C_OP_SetControlPointToCenter", Vec::new())]),
    )]);
    let PreEmission::SetControlPointToCenter {
        control_point,
        offset,
        average,
        target,
    } = system.pre_emission[0].class
    else {
        panic!("center setter")
    };
    assert_eq!(control_point.index(), 1);
    assert_eq!(offset, [0.0; 3]);
    assert!(!average);
    assert_eq!(target, ParentSet::Own);
}

#[test]
fn center_parent_modes_preserve_numeric_order_and_symbolic_spellings() {
    for (number, text, expected) in [
        (0, "PARTICLE_SET_PARENT_NO", ParentSet::Own),
        (1, "PARTICLE_SET_PARENT_IMMEDIATE", ParentSet::Parent),
        (2, "PARTICLE_SET_PARENT_ROOT", ParentSet::Root),
    ] {
        for value in [K::Int(number), s(text)] {
            let system = system(vec![(
                "m_PreEmissionOperators",
                K::Arr(vec![operator(
                    "C_OP_SetControlPointToCenter",
                    vec![("m_nSetParent", value)],
                )]),
            )]);
            let PreEmission::SetControlPointToCenter { target, .. } = system.pre_emission[0].class
            else {
                panic!("center setter")
            };
            assert_eq!(target, expected, "parent mode {number}/{text}");
        }
    }
}

#[test]
fn sequential_path_v2_reads_its_input_count_and_endpoints() {
    let system = system(vec![(
        "m_Initializers",
        K::Arr(vec![operator(
            "C_INIT_CreateSequentialPathV2",
            vec![
                (
                    "m_flNumToAssign",
                    K::Obj(vec![
                        ("m_nType", s("PF_TYPE_LITERAL")),
                        ("m_flLiteralValue", K::Float(16.0)),
                    ]),
                ),
                (
                    "m_PathParams",
                    K::Obj(vec![
                        ("m_nStartControlPointNumber", K::Int(3)),
                        ("m_nEndControlPointNumber", K::Int(4)),
                    ]),
                ),
            ],
        )]),
    )]);
    let heatel::particle::Initializer::CreateSequentialPath(path) = &system.initializers[0].class
    else {
        panic!("the V2 sequential path is supported, not omitted");
    };
    assert_eq!(path.count, FloatInput::Literal(16.0));
    assert_eq!(path.path.start.index(), 3);
    assert_eq!(path.path.end.index(), 4);
}

#[test]
fn legacy_sequential_path_scalars_are_preserved_as_literal_inputs() {
    let system = system(vec![(
        "m_Initializers",
        K::Arr(vec![operator(
            "C_INIT_CreateSequentialPath",
            vec![
                ("m_flNumToAssign", K::Float(4.0)),
                ("m_fMaxDistance", K::Float(2.0)),
            ],
        )]),
    )]);
    let heatel::particle::Initializer::CreateSequentialPath(path) = &system.initializers[0].class
    else {
        panic!("the legacy sequential path is supported");
    };
    assert_eq!(path.count, FloatInput::Literal(4.0));
    assert_eq!(path.max_distance, FloatInput::Literal(2.0));
}

fn sequential_path(
    class: &str,
    fields: Vec<(&'static str, K)>,
) -> heatel::particle::SequentialPath {
    let system = system(vec![(
        "m_Initializers",
        K::Arr(vec![operator(class, fields)]),
    )]);
    let heatel::particle::Initializer::CreateSequentialPath(path) = &system.initializers[0].class
    else {
        panic!("{class} is a supported sequential path");
    };
    *path.clone()
}

#[test]
fn sequential_path_absent_keys_default_to_a_hundred_points_looped_without_offset() {
    for class in [
        "C_INIT_CreateSequentialPath",
        "C_INIT_CreateSequentialPathV2",
    ] {
        let path = sequential_path(class, Vec::new());
        assert_eq!(path.count, FloatInput::Literal(100.0), "{class} count");
        assert_eq!(
            path.max_distance,
            FloatInput::Literal(0.0),
            "{class} distance"
        );
        assert!(path.looped, "{class} loops");
        assert!(!path.pairs, "{class} pairs");
        assert!(!path.save_offset, "{class} saves no offset");
    }
}

#[test]
fn sequential_path_v2_keeps_input_driven_count_and_distance() {
    let path = sequential_path(
        "C_INIT_CreateSequentialPathV2",
        vec![
            (
                "m_flNumToAssign",
                K::Obj(vec![
                    ("m_nType", s("PF_TYPE_RANDOM_UNIFORM")),
                    ("m_flRandomMin", K::Float(4.0)),
                    ("m_flRandomMax", K::Float(8.0)),
                ]),
            ),
            (
                "m_fMaxDistance",
                K::Obj(vec![
                    ("m_nType", s("PF_TYPE_CONTROL_POINT_COMPONENT")),
                    ("m_nControlPoint", K::Int(2)),
                    ("m_nVectorComponent", K::Int(1)),
                ]),
            ),
        ],
    );
    assert_eq!(
        path.count,
        FloatInput::Random {
            min: 4.0,
            max: 8.0,
            varying: false,
            bias: None,
            sign_flip: false,
        }
    );
    let FloatInput::ControlPointComponent {
        control_point,
        component,
        ..
    } = path.max_distance
    else {
        panic!(
            "distance follows a control point component: {:?}",
            path.max_distance
        );
    };
    assert_eq!(control_point.index(), 2);
    assert_eq!(component, 1);
}

#[test]
fn sequential_path_unsupported_input_kind_is_named_not_read_as_zero() {
    let path = sequential_path(
        "C_INIT_CreateSequentialPathV2",
        vec![(
            "m_flNumToAssign",
            K::Obj(vec![
                ("m_nType", s("PF_TYPE_PARTICLE_DETAIL_LEVEL")),
                ("m_flLiteralValue", K::Float(7.0)),
            ]),
        )],
    );
    assert_eq!(
        path.count,
        FloatInput::Unsupported {
            kind: "PF_TYPE_PARTICLE_DETAIL_LEVEL".to_owned(),
            literal: 7.0,
        }
    );
}

#[test]
fn sequential_path_count_and_distance_pass_through_raw_for_the_consumer_to_clamp() {
    for value in [-3.0_f64, 2.5, 1.0e30] {
        let legacy = sequential_path(
            "C_INIT_CreateSequentialPath",
            vec![
                ("m_flNumToAssign", K::Float(value)),
                ("m_fMaxDistance", K::Float(value)),
            ],
        );
        assert_eq!(legacy.count, FloatInput::Literal(value as f32));
        assert_eq!(legacy.max_distance, FloatInput::Literal(value as f32));
    }
}

#[test]
fn control_point_component_remaps_are_executable_in_the_imported_program() {
    let program = system(vec![(
        "m_PreEmissionOperators",
        K::Arr(vec![operator(
            "C_OP_RemapCPtoCP",
            vec![
                ("m_nInputControlPoint", K::Int(3)),
                ("m_nOutputControlPoint", K::Int(9)),
                ("m_nInputField", K::Int(2)),
                ("m_nOutputField", K::Int(0)),
                ("m_flInputMin", K::Float(-4.0)),
                ("m_flInputMax", K::Float(12.0)),
                ("m_flOutputMin", K::Float(20.0)),
                ("m_flOutputMax", K::Float(100.0)),
                ("m_flInterpRate", K::Float(0.25)),
            ],
        )]),
    )]);
    assert!(
        program
            .unsupported_classes()
            .into_iter()
            .all(|class| class != "C_OP_RemapCPtoCP"),
        "the control-point component remap must be executable, not an unsupported operator"
    );
}

#[test]
fn snapshot_placement_is_an_executable_initializer() {
    let program = system(vec![(
        "m_Initializers",
        K::Arr(vec![operator(
            "C_INIT_InitSkinnedPositionFromCPSnapshot",
            vec![
                ("m_nSnapshotControlPointNumber", K::Int(6)),
                ("m_nSnapShotStartPoint", K::Int(2)),
            ],
        )]),
    )]);
    assert!(
        !program
            .unsupported_classes()
            .contains(&"C_INIT_InitSkinnedPositionFromCPSnapshot")
    );
}

#[test]
fn particle_noise_is_typed_without_a_zero_literal_fallback() {
    let program = system(vec![(
        "m_Initializers",
        K::Arr(vec![operator(
            "C_INIT_InitFloat",
            vec![(
                "m_InputValue",
                K::Obj(vec![
                    ("m_nType", s("PF_TYPE_PARTICLE_NOISE")),
                    ("m_flNoiseOutputMin", K::Float(12.0)),
                    ("m_flNoiseOutputMax", K::Float(24.0)),
                ]),
            )],
        )]),
    )]);
    let heatel::particle::Initializer::InitFloat { value, .. } = &program.initializers[0].class
    else {
        panic!("float initializer")
    };
    assert!(
        !matches!(value, FloatInput::Unsupported { .. }),
        "noise must retain its spatial input instead of becoming zero"
    );
}

#[test]
fn component_remap_keeps_distinct_components_ranges_and_timing() {
    let program = system(vec![(
        "m_PreEmissionOperators",
        K::Arr(vec![operator(
            "C_OP_RemapCPtoCP",
            vec![
                ("m_nInputControlPoint", K::Int(3)),
                ("m_nOutputControlPoint", K::Int(9)),
                ("m_nInputField", K::Int(2)),
                ("m_nOutputField", K::Int(0)),
                ("m_flInputMin", K::Float(-4.0)),
                ("m_flInputMax", K::Float(12.0)),
                ("m_flOutputMin", K::Float(20.0)),
                ("m_flOutputMax", K::Float(100.0)),
                ("m_flInterpRate", K::Float(0.25)),
                ("m_bDerivative", K::Bool(true)),
                ("m_bRunOnce", K::Bool(true)),
            ],
        )]),
    )]);
    let PreEmission::RemapControlPoint(remap) = &program.pre_emission[0].class else {
        panic!("typed remap")
    };
    assert_eq!((remap.input.index(), remap.output.index()), (3, 9));
    assert_eq!(
        (
            remap.input_component.index(),
            remap.output_component.index()
        ),
        (2, 0)
    );
    assert_eq!(
        (remap.input_range, remap.output_range),
        ([-4.0, 12.0], [20.0, 100.0])
    );
    assert_eq!(remap.interpolation, 0.25);
    assert!(remap.derivative);
    assert!(program.pre_emission[0].timing.run_once);
}

#[test]
fn snapshot_and_noise_defaults_keep_resource_and_spatial_controls_distinct() {
    let program = system(vec![
        ("m_hSnapshot", s("particles/test.vsnap")),
        ("m_nSnapshotControlPoint", K::Int(6)),
        (
            "m_Initializers",
            K::Arr(vec![operator(
                "C_INIT_InitSkinnedPositionFromCPSnapshot",
                vec![("m_nSnapshotControlPointNumber", K::Int(6))],
            )]),
        ),
    ]);
    assert_eq!(
        program.snapshot_file.as_deref(),
        Some("particles/test.vsnap")
    );
    assert_eq!(program.snapshot_control_point.index(), 6);
    let heatel::particle::Initializer::SkinnedSnapshot {
        control_point,
        snapshot_control_point,
        start,
        random,
        set_normal,
    } = &program.initializers[0].class
    else {
        panic!("typed snapshot")
    };
    assert_eq!(
        (
            control_point.index(),
            snapshot_control_point.index(),
            *start
        ),
        (0, 6, 0)
    );
    assert!(!random);
    assert!(!set_normal);
}

#[test]
fn hostile_component_counts_and_nonfinite_inputs_are_rejected() {
    for (class, key, value) in [
        ("C_OP_RemapCPtoCP", "m_nInputField", K::Int(3)),
        (
            "C_OP_RemapCPtoCP",
            "m_flInterpRate",
            K::Float(f64::INFINITY),
        ),
    ] {
        let root = K::Obj(vec![(
            "m_PreEmissionOperators",
            K::Arr(vec![operator(class, vec![(key, value)])]),
        )]);
        let doc = kv3::decode(&kv3_file(&root)).expect("synthetic KV3");
        assert!(System::from_document(&doc).is_err(), "{class} {key}");
    }
}

fn read_system(fields: Vec<(&'static str, K)>) -> Result<System, String> {
    let root = K::Obj(
        [("_class", s("CParticleSystemDefinition"))]
            .into_iter()
            .chain(fields)
            .collect(),
    );
    let document = kv3::decode(&kv3_file(&root)).map_err(|e| e.to_string())?;
    System::from_document(&document).map_err(|e| e.to_string())
}
fn noise_initializer(field: &'static str, value: K) -> Vec<(&'static str, K)> {
    let input = K::Obj(vec![
        ("m_nType", s("PF_TYPE_PARTICLE_NOISE")),
        (field, value),
    ]);
    vec![(
        "m_Initializers",
        K::Arr(vec![operator(
            "C_INIT_InitFloat",
            vec![("m_InputValue", input)],
        )]),
    )]
}
fn remap(fields: Vec<(&'static str, K)>) -> Vec<(&'static str, K)> {
    vec![(
        "m_PreEmissionOperators",
        K::Arr(vec![operator("C_OP_RemapCPtoCP", fields)]),
    )]
}

#[test]
fn particle_noise_numbers_that_are_not_finite_or_not_f32_are_errors() {
    for (key, value) in [
        ("m_flNoiseScale", f64::NAN),
        ("m_flNoiseScale", f64::INFINITY),
        ("m_flNoiseOutputMax", f64::NEG_INFINITY),
        ("m_flNoiseOffset", f64::NAN),
        ("m_flNoiseScale", 1e300),
        ("m_flNoiseOutputMin", 3.5e38),
    ] {
        let error = read_system(noise_initializer(key, K::Float(value))).expect_err(key);
        assert!(
            error.contains(&format!("{key} is not a finite number")),
            "{key}={value}: {error}"
        );
    }
}

#[test]
fn particle_noise_octaves_outside_one_to_eight_are_errors() {
    for octaves in [0, 9, -1, 1 << 40] {
        let error = read_system(noise_initializer("m_nNoiseOctaves", K::Int(octaves)))
            .expect_err("octaves");
        assert!(error.contains("m_nNoiseOctaves"), "{octaves}: {error}");
    }
}

#[test]
fn particle_noise_types_the_reader_does_not_evaluate_stay_named_unsupported() {
    for kind in ["PF_NOISE_TYPE_VALUE", "PF_NOISE_TYPE_SIMPLEX"] {
        let system = read_system(noise_initializer("m_nNoiseType", s(kind))).expect("read");
        assert!(
            !matches!(
                &system.initializers[0].class,
                heatel::particle::Initializer::InitFloat {
                    value: FloatInput::ParticleNoise(_),
                    ..
                }
            ),
            "{kind} must not read as Perlin"
        );
    }
}

#[test]
fn control_point_remaps_with_bad_indices_or_interpolation_are_errors() {
    for (key, value, expected) in [
        (
            "m_nInputControlPoint",
            K::Int(64),
            "m_nInputControlPoint 64 is outside -1..=63",
        ),
        (
            "m_nInputControlPoint",
            K::Int(-5),
            "m_nInputControlPoint -5 is outside -1..=63",
        ),
        (
            "m_nInputField",
            K::Int(3),
            "m_nInputField 3 is outside 0..=2",
        ),
        (
            "m_nInputField",
            K::Int(-1),
            "m_nInputField -1 is outside 0..=2",
        ),
        (
            "m_nOutputField",
            K::Int(3),
            "m_nOutputField 3 is outside 0..=2",
        ),
        (
            "m_flInterpRate",
            K::Float(f64::NAN),
            "m_flInterpRate is not a finite number",
        ),
        (
            "m_flInterpRate",
            K::Float(f64::INFINITY),
            "m_flInterpRate is not a finite number",
        ),
        (
            "m_flInterpRate",
            K::Float(1e300),
            "m_flInterpRate is not a finite number",
        ),
    ] {
        let error = read_system(remap(vec![(key, value)])).expect_err(key);
        assert!(error.contains(expected), "{key}: {error}");
    }
}

#[test]
fn self_referential_remaps_are_read_and_a_flood_of_them_is_an_error() {
    let ops = (0..60)
        .map(|i| {
            operator(
                "C_OP_RemapCPtoCP",
                vec![
                    ("m_nInputControlPoint", K::Int(i % 4)),
                    ("m_nOutputControlPoint", K::Int(i % 4)),
                ],
            )
        })
        .collect();
    let system = read_system(vec![("m_PreEmissionOperators", K::Arr(ops))]).expect("60 remaps");
    assert_eq!(system.pre_emission.len(), 60);
    assert!(system.validate().is_ok());
    let flood = (0..100_000)
        .map(|_| operator("C_OP_RemapCPtoCP", vec![]))
        .collect();
    let error = read_system(vec![("m_PreEmissionOperators", K::Arr(flood))]).expect_err("flood");
    assert!(error.contains("more than 128"), "{error}");
}

#[test]
fn snapshot_paths_that_escape_or_are_absolute_are_errors_and_plain_ones_are_kept() {
    for path in [
        "../x.vsnap",
        "/abs",
        "a\\b",
        "c:x",
        "a/../b",
        &"a".repeat(5000),
    ] {
        let error = read_system(vec![("m_hSnapshot", s(path))]).expect_err(path);
        assert!(error.contains("invalid snapshot path"), "{error}");
    }
    let system = read_system(vec![("m_hSnapshot", s("ok/x.vsnap_c"))]).expect("plain path");
    assert_eq!(system.snapshot_file.as_deref(), Some("ok/x.vsnap_c"));
}

#[test]
fn snapshot_start_points_past_the_limit_or_negative_are_errors() {
    let start = |value: i64| {
        read_system(vec![(
            "m_Initializers",
            K::Arr(vec![operator(
                "C_INIT_InitSkinnedPositionFromCPSnapshot",
                vec![("m_nSnapShotStartPoint", K::Int(value))],
            )]),
        )])
    };
    for value in [-1, 5001, i64::MAX] {
        assert!(start(value).is_err(), "{value}");
    }
    assert!(start(5000).expect("limit itself").validate().is_ok());
}

#[test]
fn default_remaps_and_noise_inputs_equal_what_the_reader_gives_for_absent_keys() {
    use heatel::particle::{ControlPointRemap, ParticleNoise};
    let remap = read_system(remap(vec![])).expect("remap");
    assert_eq!(
        remap.pre_emission[0].class,
        PreEmission::RemapControlPoint(Box::<ControlPointRemap>::default())
    );
    let noise = read_system(noise_initializer("m_nNoiseOctaves", K::Int(1))).expect("noise");
    let heatel::particle::Initializer::InitFloat { value, .. } = &noise.initializers[0].class
    else {
        panic!("InitFloat")
    };
    assert_eq!(
        *value,
        FloatInput::ParticleNoise(Box::<ParticleNoise>::default())
    );
}

#[test]
fn renderers_read_their_depth_bias_and_default_to_none() {
    use heatel::particle::Renderer;
    let bias = K::Obj(vec![
        ("m_nType", s("PF_TYPE_LITERAL")),
        ("m_flLiteralValue", K::Float(5.0)),
    ]);
    let renderers = |fields: Vec<(&'static str, K)>| {
        read_system(vec![(
            "m_Renderers",
            K::Arr(vec![operator("C_OP_RenderRopes", fields)]),
        )])
        .expect("renderer")
    };
    let look = |system: &System| match &system.renderers[0].class {
        Renderer::Ropes(ropes) => ropes.look.depth_bias.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        look(&renderers(vec![("m_flDepthBias", bias)])),
        FloatInput::Literal(5.0)
    );
    assert_eq!(look(&renderers(vec![])), FloatInput::Literal(0.0));
}
