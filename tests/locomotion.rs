#[path = "common/movement.rs"]
mod movement;

use heatel::anim::{read_clips, read_clips_in_place};
use heatel::{DirSource, Resource, Vpk, find_game_dir};

#[test]
#[ignore = "needs a local Dota 2 install"]
fn in_place_runs_preserve_every_authored_root_axis_and_rotation_over_the_whole_loop() {
    let source = DirSource::new(find_game_dir(None).expect("local install"));
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let mut cases: Vec<_> = [
        ("n_creep_ogre_lrg", "run"),
        ("n_creep_ogre_med", "run"),
        ("n_creep_satyr_c", "satyr_c_run"),
    ]
    .into_iter()
    .map(|(model, run)| {
        (
            model.to_owned(),
            format!("models/creeps/neutral_creeps/{model}/{model}.vmdl_c"),
            run,
        )
    })
    .collect();
    cases.extend([
        (
            "Shadow Fiend".into(),
            "models/heroes/shadow_fiend/shadow_fiend.vmdl_c".into(),
            "run_anim",
        ),
        (
            "Pudge".into(),
            "models/heroes/pudge/pudge.vmdl_c".into(),
            "pudge_walkN_anim",
        ),
    ]);
    for (model, path, run) in cases {
        let bytes = pak.read_file(&source, &path).expect("model");
        let resource = Resource::parse(&bytes).expect("resource");
        let raw = read_clips(&resource, &[run]).expect("original clips");
        let placed =
            read_clips_in_place(&resource, &[run], &["ACT_DOTA_RUN"]).expect("in place clips");
        let trajectory = movement::Trajectory::read(&resource, run);
        eprintln!("{model}: {run} -> {}", trajectory.animation);
        for time in trajectory.times() {
            let (expected_position, expected_rotation) = trajectory.pose(&raw[0], 0, time);
            let position = movement::sample(&placed[0].tracks[0].translation, time, false);
            let rotation = movement::sample(&placed[0].tracks[0].rotation, time, true);
            for axis in 0..3 {
                assert!(
                    (position[axis] - expected_position[axis]).abs() < 1e-4,
                    "{model} axis{axis} at{time}: {position:?} vs{expected_position:?}"
                );
            }
            let dot = rotation
                .iter()
                .zip(expected_rotation)
                .map(|(a, b)| a * b)
                .sum::<f32>()
                .abs();
            assert!(
                dot > 1.0 - 1e-5,
                "{model} full root rotation at{time}: {rotation:?} vs{expected_rotation:?}"
            );
        }
        assert_eq!(
            raw[0].tracks[0].scale.values,
            placed[0].tracks[0].scale.values
        );
        for (a, b) in raw[0]
            .tracks
            .iter()
            .skip(1)
            .zip(placed[0].tracks.iter().skip(1))
        {
            assert_eq!(a.rotation.values, b.rotation.values);
            assert_eq!(a.translation.values, b.translation.values);
            assert_eq!(a.scale.values, b.scale.values);
        }
        let vertical = &placed[0].tracks[0].translation;
        let first = vertical.values.first().unwrap();
        let last = vertical.values.last().unwrap();
        for axis in 0..3 {
            assert!(
                (last[axis] - first[axis]).abs() < 1e-3,
                "{model} loop seam axis{axis}"
            );
        }
    }
}
