//! Offline coverage of authored layers and environment-gated verification against Dota VPKs.

use heatel::anim::{AnimationSet, BlendDimensions, SourceTracks, read_clips};
use heatel::model::{ModelRig, default_wearables};
use heatel::{DirSource, Model, Resource, Vpk};

#[path = "common/kv3.rs"]
mod fixture;
#[path = "support/wire_builders.rs"]
mod wire;

use fixture::{K, kv3_file, s};
use wire::resource;

const GYRO: &str = "models/heroes/gyro/gyro.vmdl_c";

fn index(name: &'static str, value: i64) -> (&'static str, K) {
    (name, K::Int(value))
}

fn numbers(values: &[f64]) -> K {
    K::Arr(values.iter().copied().map(K::Float).collect())
}

fn skeleton(names: &[&str]) -> K {
    K::Obj(vec![(
        "m_modelSkeleton",
        K::Obj(vec![
            (
                "m_boneName",
                K::Arr(names.iter().map(|name| s(name)).collect()),
            ),
            (
                "m_nParent",
                K::Arr(
                    names
                        .iter()
                        .enumerate()
                        .map(|(i, _)| K::Int(if i == 0 { -1 } else { 0 }))
                        .collect(),
                ),
            ),
            (
                "m_bonePosParent",
                K::Arr(vec![numbers(&[1.0, 2.0, 3.0]); names.len()]),
            ),
            (
                "m_boneRotParent",
                K::Arr(vec![numbers(&[0.0, 0.0, 0.0, 1.0]); names.len()]),
            ),
            ("m_nFlag", K::Arr(vec![K::UInt(1024); names.len()])),
        ]),
    )])
}

fn entries() -> K {
    K::Arr(
        [
            ("leading", 0),
            ("ACT_DOTA_CONSTANT_LAYER", 3),
            ("haste", -1),
            ("ACT_DOTA_IDLE", 5),
            ("injured", 1),
        ]
        .into_iter()
        .map(|(name, weight)| K::Obj(vec![("m_name", s(name)), index("m_nWeight", weight)]))
        .collect(),
    )
}

fn blended_sequence() -> K {
    let layer = K::Obj(vec![
        index("m_nLocalReference", 0),
        index("m_nLocalPose", 1),
        ("m_peak", K::Float(0.25)),
        ("m_tail", K::Float(0.75)),
        ("m_end", K::Float(1.0)),
        (
            "m_flags",
            K::Obj(vec![
                ("m_bPose", K::Bool(true)),
                ("m_bSpline", K::Bool(true)),
            ]),
        ),
    ]);
    K::Obj(vec![
        ("m_sName", s("blend")),
        ("m_activityArray", entries()),
        (
            "m_flags",
            K::Obj(vec![
                ("m_bAutoplay", K::Bool(true)),
                ("m_bLegacyDelta", K::Bool(true)),
                ("m_bLegacyWorldspace", K::Bool(true)),
            ]),
        ),
        (
            "m_transition",
            K::Obj(vec![
                ("m_flFadeInTime", K::Float(0.125)),
                ("m_flFadeOutTime", K::Float(0.375)),
            ]),
        ),
        index("m_nLocalWeightlist", 0),
        ("m_autoLayerArray", K::Arr(vec![layer])),
        (
            "m_fetch",
            K::Obj(vec![
                ("m_flags", K::Obj(vec![("m_b2D_TRI", K::Bool(true))])),
                ("m_nGroupSize", K::Arr(vec![K::Int(2), K::Int(2)])),
                ("m_nLocalPose", K::Arr(vec![K::Int(0), K::Int(1)])),
                (
                    "m_localReferenceArray",
                    K::Arr(vec![K::Int(0), K::Int(1), K::Int(-1), K::Int(2)]),
                ),
                ("m_poseKeyArray0", numbers(&[-1.0, 1.0, -1.0, 1.0])),
                ("m_poseKeyArray1", numbers(&[-1.0, -1.0, 1.0, 1.0])),
                ("m_bFixedBlendWeight", K::Bool(true)),
                ("m_flFixedBlendWeightVals", numbers(&[0.25, 0.75])),
            ]),
        ),
    ])
}

