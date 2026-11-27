use crate::error::Result;
use crate::kv3::{Document, Object, Value};

use super::{
    EndCap, Field, FloatInput, Initializer, Mapping, MissingParentBehavior, Operator, PreEmission,
    SetMethod, SnapshotIndexType, System, VectorComponent,
};

const OPERATORS: &str = "m_Operators";
const INITIALIZERS: &str = "m_Initializers";
const PRE_EMISSION: &str = "m_PreEmissionOperators";

fn read(stage: &str, class: &str, members: &[(&str, Value)]) -> Result<System> {
    let mut function = Object::from([("_class", class)]);
    for (key, value) in members {
        function.insert(*key, value.clone());
    }
    let root = Object::from([(stage, Value::from([function]))]);
    System::from_document(&Document {
        format: [0; 16],
        root: root.into(),
    })
}

#[test]
fn target_classes_decode_through_their_particle_stages() {
    let targets = [
        (OPERATORS, "C_OP_OscillateScalar"),
        (OPERATORS, "C_OP_FadeIn"),
        (OPERATORS, "C_OP_InheritFromParentParticlesV2"),
        (OPERATORS, "C_OP_SnapshotRigidSkinToBones"),
        (OPERATORS, "C_OP_SnapshotSkinToBones"),
        (OPERATORS, "C_OP_DistanceCull"),
        (OPERATORS, "C_OP_LerpScalar"),
        (OPERATORS, "C_OP_SetControlPointsToModelParticles"),
        (OPERATORS, "C_OP_RemapCPtoScalar"),
        (INITIALIZERS, "C_INIT_InitSkinnedPositionFromCPSnapshot"),
        (INITIALIZERS, "C_INIT_InitFromCPSnapshot"),
        (INITIALIZERS, "C_INIT_SequenceLifeTime"),
        (INITIALIZERS, "C_INIT_NormalOffset"),
        (INITIALIZERS, "C_INIT_RandomNamedModelSequence"),
        (PRE_EMISSION, "C_OP_SetControlPointOrientation"),
        (PRE_EMISSION, "C_OP_RemapSpeedtoCP"),
    ];
    let mut unsupported = Vec::new();
    for (stage, class) in targets {
        let system = read(stage, class, &[]).expect(class);
        if !system.unsupported_classes().is_empty() {
            unsupported.push(class);
        }
        #[cfg(feature = "serde")]
        {
            let cached = serde_json::to_vec(&system).expect(class);
            assert_eq!(
                serde_json::from_slice::<System>(&cached).expect(class),
                system
            );
        }
    }
    assert!(unsupported.is_empty(), "{unsupported:?}");
}

fn operator(class: &str, members: &[(&str, Value)]) -> Operator {
    read(OPERATORS, class, members)
        .expect(class)
        .operators
        .remove(0)
        .class
}

fn initializer(class: &str, members: &[(&str, Value)]) -> Initializer {
    read(INITIALIZERS, class, members)
        .expect(class)
        .initializers
        .remove(0)
        .class
}

fn cp_component(control_point: i32, component: i32) -> Value {
    Object::from([
        ("m_nType", Value::from("PF_TYPE_CONTROL_POINT_COMPONENT")),
        ("m_nControlPoint", control_point.into()),
        ("m_nVectorComponent", component.into()),
    ])
    .into()
}

fn literal(value: f32) -> Value {
    Object::from([
        ("m_nType", Value::from("PF_TYPE_LITERAL")),
        ("m_flLiteralValue", value.into()),
    ])
    .into()
}

