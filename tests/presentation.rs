//! Presentation data of a Dota 2 install: sound events, response rules, hero cosmetics and
//! particle control point configurations. The tests need a local install and are ignored
//! without `--ignored`:
//!
//! ```sh
//! cargo test --release --test presentation -- --ignored
//! ```

use heatel::items_game::{HeroItem, ModifierKind, read_hero_items};
use heatel::particle::control_point_configurations;
use heatel::sound::read_sound_events;
use heatel::talker::{ResponseKind, read_response_rules};
use heatel::{DirSource, Resource, Vpk, find_game_dir};

fn install() -> (DirSource, Vpk) {
    let dota = std::env::var_os("HEATEL_DOTA_PATH").map(std::path::PathBuf::from);
    let src = DirSource::new(
        find_game_dir(dota.as_deref()).expect("Dota 2 not found; set HEATEL_DOTA_PATH"),
    );
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    (src, pak)
}

fn items(src: &DirSource, pak: &Vpk) -> Vec<HeroItem> {
    read_hero_items(&pak.read_file(src, "scripts/items/items_game.txt").unwrap()).unwrap()
}

fn read(src: &DirSource, pak: &Vpk, path: &str) -> Vec<u8> {
    pak.read_file(src, path).unwrap()
}

/// Every sound event file (946 in 7.41), response rule file (209) and particle system's control
/// point configurations (83 312) of the install reads.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn every_file_reads() {
    let (src, pak) = install();
    let mut failed = Vec::new();
    let mut counts = [0usize; 3];
    for entry in pak.entries() {
        let kind = match entry.ext() {
            "vsndevts_c" => 0,
            "vrr_c" => 1,
            "vpcf_c" => 2,
            _ => continue,
        };
        counts[kind] += 1;
        let bytes = pak.read(&src, entry).unwrap();
        let result = Resource::parse(&bytes).and_then(|r| match kind {
            0 => read_sound_events(&r).map(drop),
            1 => read_response_rules(&r).map(drop),
            _ => control_point_configurations(&r).map(drop),
        });
        if let Err(err) = result {
            failed.push(format!("{}: {err}", entry.path()));
        }
    }
    assert!(
        counts[0] > 900 && counts[1] > 200 && counts[2] > 80_000,
        "{counts:?}"
    );
    assert!(failed.is_empty(), "{failed:#?}");
}

/// `Hero_Nevermore.Attack` picks between two files and carries 2.5 km; the pre-attack is silent.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn shadow_fiend_sound_events() {
    let (src, pak) = install();
    let bytes = read(
        &src,
        &pak,
        "soundevents/game_sounds_heroes/game_sounds_nevermore.vsndevts_c",
    );
    let events = read_sound_events(&Resource::parse(&bytes).unwrap()).unwrap();
    let event = |name: &str| {
        events
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    let attack = event("Hero_Nevermore.Attack");
    assert_eq!(attack.kind, "dota_src1_3d");
    assert_eq!(
        attack.files,
        [
            "sounds/weapons/hero/nevermore/attack01.vsnd",
            "sounds/weapons/hero/nevermore/attack02.vsnd"
        ]
    );
    assert_eq!(attack.sound_level, Some(75.0));
    assert_eq!(attack.distance_max, Some(2500.0));
    assert_eq!(attack.pitch, Some(1.0));
    let (low, high) = attack.pitch_jitter.unwrap();
    assert!((low + 0.05).abs() < 1e-6 && (high - 0.05).abs() < 1e-6);
    let pre = event("Hero_Nevermore.PreAttack");
    assert_eq!(pre.kind, "dota_null_start");
    assert!(pre.files.is_empty());
    let raze = event("Hero_Nevermore.Shadowraze");
    assert_eq!(
        raze.files,
        ["sounds/weapons/hero/nevermore/shadowraze.vsnd"]
    );
    assert_eq!(raze.mix_group.as_deref(), Some("Weapons"));
    assert_eq!(raze.spread_radius, Some(300.0));
    assert!((raze.volume.unwrap() - 0.8).abs() < 1e-3);
}

/// Shadow Fiend's 135 response groups: a spawn line needs the `Spawn` concept and his class.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn shadow_fiend_response_rules() {
    let (src, pak) = install();
    let bytes = read(&src, &pak, "scripts/talker/response_rules_nevermore.vrr_c");
    let rules = read_response_rules(&Resource::parse(&bytes).unwrap()).unwrap();
    assert_eq!(rules.groups.len(), 135);
    assert_eq!(rules.includes, ["scripts/talker/response_rules_shared.vrr"]);
    let spawn = rules
        .groups
        .iter()
        .find(|g| g.name == "nevermore_Spawn")
        .unwrap();
    assert_eq!(spawn.requirements, ["Spawn", "Isnevermore"]);
    assert_eq!(spawn.responses.len(), 8);
    assert_eq!(spawn.responses[0].kind, ResponseKind::Speak);
    assert_eq!(
        spawn.responses[0].value.as_deref(),
        Some("nevermore_nev_spawn_03")
    );
    let class = rules
        .criteria
        .iter()
        .find(|c| c.name == "Isnevermore")
        .unwrap();
    assert_eq!(class.match_key.as_deref(), Some("classname"));
    assert_eq!(class.match_expr.as_deref(), Some("npc_dota_hero_Nevermore"));
    assert_eq!(class.weight, Some(5.0));
    assert!(
        rules
            .groups
            .iter()
            .flat_map(|g| &g.responses)
            .any(|r| r.speak_once)
    );
}