fn sequence_group() -> K {
    let pose = |name| {
        K::Obj(vec![
            ("m_sName", s(name)),
            ("m_flStart", K::Float(-1.0)),
            ("m_flEnd", K::Float(1.0)),
            ("m_bLooping", K::Bool(true)),
            ("m_flLoop", K::Float(2.0)),
        ])
    };
    K::Obj(vec![
        (
            "m_localS1SeqDescArray",
            K::Arr(vec![
                blended_sequence(),
                K::Obj(vec![
                    ("m_sName", s("bind")),
                    ("m_SequenceKeys", K::Obj(vec![("bind_pose", K::Bool(true))])),
                ]),
            ]),
        ),
        (
            "m_localSequenceNameArray",
            K::Arr(vec![s("bind"), s("delta"), s("other")]),
        ),
        (
            "m_localPoseParamArray",
            K::Arr(vec![pose("yaw"), pose("pitch")]),
        ),
        ("m_localBoneNameArray", K::Arr(vec![s("ROTOR")])),
        (
            "m_localBoneMaskArray",
            K::Arr(vec![K::Obj(vec![
                ("m_sName", s("feathered")),
                ("m_nLocalBoneArray", K::Arr(vec![K::Int(0)])),
                ("m_flBoneWeightArray", numbers(&[0.375])),
                (
                    "m_morphCtrlWeightArray",
                    K::Arr(vec![K::Arr(vec![s("smile"), K::Float(0.25)])]),
                ),
            ])]),
        ),
    ])
}

fn full_segment(codec: i16, element: i16, samples: &[f32]) -> K {
    let bytes: Vec<_> = [codec, 0, 1, 0, element]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .chain(samples.iter().flat_map(|sample| sample.to_le_bytes()))
        .collect();
    K::Blob(bytes)
}

fn animation() -> K {
    let segment = |channel, data| {
        K::Obj(vec![
            index("m_nLocalChannel", channel),
            ("m_container", data),
        ])
    };
    let delta = K::Obj(vec![
        ("m_name", s("delta")),
        ("fps", K::Float(30.0)),
        (
            "m_flags",
            K::Obj(vec![
                ("m_bAnimGraphAdditive", K::Bool(true)),
                ("m_bDelta", K::Bool(true)),
            ]),
        ),
        (
            "m_pData",
            K::Obj(vec![
                index("m_nFrames", 2),
                (
                    "m_frameblockArray",
                    K::Arr(vec![K::Obj(vec![
                        index("m_nStartFrame", 0),
                        index("m_nEndFrame", 1),
                        ("m_segmentIndexArray", K::Arr(vec![K::Int(0), K::Int(1)])),
                    ])]),
                ),
            ]),
        ),
    ]);
    let codec = |name| K::Obj(vec![("m_szName", s(name))]);
    K::Obj(vec![
        ("m_animArray", K::Arr(vec![delta])),
        (
            "m_decoderArray",
            K::Arr(vec![
                codec("CCompressedFullQuaternion"),
                codec("CCompressedFullFloat"),
            ]),
        ),
        (
            "m_segmentArray",
            K::Arr(vec![
                segment(
                    0,
                    full_segment(0, 17, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0]),
                ),
                segment(1, full_segment(1, 23, &[0.25, 0.75])),
            ]),
        ),
    ])
}

fn animated_model() -> Vec<u8> {
    let ctrl = K::Obj(vec![(
        "embedded_animation",
        K::Obj(vec![
            index("anim_data_block", 2),
            index("seqgroup_data_block", 3),
            index("group_data_block", 4),
        ]),
    )]);
    let channel = |class, variable, name, id| {
        K::Obj(vec![
            ("m_szChannelClass", s(class)),
            ("m_szVariableName", s(variable)),
            ("m_szElementNameArray", K::Arr(vec![s(name)])),
            ("m_nElementIndexArray", K::Arr(vec![K::Int(id)])),
        ])
    };
    let group = K::Obj(vec![(
        "m_decodeKey",
        K::Obj(vec![(
            "m_dataChannelArray",
            K::Arr(vec![
                channel("BoneChannel", "Angle", "rotor", 17),
                channel("MorphChannel", "data", "smile", 23),
            ]),
        )]),
    )]);
    resource(&[
        (b"CTRL", kv3_file(&ctrl)),
        (b"DATA", kv3_file(&skeleton(&["root", "rotor"]))),
        (b"ANIM", kv3_file(&animation())),
        (b"ASEQ", kv3_file(&sequence_group())),
        (b"AGRP", kv3_file(&group)),
    ])
}

