//! Particle systems of a Dota 2 install: every one reads, and the definitions a missile is made
//! of come out as Source 2 runs them, upgraded from the format they are stored in.

use heatel::particle::{Emitter, FloatInput, Force, Operator, Renderer, System};
use heatel::{DirSource, Resource, Vpk, find_game_dir};

fn install() -> (DirSource, Vpk) {
    let dota = std::env::var_os("HEATEL_DOTA_PATH").map(std::path::PathBuf::from);
    let src = DirSource::new(
        find_game_dir(dota.as_deref()).expect("Dota 2 not found; set HEATEL_DOTA_PATH"),
    );
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    (src, pak)
}

fn system(src: &DirSource, pak: &Vpk, path: &str) -> System {
    let bytes = pak.read_file(src, &format!("{path}_c")).unwrap();
    System::read(&Resource::parse(&bytes).unwrap()).unwrap()
}

/// Every `vpcf_c` of the install reads (83 312 in 7.41):
///
/// ```sh
/// cargo test --release --test particle -- --ignored
/// ```
#[test]
#[ignore = "needs a local Dota 2 install"]
fn every_particle_system_reads() {
    let (src, pak) = install();
    let mut failed = Vec::new();
    let mut count = 0;
    for entry in pak.entries().filter(|e| e.ext() == "vpcf_c") {
        count += 1;
        let bytes = pak.read(&src, entry).unwrap();
        if let Err(err) = Resource::parse(&bytes).and_then(|r| System::read(&r)) {
            failed.push(format!("{}: {err}", entry.path()));
        }
    }
    assert!(count > 80_000, "{count} systems");
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Shadow Fiend's attack (stored as `vpcf45`): one particle flies from control point 0 to 1
/// at the speed control point 2 gives, its impact children wait for the end cap, and what the
/// format left at older spellings reads as the current ones.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn a_missile_reads_as_source_2_runs_it() {
    let (src, pak) = install();
    let attack = system(
        &src,
        &pak,
        "particles/units/heroes/hero_nevermore/nevermore_base_attack.vpcf",
    );
    assert_eq!(attack.max_particles, 1);
    let emits_one = matches!(
        &attack.emitters[0].class,
        Emitter::Instantaneous {
            count: FloatInput::Literal(1.0),
            ..
        }
    );
    assert!(emits_one);
    let speed = attack.operators.iter().find_map(|op| match &op.class {
        Operator::MaxVelocity { max, .. } => Some(max.clone()),
        _ => None,
    });
    assert!(
        matches!(
            speed,
            Some(FloatInput::ControlPointComponent { component: 0, .. })
        ),
        "{speed:?}"
    );
    let pull = attack.forces.iter().find_map(|f| match &f.class {
        Force::AttractToControlPoint { transform, .. } => transform.control_point(),
        _ => None,
    });
    assert_eq!(pull.map(|cp| cp.index()), Some(4));
    let end_caps: Vec<&str> = attack
        .children
        .iter()
        .filter(|c| c.end_cap)
        .map(|c| c.path.as_str())
        .collect();
    assert_eq!(end_caps.len(), 2, "{end_caps:?}");
    let core = system(
        &src,
        &pak,
        "particles/units/heroes/hero_nevermore/nevermore_base_attack_core.vpcf",
    );
    let Renderer::Sprites(sprites) = &core.renderers[0].class else {
        panic!("{:?}", core.renderers[0].class);
    };
    assert_eq!(
        sprites.look.texture.as_deref(),
        Some("materials/particle/particle_glow_oval01.vtex")
    );
    assert_eq!(core.constants.radius, 72.0);
}