#[test]
fn scalar_oscillation_keeps_ranges_phase_and_operator_timing() {
    let system = read(
        OPERATORS,
        "C_OP_OscillateScalar",
        &[
            ("m_nField", 16.into()),
            ("m_RateMin", (-5.0).into()),
            ("m_RateMax", 7.0.into()),
            ("m_FrequencyMin", 0.25.into()),
            ("m_FrequencyMax", 3.0.into()),
            ("m_flOscMult", 4.0.into()),
            ("m_flOscAdd", 0.75.into()),
            ("m_bProportional", false.into()),
            ("m_bProportionalOp", false.into()),
            ("m_flStartTime_min", (-1.0).into()),
            ("m_flStartTime_max", 0.5.into()),
            ("m_flEndTime_min", 8.0.into()),
            ("m_flEndTime_max", 10_000_000_000.0.into()),
            ("m_flOpTimeOffsetMin", 1.0.into()),
            ("m_flOpTimeOffsetMax", 8.0.into()),
            ("m_nOpTimeOffsetSeed", 42.into()),
            ("m_flOpTimeScaleMin", 2.0.into()),
            ("m_flOpTimeScaleMax", 8.0.into()),
            ("m_nOpTimeScaleSeed", 34234.into()),
            ("m_bNormalizeToStopTime", true.into()),
            ("m_nOpEndCapState", "PARTICLE_ENDCAP_ENDCAP_OFF".into()),
            ("m_flOpStrength", literal(0.5)),
        ],
    )
    .expect("scalar oscillator");
    let function = &system.operators[0];
    let Operator::OscillateScalar(oscillator) = &function.class else {
        panic!("scalar oscillator");
    };
    assert_eq!(oscillator.field, Field::ALPHA_ALTERNATE);
    assert_eq!(oscillator.rate, [-5.0, 7.0]);
    assert_eq!(oscillator.frequency, [0.25, 3.0]);
    assert_eq!((oscillator.multiplier, oscillator.phase), (4.0, 0.75));
    assert!(!oscillator.proportional && !oscillator.proportional_window);
    assert_eq!(oscillator.start, [-1.0, 0.5]);
    assert_eq!(oscillator.end, [8.0, 10_000_000_000.0]);
    assert_eq!(oscillator.fade_time.offset, [1.0, 8.0]);
    assert_eq!(oscillator.fade_time.offset_seed, 42);
    assert_eq!(oscillator.fade_time.scale, [2.0, 8.0]);
    assert_eq!(oscillator.fade_time.scale_seed, 34234);
    assert!(oscillator.fade_time.normalize_to_stop_time);
    assert_eq!(function.timing.end_cap, EndCap::Off);
    assert_eq!(function.timing.strength, FloatInput::Literal(0.5));
}

#[test]
fn fade_in_keeps_random_duration_and_offset() {
    let Operator::FadeIn {
        time,
        exponent,
        proportional,
        fade_time,
    } = operator(
        "C_OP_FadeIn",
        &[
            ("m_flFadeInTimeMin", 0.0.into()),
            ("m_flFadeInTimeMax", 0.5.into()),
            ("m_flFadeInTimeExp", 2.0.into()),
            ("m_bProportional", false.into()),
            ("m_flOpTimeOffsetMax", 1.0.into()),
        ],
    )
    else {
        panic!("fade in")
    };
    assert_eq!(time, [0.0, 0.5]);
    assert_eq!(exponent, 2.0);
    assert!(!proportional);
    assert_eq!(fade_time.offset, [0.0, 1.0]);
}

#[test]
fn parent_inheritance_reads_literal_and_input_forms_and_missing_behavior() {
    let class = "C_OP_InheritFromParentParticlesV2";
    for (value, expected) in [
        (
            "MISSING_PARENT_DO_NOTHING".into(),
            MissingParentBehavior::DoNothing,
        ),
        (
            "MISSING_PARENT_FIND_NEW".into(),
            MissingParentBehavior::FindNew,
        ),
        ("0".into(), MissingParentBehavior::Kill),
        (2.into(), MissingParentBehavior::SameIndex),
    ] {
        let Operator::InheritFromParentParticlesV2(inheritance) = operator(
            class,
            &[
                ("m_nFieldOutput", 6.into()),
                ("m_flScale", 2.0.into()),
                ("m_nIncrement", literal(0.5)),
                ("m_flInterpolation", cp_component(5, 2)),
                ("m_bSubSample", true.into()),
                ("m_bRandomDistribution", true.into()),
                ("m_bReverse", true.into()),
                ("m_nMissingParentBehavior", value),
            ],
        ) else {
            panic!("parent inheritance")
        };
        assert_eq!(inheritance.field, Field::COLOR);
        assert_eq!(inheritance.scale, FloatInput::Literal(2.0));
        assert_eq!(inheritance.increment, FloatInput::Literal(0.5));
        let FloatInput::ControlPointComponent {
            control_point,
            component,
            mapping,
        } = inheritance.interpolation
        else {
            panic!("interpolation input")
        };
        assert_eq!(
            (control_point.index(), component, mapping),
            (5, 2, Mapping::Direct)
        );
        assert!(inheritance.subsample && inheritance.random && inheritance.reverse);
        assert_eq!(inheritance.missing_parent, expected);
    }
    let Operator::InheritFromParentParticlesV2(legacy) = operator(
        class,
        &[("m_nIncrement", 4.into()), ("m_flScale", literal(3.0))],
    ) else {
        panic!("integer increment and object scale")
    };
    assert_eq!(legacy.increment, FloatInput::Literal(4.0));
    assert_eq!(legacy.scale, FloatInput::Literal(3.0));
}