#[test]
fn authored_sequence_metadata_keeps_indices_weights_and_layer_order() {
    let bytes = animated_model();
    let set = AnimationSet::read(&Resource::parse(&bytes).unwrap()).unwrap();
    let blend = &set.sequences[0];
    assert!(blend.flags.autoplay && blend.flags.delta && blend.flags.worldspace);
    assert_eq!((blend.fade_in, blend.fade_out), (Some(0.125), Some(0.375)));
    assert_eq!(blend.blend.dimensions, BlendDimensions::Triangular);
    assert_eq!(blend.blend.size, [2, 2]);
    assert_eq!(blend.blend.pose_parameters, [Some(0), Some(1)]);
    assert_eq!(
        blend.blend.pose_keys,
        [vec![-1., 1., -1., 1.], vec![-1., -1., 1., 1.]]
    );
    assert_eq!(blend.blend.references, [Some(0), Some(1), None, Some(2)]);
    assert_eq!(blend.blend.fixed_blend_weights, Some([0.25, 0.75]));
    assert_eq!(set.references[0].sequence, Some(1));
    assert_eq!(set.references[1].clip, Some(0));
    assert_eq!(
        (set.references[2].clip, set.references[2].sequence),
        (None, None)
    );
    assert!(set.sequences[1].bind_pose);
    let layer = &blend.auto_layers[0];
    assert_eq!((layer.reference, layer.pose_parameter), (Some(0), Some(1)));
    assert_eq!(
        [layer.start, layer.peak, layer.tail, layer.end],
        [0.0, 0.25, 0.75, 1.0]
    );
    assert!(layer.flags.pose && layer.flags.spline);
    let rows: Vec<_> = blend
        .activity_entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.weight, entry.is_activity()))
        .collect();
    assert_eq!(
        rows,
        [
            ("leading", 0, false),
            ("ACT_DOTA_CONSTANT_LAYER", 3, true),
            ("haste", -1, false),
            ("ACT_DOTA_IDLE", 5, true),
            ("injured", 1, false),
        ]
    );
    assert_eq!(set.bone_references[0].bone, Some(1));
    assert_eq!(set.bone_masks[blend.bone_mask.unwrap()].bones, [(0, 0.375)]);
    assert_eq!(set.bone_masks[0].morphs, [("smile".into(), 0.25)]);
    let yaw = &set.pose_parameters[0];
    assert_eq!(
        (
            yaw.name.as_str(),
            yaw.start,
            yaw.end,
            yaw.loop_range,
            yaw.looping
        ),
        ("yaw", -1., 1., 2., true)
    );
}

#[test]
fn source_clip_keeps_sparse_delta_rotation_and_morph_keyframes() {
    let bytes = animated_model();
    let resource = Resource::parse(&bytes).unwrap();
    let set = AnimationSet::read(&resource).unwrap();
    assert!(set.clips[0].flags.additive && set.clips[0].flags.delta);
    let tracks = set.decode_clip(0).unwrap();
    assert!(tracks.bones[0].rotation.values.is_empty());
    assert!(tracks.bones[1].translation.values.is_empty());
    assert_eq!(tracks.bones[1].rotation.times, [0.0, 1.0 / 30.0]);
    assert_eq!(
        tracks.bones[1].rotation.values,
        [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0]]
    );
    assert_eq!(tracks.morphs[0].name, "smile");
    assert_eq!(tracks.morphs[0].keys.values, [[0.25], [0.75]]);
    assert!(read_clips(&resource, &["delta"]).unwrap().is_empty());
    assert!(matches!(set.decode_clip(1), Err(err) if err.to_string().contains("no clip 1")));
}

