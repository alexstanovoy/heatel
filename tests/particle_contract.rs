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