#[test]
fn snapshot_skinning_operators_keep_transforms_and_fade_parameters() {
    let common = [
        ("m_nControlPointNumber", 6.into()),
        ("m_bTransformNormals", true.into()),
        ("m_bTransformRadii", true.into()),
    ];
    let Operator::SnapshotRigidSkinToBones {
        control_point,
        transform_normals,
        transform_radii,
    } = operator("C_OP_SnapshotRigidSkinToBones", &common)
    else {
        panic!("rigid bone skinning")
    };
    assert_eq!(control_point.index(), 6);
    assert!(transform_normals && transform_radii);

    let mut members = common.to_vec();
    members.extend([
        ("m_flLifeTimeFadeStart", (-1.0).into()),
        ("m_flLifeTimeFadeEnd", 2.0.into()),
        ("m_flJumpThreshold", 256.0.into()),
        ("m_flPrevPosScale", 0.75.into()),
    ]);
    let Operator::SnapshotSkinToBones {
        control_point,
        transform_normals,
        transform_radii,
        lifetime_fade,
        jump_threshold,
        previous_scale,
    } = operator("C_OP_SnapshotSkinToBones", &members)
    else {
        panic!("bone skinning")
    };
    assert_eq!(control_point.index(), 6);
    assert!(transform_normals && transform_radii);
    assert_eq!(lifetime_fade, [-1.0, 2.0]);
    assert_eq!(jump_threshold, 256.0);
    assert_eq!(previous_scale, 0.75);
}

#[test]
fn distance_cull_keeps_its_dynamic_threshold_and_point() {
    let class = "C_OP_DistanceCull";
    let Operator::DistanceCull {
        control_point,
        offset,
        distance,
        inside,
        field,
    } = operator(
        class,
        &[
            ("m_nControlPoint", 4.into()),
            ("m_vecPointOffset", [0.0, 0.0, 64.0].into()),
            ("m_flDistance", cp_component(6, 0)),
            ("m_bCullInside", true.into()),
            ("m_nAttribute", 2.into()),
        ],
    )
    else {
        panic!("distance cull")
    };
    assert_eq!(control_point.index(), 4);
    assert_eq!(offset, [0.0, 0.0, 64.0]);
    let FloatInput::ControlPointComponent {
        control_point,
        component,
        mapping,
    } = distance
    else {
        panic!("distance input")
    };
    assert_eq!(
        (control_point.index(), component, mapping),
        (6, 0, Mapping::Direct)
    );
    assert!(inside);
    assert_eq!(field, Field::POSITION_PREVIOUS);
    assert!(matches!(
        operator(class, &[("m_flDistance", 256.0.into())]),
        Operator::DistanceCull {
            distance: FloatInput::Literal(256.0),
            ..
        }
    ));
}

#[test]
fn scalar_lerp_keeps_target_input_and_window() {
    let Operator::LerpScalar {
        field,
        target,
        time,
    } = operator(
        "C_OP_LerpScalar",
        &[
            ("m_nFieldOutput", 10.into()),
            ("m_flOutput", literal(1.25)),
            ("m_flStartTime", 0.25.into()),
            ("m_flEndTime", 0.75.into()),
        ],
    )
    else {
        panic!("scalar lerp")
    };
    assert_eq!(field, Field::TRAIL_LENGTH);
    assert_eq!(target, FloatInput::Literal(1.25));
    assert_eq!(time, [0.25, 0.75]);
}

#[test]
fn model_particle_control_points_keep_the_selection_and_attachment() {
    let Operator::SetControlPointsToModelParticles(points) = operator(
        "C_OP_SetControlPointsToModelParticles",
        &[
            ("m_nFirstControlPoint", 6.into()),
            ("m_nNumControlPoints", 3.into()),
            ("m_nFirstSourcePoint", 7.into()),
            ("m_HitboxSetName", "head".into()),
            ("m_AttachmentName", "attach_hat_02".into()),
            ("m_bSkin", true.into()),
            ("m_bAttachment", true.into()),
        ],
    ) else {
        panic!("model particle control points")
    };
    assert_eq!(
        (
            points.first_control_point.index(),
            points.count,
            points.first_source
        ),
        (6, 3, 7)
    );
    assert_eq!(points.hitbox_set, "head");
    assert_eq!(points.attachment_name, "attach_hat_02");
    assert!(points.skin && points.attachment);
}