#[test]
fn rig_reads_all_attachment_influences_and_matches_wearable_bones() {
    let ctrl = K::Obj(vec![(
        "embedded_meshes",
        K::Arr(vec![K::Obj(vec![index("data_block", 2)])]),
    )]);
    let point = K::Obj(vec![
        ("m_name", s("attach_attack1")),
        ("m_bIgnoreRotation", K::Bool(true)),
        index("m_nInfluences", 2),
        ("m_influenceNames", K::Arr(vec![s("rotor"), s("external")])),
        ("m_influenceWeights", numbers(&[0.75, 0.25])),
        (
            "m_vInfluenceOffsets",
            K::Arr(vec![numbers(&[2.0, 3.0, 5.0]), numbers(&[1.0, 2.0, 3.0])]),
        ),
        (
            "m_vInfluenceRotations",
            K::Arr(vec![numbers(&[0.0, 0.0, 1.0, 0.0]); 2]),
        ),
        (
            "m_bInfluenceRootTransform",
            K::Arr(vec![K::Bool(false), K::Bool(true)]),
        ),
    ]);
    let mesh = K::Obj(vec![("m_attachments", K::Arr(vec![point]))]);
    let bytes = resource(&[
        (b"CTRL", kv3_file(&ctrl)),
        (b"DATA", kv3_file(&skeleton(&["root", "ROTOR"]))),
        (b"MDAT", kv3_file(&mesh)),
    ]);
    let rig = ModelRig::read(&Resource::parse(&bytes).unwrap()).unwrap();
    assert_eq!(rig.bones[1].parent, Some(0));
    assert_eq!(rig.bones[1].flags, 1024);
    let point = &rig.attachments[0];
    assert_eq!(point.name, "attach_attack1");
    assert!(point.ignore_rotation);
    assert_eq!(point.influences[0].bone, Some(1));
    assert_eq!(point.influences[0].weight, 0.75);
    assert_eq!(point.influences[0].position, [2.0, 3.0, 5.0]);
    assert_eq!(point.influences[0].rotation, [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(point.influences[1].bone, None);
    assert!(point.influences[1].root_transform);
    let wearable = resource(&[(b"DATA", kv3_file(&skeleton(&["Rotor", "own"])))]);
    let wearable = ModelRig::read(&Resource::parse(&wearable).unwrap()).unwrap();
    assert_eq!(wearable.bone_merge_map(&rig), [Some(1), None]);
}

#[test]
fn default_wearables_select_the_base_forms_slots() {
    let items = br#""items_game" {
        "items" {
            "1" { "prefab" "default_item" "item_slot" "weapon" "model_player" "base.vmdl" "used_by_heroes" { "hero" "1" } }
            "2" { "prefab" "default_item" "item_slot" "weapon_persona_1" "model_player" "persona.vmdl" "used_by_heroes" { "hero" "1" } }
            "3" { "prefab" "default_item" "item_slot" "hero_base" "model_player" "body.vmdl" "used_by_heroes" { "hero" "1" } }
            "4" { "prefab" "wearable" "item_slot" "weapon" "model_player" "fancy.vmdl" "used_by_heroes" { "hero" "1" } }
            "5" { "prefab" "default_item" "item_slot" "weapon" "model_player" "other.vmdl" "used_by_heroes" { "other" "1" } }
        }
    }"#;
    let items = heatel::items_game::read_hero_items(items).unwrap();
    let models: Vec<_> = default_wearables(&items, "hero")
        .filter_map(|item| item.model.as_deref())
        .collect();
    assert_eq!(models, ["base.vmdl"]);
}

fn install() -> Option<(DirSource, Vpk)> {
    let path = std::path::PathBuf::from(std::env::var_os("HEATEL_DOTA_PATH")?);
    let source = DirSource::new(heatel::find_game_dir(Some(&path)).expect("Dota install"));
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("Dota VPK");
    Some((source, pak))
}

fn changed_rotation(tracks: &SourceTracks, bone: usize) -> bool {
    tracks.bones[bone].rotation.values.windows(2).any(|pair| {
        pair[0]
            .iter()
            .zip(pair[1])
            .map(|(a, b)| a * b)
            .sum::<f32>()
            .abs()
            < 0.9999
    })
}

#[test]
fn gyrocopter_constant_layers_spin_both_rotor_bones() {
    let Some((source, pak)) = install() else {
        return;
    };
    let bytes = pak.read_file(&source, GYRO).unwrap();
    let resource = Resource::parse(&bytes).unwrap();
    let rig = ModelRig::read(&resource).unwrap();
    let set = AnimationSet::read(&resource).unwrap();
    let constants: Vec<_> = set
        .sequences
        .iter()
        .filter(|seq| {
            seq.activity_entries
                .iter()
                .any(|entry| entry.name == "ACT_DOTA_CONSTANT_LAYER")
        })
        .collect();
    assert_eq!(
        constants
            .iter()
            .map(|seq| seq.name.as_str())
            .collect::<Vec<_>>(),
        [
            "rattle_loop",
            "rattle_loop_02",
            "guns_rattle_loop",
            "guns_rattle_loop02",
        ]
    );
    for seq in constants {
        assert!(seq.flags.looping && seq.flags.hidden && seq.flags.delta);
        assert!(!seq.flags.autoplay && !seq.flags.worldspace);
        assert_eq!(seq.activity_entries[0].weight, 1);
        assert_eq!((seq.fade_in, seq.fade_out), (Some(0.2), Some(0.2)));
        assert_eq!(
            seq.activity_entries.get(1).map(|entry| entry.name.as_str()),
            seq.name.starts_with("guns_").then_some("guns"),
        );
        let mask = &set.bone_masks[seq.bone_mask.unwrap()];
        assert_eq!(mask.name, "jiggle");
        let clip = set.references[seq.blend.references[0].unwrap()]
            .clip
            .unwrap();
        assert!(set.clips[clip].flags.delta && !set.clips[clip].flags.additive);
        assert_eq!(set.clips[clip].name, format!("@@{}", seq.name));
        assert_eq!((set.clips[clip].frames, set.clips[clip].fps), (115, 30.0));
        let tracks = set.decode_clip(clip).unwrap();
        for (bone, name) in [(41, "ft_prop16_1"), (81, "rear_prop18_1")] {
            assert_eq!(rig.bones[bone].name, name);
            assert!(mask.bones.iter().any(|&(reference, weight)| {
                set.bone_references[reference].bone == Some(bone) && weight == 1.0
            }));
            assert!(changed_rotation(&tracks, bone), "{}: {name}", seq.name);
            assert_eq!(tracks.bones[bone].rotation.values.len(), 115);
        }
    }
}

#[test]
fn gyrocopter_default_wearables_resolve_front_and_rear_rotors() {
    let Some((source, pak)) = install() else {
        return;
    };
    let bytes = pak.read_file(&source, GYRO).unwrap();
    let rig = ModelRig::read(&Resource::parse(&bytes).unwrap()).unwrap();
    let attachment = rig
        .attachments
        .iter()
        .find(|point| point.name == "attach_prop1")
        .unwrap();
    assert_eq!(attachment.influences[0].bone, Some(6));
    assert_eq!(attachment.influences[0].position, [6.0, 0.0, 0.0]);
    let items = pak
        .read_file(&source, "scripts/items/items_game.txt")
        .unwrap();
    let items = heatel::items_game::read_hero_items(&items).unwrap();
    let wearables: Vec<_> = default_wearables(&items, "npc_dota_hero_gyrocopter").collect();
    assert_eq!(wearables.len(), 6);
    for (slot, path, wearable_bone, base_bone) in [
        ("misc", "models/heroes/gyro/gyro_propeller.vmdl", 3, 41),
        ("back", "models/heroes/gyro/gyro_bottles.vmdl", 11, 81),
    ] {
        let item = wearables
            .iter()
            .find(|item| item.slot.as_deref() == Some(slot))
            .unwrap();
        assert_eq!(item.model.as_deref(), Some(path));
        let bytes = pak.read_file(&source, &format!("{path}_c")).unwrap();
        let wearable = ModelRig::read(&Resource::parse(&bytes).unwrap()).unwrap();
        assert_eq!(
            wearable.bone_merge_map(&rig)[wearable_bone],
            Some(base_bone)
        );
    }
}

fn hero_models(source: &DirSource, pak: &Vpk) -> Vec<(String, String)> {
    let registry = pak.read_file(source, "scripts/npc/npc_heroes.txt").unwrap();
    let paths = heatel::keyvalues::text(&registry)
        .unwrap()
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("#base ")?
                .trim()
                .strip_prefix('"')?
                .strip_suffix('"')
        });
    let scripts: Vec<_> = paths
        .map(|path| {
            pak.read_file(source, &format!("scripts/npc/{path}"))
                .unwrap()
        })
        .collect();
    let inputs: Vec<_> = scripts.iter().map(Vec::as_slice).collect();
    heatel::scripts::npc_models(&inputs)
        .unwrap()
        .into_iter()
        .filter(|(hero, _)| {
            hero.starts_with("npc_dota_hero_")
                && !matches!(
                    hero.as_str(),
                    "npc_dota_hero_base" | "npc_dota_hero_target_dummy"
                )
        })
        .map(|(hero, path)| (hero, format!("{path}_c")))
        .collect()
}

