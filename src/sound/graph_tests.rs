use super::{SoundEventGraph, SoundSelectionMode, SoundTrack};
use crate::{Budget, kv3::Value};

#[test]
fn sound_event_inherits_across_documents_and_overrides_whole_fields() {
    let mut graph = SoundEventGraph::default();
    let mut budget = Budget::default();
    let parent = Value::object([
        ("type", Value::from("stack")),
        ("volume", Value::from(0.75)),
        ("pitch_rand_min", Value::from(-0.25)),
        ("vsnd_files", Value::from(["sounds/parent.vsnd"])),
        ("vsnd_files_track_01", Value::from(["sounds/layer.vsnd_c"])),
        ("weights_track_01", Value::from(0.25)),
        (
            "vsnd_selection_type_track_01",
            Value::from("random_weighted"),
        ),
    ]);
    let middle = Value::object([("base", Value::from("Base")), ("pitch", Value::from(0.5))]);
    let child = Value::object([
        ("base", Value::from("Middle")),
        ("volume", Value::from(0)),
        ("vsnd_files", Value::from_array([] as [Value; 0])),
        ("pitch_rand_max", Value::from(0.125)),
    ]);
    graph
        .add_events(Value::object([("Child", child)]), &mut budget)
        .unwrap();
    graph
        .add_events(
            Value::object([("Base", parent), ("Middle", middle)]),
            &mut budget,
        )
        .unwrap();

    let event = graph.resolve("Child").unwrap().unwrap();
    assert_eq!(event.base_chain().collect::<Vec<_>>(), ["Middle", "Base"]);
    assert_eq!(event.kind(), Some("stack"));
    assert_eq!(event.playback().volume, Some(0.0));
    assert_eq!(event.playback().pitch, Some(0.5));
    assert_eq!(event.playback().pitch_rand_min, Some(-0.25));
    assert_eq!(event.playback().pitch_rand_max, Some(0.125));
    assert_eq!(event.track(SoundTrack::Main).unwrap().files().count(), 0);
    let track = event.track(SoundTrack::Track01).unwrap();
    assert_eq!(
        track.selection_mode(),
        Some(SoundSelectionMode::RandomWeighted)
    );
    let file = track.files().next().unwrap();
    assert_eq!(file.weight, Some(0.25));
    assert_eq!(file.compiled_path().as_deref(), Some("sounds/layer.vsnd_c"));
}