/// Shadow Fiend's default cosmetics, and what other items replace of his effects.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn shadow_fiend_cosmetics() {
    let (src, pak) = install();
    let all = items(&src, &pak);
    assert!(all.len() > 10_000, "{}", all.len());
    let mine: Vec<_> = all
        .iter()
        .filter(|i| i.heroes == ["npc_dota_hero_nevermore"])
        .collect();
    let defaults: Vec<_> = mine.iter().filter(|i| i.is_default()).collect();
    let slots: Vec<_> = defaults
        .iter()
        .map(|i| i.slot.as_deref().unwrap())
        .collect();
    assert_eq!(
        slots,
        ["head", "shoulder", "arms", "ambient_effects", "hero_base"]
    );
    let head = defaults[0];
    assert_eq!(
        head.model.as_deref(),
        Some("models/heroes/shadow_fiend/shadow_fiend_head.vmdl")
    );
    let created = |i: &HeroItem| -> Vec<String> {
        i.modifiers
            .iter()
            .filter(|m| m.kind == ModifierKind::ParticleCreate)
            .filter_map(|m| m.modifier.clone())
            .collect()
    };
    assert_eq!(
        created(head),
        ["particles/units/heroes/hero_nevermore/shadow_fiend_ambient_eyes.vpcf"]
    );
    assert_eq!(
        created(defaults[3]),
        [
            "particles/units/heroes/hero_nevermore/nevermore_trail.vpcf",
            "particles/units/heroes/hero_nevermore/nevermore_ambient_glow.vpcf"
        ]
    );
    let replaced: Vec<&str> = mine
        .iter()
        .flat_map(|i| &i.modifiers)
        .filter(|m| m.kind == ModifierKind::Particle)
        .filter_map(|m| m.asset.as_deref())
        .collect();
    for base in [
        "nevermore_shadowraze",
        "nevermore_requiemofsouls",
        "nvm_atk_blur",
    ] {
        let path = format!("particles/units/heroes/hero_nevermore/{base}.vpcf");
        assert!(replaced.contains(&path.as_str()), "{base}");
    }
}

/// Pudge's cosmetics replace his hook and rot effects and his hook sound events, and place control points.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn pudge_cosmetics() {
    let (src, pak) = install();
    let all = items(&src, &pak);
    let mine: Vec<_> = all
        .iter()
        .filter(|i| i.heroes.iter().any(|h| h == "npc_dota_hero_pudge"))
        .flat_map(|i| &i.modifiers)
        .collect();
    let replaces = |kind: ModifierKind, what: &str| {
        mine.iter()
            .any(|m| m.kind == kind && m.asset.as_deref().is_some_and(|a| a.ends_with(what)))
    };
    assert!(replaces(ModifierKind::Particle, "pudge_meathook.vpcf"));
    assert!(replaces(ModifierKind::Particle, "pudge_rot.vpcf"));
    assert!(replaces(ModifierKind::Sound, "Hero_Pudge.AttackHookExtend"));
    let cp = mine
        .iter()
        .find(|m| m.kind == ModifierKind::ParticleControlPoint)
        .unwrap();
    assert!(cp.extra.iter().any(|(k, _)| k == "control_point_number"));
}

/// Shadowraze expects control points 0, 3, 60 and 61; Requiem 0, 1 and 3.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn control_point_configurations_of_the_shadow_fiend() {
    let (src, pak) = install();
    let points = |name: &str| {
        let bytes = read(
            &src,
            &pak,
            &format!("particles/units/heroes/hero_nevermore/{name}.vpcf_c"),
        );
        let configs = control_point_configurations(&Resource::parse(&bytes).unwrap()).unwrap();
        assert_eq!(configs[0].name, "preview");
        configs[0]
            .drivers
            .iter()
            .map(|d| d.control_point)
            .collect::<Vec<_>>()
    };
    assert_eq!(points("nevermore_shadowraze"), [0, 3, 60, 61]);
    assert_eq!(points("nevermore_requiemofsouls"), [0, 1, 3]);
    let bytes = read(
        &src,
        &pak,
        "particles/units/heroes/hero_nevermore/nevermore_shadowraze.vpcf_c",
    );
    let configs = control_point_configurations(&Resource::parse(&bytes).unwrap()).unwrap();
    let drivers = &configs[0].drivers;
    assert_eq!(
        drivers[0].attach_type.as_deref(),
        Some("PATTACH_WORLDORIGIN")
    );
    assert_eq!(drivers[2].offset, [0.0, 243.0, 0.0]);
}

/// A hostile `items_game.txt` is an error or a few items, never a panic or an unbounded read.
#[test]
fn hostile_items_game_is_bounded() {
    assert!(read_hero_items(&b"\"a\" {".repeat(100)).is_err());
    assert!(
        read_hero_items(b"} } } \"x\" \"y\" { }")
            .unwrap()
            .is_empty()
    );
    assert!(read_hero_items(b"\xff\xfe").is_err());
    let item = br#""r" { "items" { "1" { "used_by_heroes" { "h" "1" } "visuals" { "asset_modifier" { "type" "sound" "asset" "a" "modifier" "b" "k" "v" } } } } }"#;
    let items = read_hero_items(item).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].modifiers[0].kind, ModifierKind::Sound);
    assert_eq!(
        items[0].modifiers[0].extra,
        [("k".to_owned(), "v".to_owned())]
    );
}