#[test]
fn control_point_remap_keeps_component_window_and_set_method() {
    let Operator::RemapCpToScalar(remap) = operator(
        "C_OP_RemapCPtoScalar",
        &[
            ("m_nCPInput", 2.into()),
            ("m_nField", 1.into()),
            ("m_nFieldOutput", 7.into()),
            ("m_flInputMin", 30.0.into()),
            ("m_flInputMax", 255.0.into()),
            ("m_flOutputMin", 0.25.into()),
            ("m_flOutputMax", 0.75.into()),
            ("m_flEndTime", 20.0.into()),
            ("m_flInterpRate", 0.5.into()),
            ("m_nSetMethod", "PARTICLE_SET_SCALE_CURRENT_VALUE".into()),
        ],
    ) else {
        panic!("control-point remap")
    };
    assert_eq!(remap.control_point.index(), 2);
    assert_eq!(remap.component, VectorComponent::Y);
    assert_eq!(remap.field, Field::ALPHA);
    assert_eq!(remap.input_range, [30.0, 255.0]);
    assert_eq!(remap.output_range, [0.25, 0.75]);
    assert_eq!(remap.time, [-1.0, 20.0]);
    assert_eq!(remap.interpolation, 0.5);
    assert_eq!(remap.set_method, SetMethod::ScaleCurrent);
}

#[test]
fn skinned_snapshot_keeps_legacy_shape_and_reads_extended_parameters() {
    let class = "C_INIT_InitSkinnedPositionFromCPSnapshot";
    assert!(matches!(
        initializer(class, &[]),
        Initializer::SkinnedSnapshot { .. }
    ));
    let Initializer::SkinnedSnapshotWithOptions(snapshot) = initializer(
        class,
        &[
            ("m_nControlPointNumber", 4.into()),
            ("m_nSnapshotControlPointNumber", 6.into()),
            ("m_nSnapShotStartPoint", 9.into()),
            ("m_nRandomSeed", 2.into()),
            ("m_bRandom", true.into()),
            ("m_bRigid", true.into()),
            ("m_bRigidRotationLock", true.into()),
            ("m_bSetNormal", true.into()),
            ("m_bSetRadius", true.into()),
            ("m_bCopyColor", true.into()),
            ("m_bCopyAlpha", true.into()),
            ("m_nIndexType", "SNAPSHOT_INDEX_DIRECT".into()),
            ("m_flReadIndex", literal(2.0)),
            ("m_flIncrement", 2.5.into()),
            ("m_nFullLoopIncrement", 3.into()),
            ("m_flMinNormalVelocity", 10.0.into()),
            ("m_flMaxNormalVelocity", 50.0.into()),
            ("m_bIgnoreDt", true.into()),
            ("m_flBoneVelocity", 0.25.into()),
            ("m_flBoneVelocityMax", 100.0.into()),
        ],
    ) else {
        panic!("extended skinned snapshot")
    };
    assert_eq!(snapshot.control_point.index(), 4);
    assert_eq!(snapshot.snapshot_control_point.index(), 6);
    assert_eq!(snapshot.start, 9);
    assert!(
        snapshot.random && snapshot.rigid && snapshot.rigid_rotation_lock && snapshot.ignore_dt
    );
    assert!(
        snapshot.set_normal && snapshot.set_radius && snapshot.copy_color && snapshot.copy_alpha
    );
    assert_eq!(snapshot.random_seed, 2);
    assert_eq!(snapshot.index_type, SnapshotIndexType::Direct);
    assert_eq!(snapshot.read_index, FloatInput::Literal(2.0));
    assert_eq!((snapshot.increment, snapshot.full_loop_increment), (2.5, 3));
    assert_eq!(snapshot.normal_velocity, [10.0, 50.0]);
    assert_eq!(
        (snapshot.bone_velocity, snapshot.max_bone_velocity),
        (0.25, 100.0)
    );
    assert!(matches!(initializer(class, &[("m_nRandomSeed", 2.into())]),
        Initializer::SkinnedSnapshotWithOptions(settings) if !settings.rigid && settings.random_seed == 2));
}