#[test]
fn base_errors_name_the_missing_or_cyclic_event() {
    let mut graph = SoundEventGraph::default();
    graph
        .add_events(
            Value::object([
                ("First", Value::object([("base", "Second")])),
                ("Second", Value::object([("base", "First")])),
                ("Orphan", Value::object([("base", "Missing")])),
            ]),
            &mut Budget::default(),
        )
        .unwrap();

    assert!(
        graph
            .resolve("First")
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    assert!(
        graph
            .resolve("Orphan")
            .unwrap_err()
            .to_string()
            .contains("Missing")
    );
    assert!(graph.resolve("Absent").unwrap().is_none());
}

fn add_test_stacks(graph: &mut SoundEventGraph, budget: &mut Budget) {
    let make_stack = |variables| {
        Value::object([(
            "operators",
            Value::from_array([Value::object([
                ("name", Value::from("public")),
                ("operator_variables", variables),
            ])]),
        )])
    };
    graph
        .add_stacks(
            Value::object([
                (
                    "base_stack",
                    make_stack(Value::object([
                        ("mixgroup", Value::object([("value", "Weapons")])),
                        ("distance_max", Value::object([("value", "800")])),
                        ("limiter_stop_oldest", Value::object([("value", "true")])),
                    ])),
                ),
                (
                    "child_stack",
                    make_stack(Value::object([
                        ("import_public_opvars", Value::from("base_stack")),
                        ("position_offset", Value::object([("value", "[1, 2, 3,]")])),
                    ])),
                ),
            ]),
            budget,
        )
        .unwrap();
}

#[test]
fn compiled_flags_and_stack_public_defaults_are_typed() {
    let mut graph = SoundEventGraph::default();
    let mut budget = Budget::default();
    add_test_stacks(&mut graph, &mut budget);
    graph
        .add_events(
            Value::object([(
                "Event",
                Value::object([
                    ("type", Value::from("child_stack")),
                    ("limiter_stop_oldest", Value::from(false)),
                    ("looping", Value::from(true)),
                    ("use_distance_volume_mapping_curve", Value::from(true)),
                    (
                        "distance_volume_mapping_curve",
                        Value::from_array([[0.0, 1.0], [800.0, 0.0]]),
                    ),
                    ("spread_min", Value::from(10)),
                    ("spread_max", Value::from(100)),
                    ("vsnd_selection_index_track_02", Value::from(1)),
                    ("vsnd_selection_type_track_02", Value::from("index")),
                    ("max_voices", Value::from(8)),
                    (
                        "vsnd_files_track_02",
                        Value::from(["sounds/a.vsnd", "sounds/b.vsnd"]),
                    ),
                    ("weights_track_02", Value::from([0.25, 0.75])),
                ]),
            )]),
            &mut budget,
        )
        .unwrap();

    let event = graph.resolve("Event").unwrap().unwrap();
    assert_eq!(event.playback().mix_group, Some("Weapons"));
    assert_eq!(event.playback().looping, Some(true));
    assert_eq!(event.playback().max_voices, Some(8.0));
    assert_eq!(event.spatial().distance_max, Some(800.0));
    assert_eq!(event.spatial().position_offset, Some([1.0, 2.0, 3.0]));
    assert_eq!(event.limits().limiter_stop_oldest, Some(false));
    let spatial = event.spatial();
    assert_eq!(spatial.use_distance_volume_mapping_curve, Some(true));
    assert_eq!(
        spatial.spread_min.zip(spatial.spread_max),
        Some((10.0, 100.0))
    );
    let curve = spatial.distance_volume_mapping_curve.unwrap();
    assert_eq!(
        curve.points::<2>().collect::<Vec<_>>(),
        [[0.0, 1.0], [800.0, 0.0]]
    );
    let track = event.track(SoundTrack::Track02).unwrap();
    assert_eq!(track.selection_index, Some(1.0));
    assert_eq!(track.selection_mode(), Some(SoundSelectionMode::Index));
    let files: Vec<_> = track.files().collect();
    assert_eq!(files[1].weight, Some(0.75));
    assert_eq!(files[1].compiled_path().as_deref(), Some("sounds/b.vsnd_c"));
}

#[test]
fn a_deep_base_chain_returns_an_error() {
    let mut graph = SoundEventGraph::default();
    let definitions = (0..80).map(|index| {
        (
            index.to_string(),
            Value::object([("base", (index + 1).to_string())]),
        )
    });
    graph
        .add_events(Value::object(definitions), &mut Budget::default())
        .unwrap();
    let error = graph.resolve("0").unwrap_err().to_string();
    assert!(error.contains("event 0: more than 64 bases"), "{error}");
}

#[test]
fn an_unaffordable_event_file_leaves_the_graph_unchanged() {
    let mut graph = SoundEventGraph::default();
    graph
        .add_events(
            Value::object([("event", Value::object([("type", "original")]))]),
            &mut Budget::default(),
        )
        .unwrap();
    let replacement = Value::object([
        ("event", Value::object([("type", "replacement")])),
        ("extra", Value::object([("type", "stack")])),
    ]);
    let error = graph
        .add_events(replacement, &mut Budget::new(200))
        .unwrap_err();
    assert!(error.to_string().contains("takes"));
    assert_eq!(
        graph.resolve("event").unwrap().unwrap().kind(),
        Some("original")
    );
    assert!(graph.resolve("extra").unwrap().is_none());
}

#[cfg(any(unix, windows))]
#[test]
fn vpk_sound_event_audit() {
    use crate::{DirSource, Vpk, locate::install_in};
    use std::path::Path;

    let Some(path) = std::env::var_os("HEATEL_DOTA_PATH") else {
        return;
    };
    let source = DirSource::new(install_in(Path::new(&path)).expect("Dota install").game);
    let pak = Vpk::open(&source, "pak01_dir.vpk").unwrap();
    let (graph, resource_counts, definitions) = read_graph(&source, &pak);
    println!(
        "events / stacks: {resource_counts:?} files, {definitions} event definitions, {} stack names",
        graph.stack_names().len()
    );
    audit_events(&graph, &source, &pak);
}

#[cfg(any(unix, windows))]
fn audit_events(graph: &SoundEventGraph, source: &crate::DirSource, pak: &crate::Vpk) {
    use std::collections::{BTreeMap, BTreeSet};

    let mut groups = BTreeMap::<&str, EventAudit>::new();
    let mut fields = BTreeMap::<&str, usize>::new();
    let mut assets = BTreeSet::<String>::new();
    for name in graph.event_names() {
        let event = graph.resolve(name).unwrap().unwrap();
        for (key, _) in graph
            .event_definition(name)
            .unwrap()
            .as_object()
            .unwrap()
            .iter()
        {
            *fields.entry(key).or_default() += 1;
        }
        if event.stack_definition().is_none() {
            println!("missing stack for {name}: {:?}", event.kind());
        }
        let shortlist = groups_for(name).any(|group| {
            matches!(
                group,
                "hero death"
                    | "creep death"
                    | "neutral death"
                    | "siege destruction"
                    | "item family"
            )
        });
        let mut audit = EventAudit::of(&event, pak, |file, entry| {
            if shortlist {
                match entry {
                    Some(entry) => {
                        assets.insert(entry.path());
                    }
                    None => println!("missing candidate for {name}: {file}"),
                }
            }
        });
        if audit.with_asset == 0 {
            audit.layer_only = usize::from(layer_has_asset(graph, pak, &event));
        }
        for group in groups_for(name) {
            groups.entry(group).or_default().add(audit);
        }
    }
    assert!(groups["hero death"].with_asset > 0);
    groups.entry("creep death").or_default();
    assert!(groups["item activate"].with_asset > 0);
    for (group, counts) in groups {
        println!("{group}: {counts:?}");
    }
    for path in &assets {
        let bytes = pak
            .read_file(source, path)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        crate::Resource::parse(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    println!(
        "{} unique shortlisted clips read and CRC verified",
        assets.len()
    );
    print_fields(&fields);
}

#[cfg(any(unix, windows))]
fn layer_has_asset(
    graph: &SoundEventGraph,
    pak: &crate::Vpk,
    event: &super::ResolvedSoundEvent<'_>,
) -> bool {
    use std::collections::BTreeSet;

    let mut visited = BTreeSet::from([event.name()]);
    let mut pending: Vec<_> = enabled_layers(event).collect();
    while let Some(name) = pending.pop() {
        if !visited.insert(name) {
            continue;
        }
        let Some(event) = graph.resolve(name).unwrap() else {
            continue;
        };
        if EventAudit::of(&event, pak, |_, _| {}).with_asset != 0 {
            return true;
        }
        pending.extend(enabled_layers(&event));
    }
    false
}

#[cfg(any(unix, windows))]
fn enabled_layers<'a, 'event>(
    event: &'event super::ResolvedSoundEvent<'a>,
) -> impl Iterator<Item = &'a str> + 'event {
    event
        .layers()
        .filter(|link| link.enabled != Some(false))
        .map(|link| link.event)
}

#[cfg(any(unix, windows))]
fn read_graph(source: &crate::DirSource, pak: &crate::Vpk) -> (SoundEventGraph, [usize; 2], usize) {
    use crate::Resource;
    let mut graph = SoundEventGraph::default();
    let mut resource_counts = [0; 2];
    let mut definitions = 0;
    for entry in pak
        .entries()
        .filter(|entry| matches!(entry.ext(), "vsndevts_c" | "vsndstck_c"))
    {
        let path = entry.path();
        let bytes = pak
            .read(source, entry)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        let resource = Resource::parse(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut budget = Budget::default();
        if entry.ext() == "vsndevts_c" {
            resource_counts[0] += 1;
            let document = resource.kv3(b"DATA").unwrap().unwrap();
            definitions += document
                .root
                .as_object()
                .unwrap()
                .iter()
                .filter(|(_, value)| value.as_object().is_some())
                .count();
            graph
                .read_events(&resource, &mut budget)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
        } else {
            resource_counts[1] += 1;
            graph
                .read_stacks(&resource, &mut budget)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
        }
    }
    (graph, resource_counts, definitions)
}

#[cfg(any(unix, windows))]
fn print_fields(fields: &std::collections::BTreeMap<&str, usize>) {
    for key in [
        "base",
        "type",
        "vsnd_files",
        "vsnd_files_track_01",
        "vsnd_files_track_02",
        "weights",
        "vsnd_selection_type",
        "volume",
        "volume_rand_min",
        "volume_rand_max",
        "pitch",
        "pitch_rand_min",
        "pitch_rand_max",
        "delay",
        "distance_max",
        "distance_volume_mapping_curve",
        "spread_radius",
        "spread_min",
        "spread_max",
        "position",
        "position_offset",
        "mixgroup",
        "soundlevel",
        "limiter_on",
        "limiter_max",
        "limiter_stop_oldest",
        "max_voices",
        "looping",
    ] {
        println!("{key}: {}", fields.get(key).unwrap_or(&0));
    }
}

#[cfg(any(unix, windows))]
fn groups_for(name: &str) -> impl Iterator<Item = &'static str> {
    let name = name.to_ascii_lowercase();
    let death = name.ends_with(".death");
    let item = name.starts_with("dota_item.");
    let hero = name.starts_with("hero_");
    [
        ("all", true),
        ("hero death", hero && death),
        ("creep death", name.starts_with("creep_") && death),
        (
            "siege destruction",
            name.starts_with("creep_siege_") && name.ends_with(".destruction"),
        ),
        (
            "neutral death",
            (name.starts_with("n_creep_") || name.starts_with("neutral_")) && death,
        ),
        ("item family", item),
        (
            "item activate",
            item && name.split('.').any(|part| part == "activate"),
        ),
        ("hero cast", hero && name.ends_with(".cast")),
        ("ui", name.starts_with("ui.") || name.starts_with("ui_")),
    ]
    .into_iter()
    .filter_map(|(name, included)| included.then_some(name))
}

#[cfg(any(unix, windows))]
#[derive(Clone, Copy, Debug, Default)]
struct EventAudit {
    events: usize,
    with_stack: usize,
    with_authored_files: usize,
    with_asset: usize,
    layer_only: usize,
    candidates: usize,
    missing: usize,
}

#[cfg(any(unix, windows))]
impl EventAudit {
    fn of<'a>(
        event: &super::ResolvedSoundEvent<'_>,
        pak: &'a crate::Vpk,
        mut observe: impl FnMut(&str, Option<crate::vpk::Entry<'a>>),
    ) -> Self {
        let with_authored_files = usize::from(event.authored("vsnd_files").is_some());
        let mut counts = Self {
            events: 1,
            with_stack: usize::from(event.stack_definition().is_some()),
            with_authored_files,
            ..Self::default()
        };
        for track in event.tracks() {
            for file in track.files() {
                if file.path.is_empty() || file.path == "sounds/common/null.vsnd" {
                    continue;
                }
                counts.candidates += 1;
                let entry = file.compiled_path().and_then(|path| pak.get(&path));
                observe(file.path, entry);
                if entry.is_some() {
                    counts.with_asset = 1;
                } else {
                    counts.missing += 1;
                }
            }
        }
        counts
    }

    fn add(&mut self, other: Self) {
        self.events += other.events;
        self.with_stack += other.with_stack;
        self.with_authored_files += other.with_authored_files;
        self.with_asset += other.with_asset;
        self.layer_only += other.layer_only;
        self.candidates += other.candidates;
        self.missing += other.missing;
    }
}

#[test]
fn indexed_definitions_keep_file_positions_and_last_object_duplicate() {
    let mut graph = SoundEventGraph::default();
    let mut budget = Budget::default();
    graph
        .add_events(
            Value::object([
                ("skip", Value::Null),
                ("event", Value::object([("type", "first")])),
                ("event", Value::object([("type", "last")])),
                ("tail", Value::object([("base", "event")])),
                ("event", Value::Null),
            ]),
            &mut budget,
        )
        .unwrap();
    assert_eq!(
        graph.resolve("event").unwrap().unwrap().kind(),
        Some("last")
    );
    assert_eq!(graph.resolve("tail").unwrap().unwrap().kind(), Some("last"));
    assert!(graph.resolve("skip").unwrap().is_none());
    graph
        .add_events(
            Value::object([("event", Value::object([("type", "replacement")]))]),
            &mut budget,
        )
        .unwrap();
    assert_eq!(
        graph.resolve("tail").unwrap().unwrap().kind(),
        Some("replacement")
    );
}