#[derive(Default)]
struct Totals {
    sequences: usize,
    clips: usize,
    attachments: usize,
    layers: usize,
    poses: usize,
    blend_modes: [usize; 4],
    morph_tracks: usize,
    wearables: usize,
    matched_wearable_bones: usize,
    unmatched_wearable_bones: usize,
}

fn audit_model(bytes: &[u8], totals: &mut Totals) -> heatel::Result<ModelRig> {
    let resource = Resource::parse(bytes)?;
    let set = AnimationSet::read(&resource)?;
    let rig = ModelRig::read(&resource)?;
    let model = Model::read(&resource)?;
    assert_eq!(rig.bones.len(), model.bones.len());
    assert!(!set.sequences.is_empty());
    totals.sequences += set.sequences.len();
    totals.clips += set.clips.len();
    totals.attachments += rig.attachments.len();
    totals.poses += set.pose_parameters.len();
    for seq in &set.sequences {
        totals.layers += seq.auto_layers.len();
        let mode = match seq.blend.dimensions {
            BlendDimensions::Single => 0,
            BlendDimensions::One => 1,
            BlendDimensions::Two => 2,
            BlendDimensions::Triangular => 3,
        };
        totals.blend_modes[mode] += 1;
        for index in seq.blend.references.iter().flatten() {
            let reference = &set.references[*index];
            assert!(
                reference.clip.is_some() || reference.sequence.is_some(),
                "{}",
                reference.name
            );
        }
        for index in seq.auto_layers.iter().filter_map(|layer| layer.reference) {
            assert!(
                set.references[index].sequence.is_some(),
                "{}",
                set.references[index].name
            );
        }
    }
    for (index, clip) in set.clips.iter().enumerate() {
        let tracks = set
            .decode_clip(index)
            .unwrap_or_else(|err| panic!("clip {}: {err}", clip.name));
        assert_eq!(tracks.bones.len(), rig.bones.len());
        totals.morph_tracks += tracks.morphs.len();
    }
    Ok(rig)
}