#[test]
fn snapshot_attribute_keeps_source_destination_and_index_inputs() {
    let class = "C_INIT_InitFromCPSnapshot";
    let Initializer::InitFromCpSnapshot(snapshot) = initializer(
        class,
        &[
            ("m_nControlPointNumber", 6.into()),
            ("m_strSnapshotSubset", "chunk".into()),
            ("m_nAttributeToRead", 6.into()),
            ("m_nAttributeToWrite", 17.into()),
            ("m_nLocalSpaceCP", (-1).into()),
            ("m_bLocalSpaceAngles", true.into()),
            ("m_bRandom", true.into()),
            ("m_nRandomSeed", 5.into()),
            ("m_bReverse", true.into()),
            ("m_nSnapShotIncrement", literal(110.0)),
            ("m_bResetSnapshotIndexOnChanges", true.into()),
            ("m_nManualSnapshotIndex", 4.into()),
        ],
    ) else {
        panic!("snapshot attribute")
    };
    assert_eq!(
        snapshot.control_point.map(super::ControlPoint::index),
        Some(6)
    );
    assert_eq!(snapshot.subset, "chunk");
    assert_eq!(snapshot.input, Some(Field::COLOR));
    assert_eq!(snapshot.output, Field::SCRATCH_VECTOR);
    assert_eq!(snapshot.local_control_point, None);
    assert!(
        snapshot.local_space_angles
            && snapshot.random
            && snapshot.reverse
            && snapshot.reset_on_change
    );
    assert_eq!(snapshot.random_seed, 5);
    assert_eq!(snapshot.increment, FloatInput::Literal(110.0));
    assert_eq!(snapshot.manual_index, FloatInput::Literal(4.0));
    let Initializer::InitFromCpSnapshot(defaults) = initializer(class, &[]) else {
        panic!("snapshot defaults")
    };
    assert_eq!(defaults.input, None);
    assert_eq!(defaults.increment, FloatInput::Literal(1.0));
}

#[test]
fn sequence_lifetime_uses_authored_or_default_framerate() {
    let class = "C_INIT_SequenceLifeTime";
    assert_eq!(
        initializer(class, &[]),
        Initializer::SequenceLifetime { framerate: 30.0 }
    );
    assert_eq!(
        initializer(class, &[("m_flFramerate", 24.0.into())]),
        Initializer::SequenceLifetime { framerate: 24.0 }
    );
}

#[test]
fn normal_offset_keeps_both_vectors_and_the_local_frame() {
    let Initializer::NormalOffset {
        offset,
        control_point,
        local,
        normalize,
    } = initializer(
        "C_INIT_NormalOffset",
        &[
            ("m_OffsetMin", [-1.0, 2.0, -5.0].into()),
            ("m_OffsetMax", [1.0, 4.0, 5.0].into()),
            ("m_nControlPointNumber", 10.into()),
            ("m_bLocalCoords", true.into()),
            ("m_bNormalize", true.into()),
        ],
    )
    else {
        panic!("normal offset")
    };
    assert_eq!(offset, [[-1.0, 2.0, -5.0], [1.0, 4.0, 5.0]]);
    assert_eq!(control_point.index(), 10);
    assert!(local && normalize);
}

#[test]
fn named_model_sequence_keeps_authored_order_and_model_source() {
    let Initializer::RandomNamedModelSequence(sequence) = initializer(
        "C_INIT_RandomNamedModelSequence",
        &[
            ("m_hModel", "models/test.vmdl".into()),
            ("m_names", ["spike_9frame", "spike_15frame"].into()),
            ("m_bModelFromRenderer", true.into()),
            ("m_bShuffle", true.into()),
            ("m_bLinear", true.into()),
            ("m_nFieldOutput", 13.into()),
        ],
    ) else {
        panic!("named model sequence")
    };
    assert_eq!(sequence.model.as_deref(), Some("models/test.vmdl"));
    assert_eq!(sequence.names, ["spike_9frame", "spike_15frame"]);
    assert_eq!(sequence.field, Field::SECOND_SEQUENCE_NUMBER);
    assert!(sequence.model_from_renderer && sequence.shuffle && sequence.linear);
}

