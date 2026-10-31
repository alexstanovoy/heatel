//! The sequences of a Dota 2 install's models: activities, lengths and animation events.
//! The tests need a local install and are ignored without `--ignored`:
//!
//! ```sh
//! cargo test --release --test sequence -- --ignored
//! ```

use heatel::anim::{AnimEvent, EventData, Sequence, read_sequences};
use heatel::{DirSource, Model, Resource, Vpk, find_game_dir};

const SHADOW_FIEND: &str = "models/heroes/shadow_fiend/shadow_fiend.vmdl_c";
const PUDGE: &str = "models/heroes/pudge/pudge.vmdl_c";
const CREEP: &str = "models/creeps/lane_creeps/creep_radiant_melee/radiant_melee.vmdl_c";
const TOWER: &str = "models/props_structures/radiant_tower002.vmdl_c";

fn install() -> (DirSource, Vpk) {
    let dota = std::env::var_os("HEATEL_DOTA_PATH").map(std::path::PathBuf::from);
    let src = DirSource::new(
        find_game_dir(dota.as_deref()).expect("Dota 2 not found; set HEATEL_DOTA_PATH"),
    );
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    (src, pak)
}

fn sequences(src: &DirSource, pak: &Vpk, path: &str) -> Vec<Sequence> {
    let bytes = pak.read_file(src, path).unwrap();
    read_sequences(&Resource::parse(&bytes).unwrap()).unwrap()
}

fn find<'a>(list: &'a [Sequence], name: &str) -> &'a Sequence {
    list.iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no sequence {name}"))
}

fn events(list: &[Sequence]) -> impl Iterator<Item = &AnimEvent> {
    list.iter().flat_map(|s| s.events.iter())
}

/// Every model of the install (18 675 in 7.41) reads, and what comes out is sane.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn every_model_reads() {
    let (src, pak) = install();
    let (mut count, mut failed) = (0, Vec::new());
    for entry in pak.entries().filter(|e| e.ext() == "vmdl_c") {
        count += 1;
        let bytes = pak.read(&src, entry).unwrap();
        match Resource::parse(&bytes).and_then(|r| read_sequences(&r)) {
            Ok(list) => {
                for s in &list {
                    assert!(
                        s.fps > 0.0 && s.fps.is_finite(),
                        "{}: {}",
                        entry.path(),
                        s.name
                    );
                    assert!(s.events.windows(2).all(|w| w[0].frame <= w[1].frame));
                }
            }
            Err(err) => failed.push(format!("{}: {err}", entry.path())),
        }
    }
    assert!(count > 15_000, "{count} models");
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Shadow Fiend: the attack plays for `ACT_DOTA_ATTACK` with the speed modifiers Dota picks
/// by, the swing starts `nvm_atk_blur` on frame 12, a raze plays for its own activity.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn shadow_fiend_sequences() {
    let (src, pak) = install();
    let list = sequences(&src, &pak, SHADOW_FIEND);
    for modifier in ["fast", "faster", "fastest"] {
        let s = find(&list, &format!("attack01_{modifier}"));
        let attack = &s.activities[0];
        assert_eq!(attack.name, "ACT_DOTA_ATTACK");
        assert_eq!(attack.modifiers, [modifier]);
        assert!(!s.looping);
    }
    let attack = find(&list, "attack01_anim");
    assert_eq!(attack.fps, 30.0);
    assert!(attack.frames > 12);
    let blur = attack
        .events
        .iter()
        .find(|e| e.name == "AE_CL_CREATE_PARTICLE_EFFECT_CFG")
        .expect("the blur");
    assert_eq!(blur.frame, 12);
    assert!((blur.cycle - 12.0 / (attack.frames - 1) as f32).abs() < 1e-3);
    let EventData::Particle {
        particle, config, ..
    } = &blur.data
    else {
        panic!("{:?}", blur.data)
    };
    assert_eq!(
        particle,
        "particles/units/heroes/hero_nevermore/nvm_atk_blur.vpcf"
    );
    assert_eq!(config, "absorigin_follow");
    assert!((attack.time_of(12) - 0.4).abs() < 1e-6);
    assert!(attack.duration() > attack.time_of(12));
    assert_eq!(
        find(&list, "cast1_shadowraze_a_anim").activities[0].name,
        "ACT_DOTA_RAZE_1"
    );
    assert_eq!(
        find(&list, "cast6_requiem_anim").activities[0].name,
        "ACT_DOTA_CAST_ABILITY_6"
    );
    let run = find(&list, "run_anim");
    assert!(run.looping);
    assert_eq!(run.activities[0].weight, 11);
}

/// Pudge: footsteps and chain sounds, the dismember particle, attack sounds of the sound set,
/// and the attachments his hook and hits use.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn pudge_sequences() {
    let (src, pak) = install();
    let list = sequences(&src, &pak, PUDGE);
    let sound = |name: &str| {
        events(&list)
            .any(|e| matches!(&e.data, EventData::PlaySound { sound, .. } if sound == name))
    };
    assert!(sound("Hero_Pudge.Footsteps") && sound("Hero_Pudge.ChainFoley"));
    assert!(sound("Hero_Pudge.DismemberSwings"));
    let steps = events(&list).find_map(|e| match &e.data {
        EventData::PlaySound { sound, tag, .. } if sound == "Hero_Pudge.Footsteps" => Some(tag),
        _ => None,
    });
    assert_eq!(steps.unwrap().as_deref(), Some("run_suppress"));
    assert!(events(&list).any(|e| matches!(&e.data,
        EventData::Particle { particle, .. } if particle.ends_with("pudge_dismember.vpcf"))));
    let attack_sounds = events(&list)
        .filter(|e| matches!(&e.data, EventData::AttackSound { sound } if sound.is_empty()))
        .count();
    assert!(attack_sounds >= 14, "{attack_sounds}");
    let bytes = pak.read_file(&src, PUDGE).unwrap();
    let model = Model::read(&Resource::parse(&bytes).unwrap()).unwrap();
    for name in ["attach_hitloc", "attach_hook"] {
        assert!(model.attachments.iter().any(|a| a.name == name), "{name}");
    }
}

/// A lane creep's footsteps, a tower's single attack sound.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn creep_and_tower_sequences() {
    let (src, pak) = install();
    let creep = sequences(&src, &pak, CREEP);
    assert!(events(&creep).any(|e| matches!(&e.data,
        EventData::PlaySound { sound, .. } if sound == "Creep_Radiant.Footstep")));
    assert!(
        creep
            .iter()
            .any(|s| s.activities.iter().any(|a| a.name == "ACT_DOTA_ATTACK"))
    );
    let tower = sequences(&src, &pak, TOWER);
    let attack: Vec<_> = events(&tower).collect();
    assert_eq!(attack.len(), 1);
    assert!(matches!(&attack[0].data, EventData::AttackSound { sound } if sound.is_empty()));
    assert_eq!(attack[0].frame, 12);
}