fn audit_wearables(
    source: &DirSource,
    pak: &Vpk,
    base: &ModelRig,
    items: &[heatel::items_game::HeroItem],
    hero: &str,
    totals: &mut Totals,
) -> heatel::Result<()> {
    for item in default_wearables(items, hero) {
        let path = format!("{}_c", item.model.as_deref().unwrap());
        let bytes = pak.read_file(source, &path)?;
        let rig = ModelRig::read(&Resource::parse(&bytes)?)?;
        let map = rig.bone_merge_map(base);
        let matched = map.iter().flatten().count();
        totals.wearables += 1;
        totals.matched_wearable_bones += matched;
        totals.unmatched_wearable_bones += map.len() - matched;
    }
    Ok(())
}

#[test]
fn every_playable_hero_model_and_clip_decodes_from_the_install() {
    let Some((source, pak)) = install() else {
        return;
    };
    let models = hero_models(&source, &pak);
    assert_eq!(models.len(), 127);
    let mut totals = Totals::default();
    let items = pak
        .read_file(&source, "scripts/items/items_game.txt")
        .unwrap();
    let items = heatel::items_game::read_hero_items(&items).unwrap();
    for (hero, path) in &models {
        let bytes = pak
            .read_file(&source, path)
            .unwrap_or_else(|err| panic!("{hero} {path}: {err}"));
        let rig =
            audit_model(&bytes, &mut totals).unwrap_or_else(|err| panic!("{hero} {path}: {err}"));
        audit_wearables(&source, &pak, &rig, &items, hero, &mut totals)
            .unwrap_or_else(|err| panic!("{hero}: {err}"));
    }
    eprintln!(
        "{} models, {} sequences, {} clips, {} attachments, {} automatic layers, {} poses, blend modes {:?}, {} sampled morph tracks",
        models.len(),
        totals.sequences,
        totals.clips,
        totals.attachments,
        totals.layers,
        totals.poses,
        totals.blend_modes,
        totals.morph_tracks,
    );
    assert!(totals.wearables > 500);
    eprintln!(
        "{} default wearables, {} matched bones, {} unmatched bones",
        totals.wearables, totals.matched_wearable_bones, totals.unmatched_wearable_bones,
    );
}