#[test]
fn control_point_orientation_keeps_angles_frame_and_run_flags() {
    let system = read(
        PRE_EMISSION,
        "C_OP_SetControlPointOrientation",
        &[
            ("m_nCP", 4.into()),
            ("m_nHeadLocation", 2.into()),
            ("m_vecRotation", [-45.0, 0.0, 0.0].into()),
            ("m_vecRotationB", [90.0, 360.0, 6.0].into()),
            ("m_flInterpolation", literal(0.0)),
            ("m_bUseWorldLocation", true.into()),
            ("m_bRandomize", true.into()),
            ("m_bSetOnce", true.into()),
            ("m_bRunOnce", true.into()),
        ],
    )
    .expect("control point orientation");
    let function = &system.pre_emission[0];
    let PreEmission::SetControlPointOrientation {
        control_point,
        head,
        rotation,
        interpolation,
        world,
        randomize,
        set_once,
    } = &function.class
    else {
        panic!("control point orientation")
    };
    assert_eq!((control_point.index(), head.index()), (4, 2));
    assert_eq!(rotation, &[[-45.0, 0.0, 0.0], [90.0, 360.0, 6.0]]);
    assert_eq!(interpolation, &FloatInput::Literal(0.0));
    assert!(*world && *randomize && *set_once && function.timing.run_once);
}

#[test]
fn speed_remap_keeps_both_control_points_and_delta_mode() {
    let class = "C_OP_RemapSpeedtoCP";
    let system = read(
        PRE_EMISSION,
        class,
        &[
            ("m_nInControlPointNumber", 3.into()),
            ("m_nOutControlPointNumber", 6.into()),
            ("m_nField", 1.into()),
            ("m_flInputMin", 50.0.into()),
            ("m_flInputMax", 150.0.into()),
            ("m_flOutputMin", 0.25.into()),
            ("m_flOutputMax", 1.5.into()),
            ("m_bUseDeltaV", true.into()),
            ("m_flOpTimeOffsetMax", 1.0.into()),
        ],
    )
    .expect("speed remap");
    let PreEmission::RemapSpeedToCp(remap) = &system.pre_emission[0].class else {
        panic!("speed remap")
    };
    assert_eq!(remap.input.index(), 3);
    assert_eq!(remap.output.map(super::ControlPoint::index), Some(6));
    assert_eq!(remap.component, VectorComponent::Y);
    assert_eq!(remap.input_range, [50.0, 150.0]);
    assert_eq!(remap.output_range, [0.25, 1.5]);
    assert!(remap.use_delta_velocity);
    assert_eq!(remap.fade_time.offset, [0.0, 1.0]);

    let system = read(PRE_EMISSION, class, &[]).expect("speed remap defaults");
    assert!(
        matches!(&system.pre_emission[0].class, PreEmission::RemapSpeedToCp(remap) if remap.output.is_none())
    );
}

#[test]
fn newly_read_fields_reject_invalid_types_and_bounds() {
    let cases = [
        (
            OPERATORS,
            "C_OP_InheritFromParentParticlesV2",
            "m_nMissingParentBehavior",
            "bad".into(),
            "m_nMissingParentBehavior",
        ),
        (
            OPERATORS,
            "C_OP_SetControlPointsToModelParticles",
            "m_nNumControlPoints",
            65.into(),
            "m_nNumControlPoints 65 is outside 0..=64",
        ),
        (
            OPERATORS,
            "C_OP_OscillateScalar",
            "m_RateMin",
            f32::INFINITY.into(),
            "m_RateMin is not a finite number",
        ),
        (
            INITIALIZERS,
            "C_INIT_RandomNamedModelSequence",
            "m_names",
            [12].into(),
            "m_names contains a non-string",
        ),
        (
            INITIALIZERS,
            "C_INIT_InitSkinnedPositionFromCPSnapshot",
            "m_nSnapShotStartPoint",
            5001.into(),
            "m_nSnapShotStartPoint 5001 is outside 0..=5000",
        ),
        (
            PRE_EMISSION,
            "C_OP_RemapSpeedtoCP",
            "m_nField",
            3.into(),
            "m_nField 3 is outside 0..=2",
        ),
    ];
    for (stage, class, member, value, expected) in cases {
        let error = read(stage, class, &[(member, value)]).expect_err(class);
        assert!(error.to_string().contains(expected), "{class}: {error}");
    }
}
