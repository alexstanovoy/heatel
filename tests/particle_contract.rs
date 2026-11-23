//! Defaults and enum meanings used by imported particle programs, from synthetic KV3.
#[path = "common/kv3.rs"]
mod fixture;
use fixture::{K, kv3_file, s};
use heatel::{
    kv3,
    particle::{
        Constraint, ControlPoint, ControlPointRemap, EndCap, Field, FloatInput, Initializer,
        Operator, ParentSet, ParticleNoise, PreEmission, Renderer, System, Transform, VectorInput,
    },
};

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

fn system(fields: Vec<(&'static str, K)>) -> System {
    read_system(fields).expect("particle system")
}

fn operator(class: &str, fields: Vec<(&'static str, K)>) -> K {
    K::Obj([("_class", s(class))].into_iter().chain(fields).collect())
}

fn with_op(
    list: &'static str,
    class: &str,
    fields: Vec<(&'static str, K)>,
) -> Vec<(&'static str, K)> {
    vec![(list, K::Arr(vec![operator(class, fields)]))]
}

fn literal_input(value: f64) -> K {
    K::Obj(vec![
        ("m_nType", s("PF_TYPE_LITERAL")),
        ("m_flLiteralValue", K::Float(value)),
    ])
}

fn distance(fields: Vec<(&'static str, K)>) -> heatel::particle::Distance {
    let sys = system(with_op("m_Constraints", "C_OP_ConstrainDistance", fields));
    let Constraint::Distance(distance) = &sys.constraints[0].class else {
        panic!("distance class")
    };
    *distance.clone()
}

fn sequential_path(
    class: &str,
    fields: Vec<(&'static str, K)>,
) -> heatel::particle::SequentialPath {
    let sys = system(with_op("m_Initializers", class, fields));
    let Initializer::CreateSequentialPath(path) = &sys.initializers[0].class else {
        panic!("{class} is a supported sequential path");
    };
    *path.clone()
}

fn noise_initializer(fields: Vec<(&'static str, K)>) -> Vec<(&'static str, K)> {
    let input = K::Obj(
        [("m_nType", s("PF_TYPE_PARTICLE_NOISE"))]
            .into_iter()
            .chain(fields)
            .collect(),
    );
    with_op(
        "m_Initializers",
        "C_INIT_InitFloat",
        vec![("m_InputValue", input)],
    )
}

#[test]
fn rigid_attachment_defaults_and_rejects_invalid_properties() {
    let sys = system(with_op(
        "m_Operators",
        "C_OP_MovementRigidAttachToCP",
        vec![],
    ));
    assert_eq!(
        sys.operators[0].class,
        Operator::RigidAttachToControlPoint {
            control_point: ControlPoint::new(0).unwrap(),
            scale: None,
            input: Field::POSITION_PREVIOUS,
            output: Field::POSITION,
            local: true,
        }
    );
    for (field, value, detail) in [
        ("m_nControlPointNumber", K::Int(64), "64 is outside -1..=63"),
        ("m_nScaleControlPoint", K::Int(-2), "-2 is outside -1..=63"),
        ("m_nScaleCPField", K::Int(3), "3 is outside 0..=2"),
        ("m_nFieldInput", K::Int(-1), "field -1 is not known"),
        ("m_nFieldOutput", K::Int(49), "field 49 is not known"),
        ("m_nControlPointNumber", s("wrong"), "is not an integer"),
        ("m_nScaleControlPoint", s("wrong"), "is not an integer"),
        ("m_nScaleCPField", s("wrong"), "is not an integer"),
        ("m_nFieldInput", s("wrong"), "is not a particle field"),
        ("m_nFieldOutput", s("wrong"), "is not a particle field"),
        ("m_bOffsetLocal", s("wrong"), "is not a boolean"),
    ] {
        let error = read_system(with_op(
            "m_Operators",
            "C_OP_MovementRigidAttachToCP",
            vec![(field, value)],
        ))
        .expect_err("the bad property");
        assert_eq!(error, format!("vpcf: {field} {detail}"));
    }
}

#[test]
fn infinite_bounds_preserves_explicit_values_and_the_finite_default() {
    assert!(!system(vec![]).infinite_bounds);
    for value in [false, true] {
        assert_eq!(
            system(vec![("m_bInfiniteBounds", K::Bool(value))]).infinite_bounds,
            value
        );
    }
}

#[test]
fn distance_defaults_radii_and_transform_contract() {
    let d = distance(vec![]);
    assert_eq!(d.min_distance, FloatInput::Literal(0.0));
    assert_eq!(d.max_distance, FloatInput::Literal(100.0));
    assert_eq!(
        d.transform,
        Transform::ControlPoint {
            control_point: ControlPoint::new(0).unwrap(),
            orientation: true,
            range_max: None,
        }
    );
    assert_eq!(d.center_offset, VectorInput::Literal([0.0; 3]));
    assert!(!d.global_center);

    for (inner, outer, expected) in [(-10.0, 5.0, 10.0), (10.0, 5.0, 10.0), (10.0, -1.0, -1.0)] {
        let shell = distance(vec![
            ("m_fMinDistance", K::Float(inner)),
            ("m_fMaxDistance", K::Float(outer)),
        ]);
        assert_eq!(shell.max_distance, FloatInput::Literal(expected));
    }

    for orientation in [false, true] {
        let shell = distance(vec![(
            "m_nControlPointNumber",
            K::Obj(vec![
                ("m_nControlPoint", K::Int(3)),
                ("m_bUseOrientation", K::Bool(orientation)),
            ]),
        )]);
        assert_eq!(
            shell.transform,
            Transform::ControlPoint {
                control_point: ControlPoint::new(3).unwrap(),
                orientation,
                range_max: None,
            }
        );
    }
}

#[test]
fn center_setter_defaults_and_parent_modes() {
    let sys = system(with_op(
        "m_PreEmissionOperators",
        "C_OP_SetControlPointToCenter",
        vec![],
    ));
    assert_eq!(
        sys.pre_emission[0].class,
        PreEmission::SetControlPointToCenter {
            control_point: ControlPoint::new(1).unwrap(),
            offset: [0.0; 3],
            average: false,
            target: ParentSet::Own,
        }
    );
    for (number, text, expected) in [
        (0, "PARTICLE_SET_PARENT_NO", ParentSet::Own),
        (1, "PARTICLE_SET_PARENT_IMMEDIATE", ParentSet::Parent),
        (2, "PARTICLE_SET_PARENT_ROOT", ParentSet::Root),
    ] {
        for value in [K::Int(number), s(text)] {
            let sys = system(with_op(
                "m_PreEmissionOperators",
                "C_OP_SetControlPointToCenter",
                vec![("m_nSetParent", value)],
            ));
            let PreEmission::SetControlPointToCenter { target, .. } = sys.pre_emission[0].class
            else {
                panic!("center setter")
            };
            assert_eq!(target, expected, "parent mode {number}/{text}");
        }
    }
}

#[test]
fn sequential_path_v1_and_v2_defaults_inputs_and_raw_passthrough() {
    for class in [
        "C_INIT_CreateSequentialPath",
        "C_INIT_CreateSequentialPathV2",
    ] {
        let path = sequential_path(class, vec![]);
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

    let v2 = sequential_path(
        "C_INIT_CreateSequentialPathV2",
        vec![
            ("m_flNumToAssign", literal_input(16.0)),
            (
                "m_PathParams",
                K::Obj(vec![
                    ("m_nStartControlPointNumber", K::Int(3)),
                    ("m_nEndControlPointNumber", K::Int(4)),
                ]),
            ),
        ],
    );
    assert_eq!(v2.count, FloatInput::Literal(16.0));
    assert_eq!((v2.path.start.index(), v2.path.end.index()), (3, 4));

    let legacy = sequential_path(
        "C_INIT_CreateSequentialPath",
        vec![
            ("m_flNumToAssign", K::Float(4.0)),
            ("m_fMaxDistance", K::Float(2.0)),
        ],
    );
    assert_eq!(legacy.count, FloatInput::Literal(4.0));
    assert_eq!(legacy.max_distance, FloatInput::Literal(2.0));

    let driven = sequential_path(
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
        driven.count,
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
    } = driven.max_distance
    else {
        panic!("expected ControlPointComponent: {:?}", driven.max_distance);
    };
    assert_eq!((control_point.index(), component), (2, 1));

    let unsupported = sequential_path(
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
        unsupported.count,
        FloatInput::Unsupported {
            kind: "PF_TYPE_PARTICLE_DETAIL_LEVEL".to_owned(),
            literal: 7.0,
        }
    );

    for value in [-3.0_f64, 2.5, 1.0e30] {
        let p = sequential_path(
            "C_INIT_CreateSequentialPath",
            vec![
                ("m_flNumToAssign", K::Float(value)),
                ("m_fMaxDistance", K::Float(value)),
            ],
        );
        assert_eq!(p.count, FloatInput::Literal(value as f32));
        assert_eq!(p.max_distance, FloatInput::Literal(value as f32));
    }
}

#[test]
fn component_remap_fields_defaults_validation_and_flood_limits() {
    let program = system(with_op(
        "m_PreEmissionOperators",
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
    ));
    assert!(!program.unsupported_classes().contains(&"C_OP_RemapCPtoCP"));
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

    let default_remap = system(with_op(
        "m_PreEmissionOperators",
        "C_OP_RemapCPtoCP",
        vec![],
    ));
    assert_eq!(
        default_remap.pre_emission[0].class,
        PreEmission::RemapControlPoint(Box::<ControlPointRemap>::default())
    );

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
        let error = read_system(with_op(
            "m_PreEmissionOperators",
            "C_OP_RemapCPtoCP",
            vec![(key, value)],
        ))
        .expect_err(key);
        assert!(error.contains(expected), "{key}: {error}");
    }

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
    let self_ref = system(vec![("m_PreEmissionOperators", K::Arr(ops))]);
    assert_eq!(self_ref.pre_emission.len(), 60);
    assert!(self_ref.validate().is_ok());

    let flood = (0..100_000)
        .map(|_| operator("C_OP_RemapCPtoCP", vec![]))
        .collect();
    let error = read_system(vec![("m_PreEmissionOperators", K::Arr(flood))]).expect_err("flood");
    assert!(error.contains("more than 128"), "{error}");
}

#[test]
fn snapshot_placement_paths_and_start_point_bounds() {
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
    assert!(
        !program
            .unsupported_classes()
            .contains(&"C_INIT_InitSkinnedPositionFromCPSnapshot")
    );
    assert_eq!(
        program.snapshot_file.as_deref(),
        Some("particles/test.vsnap")
    );
    assert_eq!(program.snapshot_control_point.index(), 6);
    let Initializer::SkinnedSnapshot {
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
    assert!(!random && !set_normal);

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
    assert_eq!(
        system(vec![("m_hSnapshot", s("ok/x.vsnap_c"))])
            .snapshot_file
            .as_deref(),
        Some("ok/x.vsnap_c")
    );

    let start_res = |value: i64| {
        read_system(with_op(
            "m_Initializers",
            "C_INIT_InitSkinnedPositionFromCPSnapshot",
            vec![("m_nSnapShotStartPoint", K::Int(value))],
        ))
    };
    for value in [-1, 5001, i64::MAX] {
        assert!(start_res(value).is_err(), "{value}");
    }
    assert!(start_res(5000).expect("limit itself").validate().is_ok());
}

#[test]
fn particle_noise_typing_defaults_and_validation() {
    let program = system(noise_initializer(vec![
        ("m_flNoiseOutputMin", K::Float(12.0)),
        ("m_flNoiseOutputMax", K::Float(24.0)),
    ]));
    let Initializer::InitFloat { value, .. } = &program.initializers[0].class else {
        panic!("float initializer")
    };
    assert!(!matches!(value, FloatInput::Unsupported { .. }));

    let default_noise = system(noise_initializer(vec![("m_nNoiseOctaves", K::Int(1))]));
    let Initializer::InitFloat { value, .. } = &default_noise.initializers[0].class else {
        panic!("InitFloat")
    };
    assert_eq!(
        *value,
        FloatInput::ParticleNoise(Box::<ParticleNoise>::default())
    );

    for (key, value) in [
        ("m_flNoiseScale", f64::NAN),
        ("m_flNoiseScale", f64::INFINITY),
        ("m_flNoiseOutputMax", f64::NEG_INFINITY),
        ("m_flNoiseOffset", f64::NAN),
        ("m_flNoiseScale", 1e300),
        ("m_flNoiseOutputMin", 3.5e38),
    ] {
        let error = read_system(noise_initializer(vec![(key, K::Float(value))])).expect_err(key);
        assert!(
            error.contains(&format!("{key} is not a finite number")),
            "{key}={value}: {error}"
        );
    }

    for octaves in [0, 9, -1, 1 << 40] {
        let error = read_system(noise_initializer(vec![(
            "m_nNoiseOctaves",
            K::Int(octaves),
        )]))
        .expect_err("octaves");
        assert!(error.contains("m_nNoiseOctaves"), "{octaves}: {error}");
    }

    for kind in ["PF_NOISE_TYPE_VALUE", "PF_NOISE_TYPE_SIMPLEX"] {
        let sys = system(noise_initializer(vec![("m_nNoiseType", s(kind))]));
        assert!(
            !matches!(
                &sys.initializers[0].class,
                Initializer::InitFloat {
                    value: FloatInput::ParticleNoise(_),
                    ..
                }
            ),
            "{kind} must not read as Perlin"
        );
    }
}

#[test]
fn renderer_depth_bias_and_end_cap_states() {
    let look = |fields: Vec<(&'static str, K)>| {
        let sys = system(with_op("m_Renderers", "C_OP_RenderRopes", fields));
        match &sys.renderers[0].class {
            Renderer::Ropes(ropes) => ropes.look.depth_bias.clone(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(
        look(vec![("m_flDepthBias", literal_input(5.0))]),
        FloatInput::Literal(5.0)
    );
    assert_eq!(look(vec![]), FloatInput::Literal(0.0));

    let end_cap = |state: K| {
        read_system(with_op(
            "m_Operators",
            "C_OP_Decay",
            vec![("m_nOpEndCapState", state)],
        ))
        .map(|sys| sys.operators[0].timing.end_cap)
    };
    for (input, expected) in [
        (s(""), EndCap::Always),
        (s("PARTICLE_ENDCAP_ENDCAP_ON"), EndCap::On),
        (K::Int(i64::MIN), EndCap::Always),
        (K::Int(i64::MAX), EndCap::On),
    ] {
        assert_eq!(end_cap(input), Ok(expected));
    }
    let refused = end_cap(s("PARTICLE_ENDCAP_SOMETIMES")).expect_err("an unknown name");
    assert!(
        refused.contains("m_nOpEndCapState \"PARTICLE_ENDCAP_SOMETIMES\" is not known"),
        "{refused}"
    );
}
