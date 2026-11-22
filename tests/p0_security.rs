//! Regression tests for P0 security, DoS, overflow, and validation bugs.

use std::sync::atomic::{AtomicBool, Ordering};

use heatel::anim::read_clips;
use heatel::face::MorphSet;
use heatel::sound::Lipsync;
use heatel::texture::Texture;
use heatel::world::lighting;
use heatel::{Budget, Error, Resource, Source, Vpk, kv3};

#[path = "common/kv3.rs"]
mod fixture;
use fixture::{K, kv3_file, s};

/// Builds a compiled Source 2 resource from `(kind, payload)` blocks.
fn resource(blocks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_end = 16 + 12 * blocks.len();
    let size = table_end + blocks.iter().map(|(_, b)| b.len()).sum::<usize>();
    let mut out = (size as u32).to_le_bytes().to_vec();
    out.extend(12u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(8u32.to_le_bytes());
    out.extend((blocks.len() as u32).to_le_bytes());
    let mut at = table_end;
    for (kind, block) in blocks {
        let field = out.len() + 4;
        out.extend(*kind);
        out.extend(((at - field) as u32).to_le_bytes());
        out.extend((block.len() as u32).to_le_bytes());
        at += block.len();
    }
    for (_, block) in blocks {
        out.extend(block);
    }
    out
}

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn vtex_bytes(
    width: u16,
    height: u16,
    format: u8,
    extras: &[(u32, Vec<u8>)],
    raw: &[u8],
) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&[0; 16]);
    for v in [width, height, 1] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(&[format, 1]);
    data.extend_from_slice(&words(&[0, 8, extras.len() as u32]));
    let mut payload_at = data.len() + 12 * extras.len();
    let mut payloads = Vec::new();
    for (kind, payload) in extras {
        let field = data.len() + 4;
        data.extend_from_slice(&words(&[
            *kind,
            (payload_at - field) as u32,
            payload.len() as u32,
        ]));
        payload_at += payload.len();
        payloads.extend_from_slice(payload);
    }
    data.extend_from_slice(&payloads);
    let mut out = resource(&[(b"DATA", data)]);
    out.extend_from_slice(raw);
    out
}

fn sheet_extra(rect: [f32; 4]) -> Vec<u8> {
    let mut out = words(&[8, 1]);
    out.extend_from_slice(&words(&[0, 0, 24, 1, 1.0f32.to_bits(), 0, 0, 0]));
    out.extend_from_slice(&words(&[1.0f32.to_bits(), 8, 1]));
    for v in rect.into_iter().chain([0.0, 0.0, 1.0, 1.0]) {
        out.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    out
}

/// P0 #1: A clip with `m_nFrames = u64::MAX`, 0 bones, and 0 frame blocks must fail
/// instantaneously instead of spinning for 2^64 iterations.
#[test]
fn anim_clip_zero_bones_u64_max_frames_returns_err_immediately() {
    let ctrl = K::Obj(vec![(
        "embedded_animation",
        K::Obj(vec![
            ("group_data_block", K::Int(2)),
            ("anim_data_block", K::Int(3)),
        ]),
    )]);
    let data = K::Obj(vec![("m_modelSkeleton", K::Obj(vec![]))]);
    let group = K::Obj(vec![("m_decodeKey", K::Obj(vec![]))]);
    let anim = K::Obj(vec![(
        "m_animArray",
        K::Arr(vec![K::Obj(vec![
            ("m_name", s("spin")),
            ("fps", K::Float(30.0)),
            (
                "m_pData",
                K::Obj(vec![
                    ("m_nFrames", K::UInt(u64::MAX)),
                    ("m_frameblockArray", K::Arr(vec![])),
                ]),
            ),
        ])]),
    )]);
    let bytes = resource(&[
        (b"CTRL", kv3_file(&ctrl)),
        (b"DATA", kv3_file(&data)),
        (b"AGRP", kv3_file(&group)),
        (b"ANIM", kv3_file(&anim)),
    ]);
    let parsed = Resource::parse(&bytes).expect("valid resource");
    let err = read_clips(&parsed, &["spin"])
        .err()
        .expect("must reject u64::MAX frames");
    assert!(err.to_string().contains("animation"), "{err}");
}

/// P0 #2a: MorphSet::targets with `(1 << 62, 1)` atlas dimensions must not wrap `aw * ah * 4`
/// to 0 or panic.
#[test]
fn morph_set_atlas_dims_overflow_returns_err() {
    let kv = kv3::decode(&kv3_file(&K::Obj(vec![
        ("m_nWidth", K::UInt(1)),
        ("m_nHeight", K::UInt(1)),
        ("m_morphDatas", K::Arr(vec![])),
    ])))
    .unwrap();
    let set = MorphSet::read(&kv.root, "atlas.vtex").unwrap();
    let err = set.targets(&[], (1usize << 62, 1), 0).unwrap_err();
    assert!(err.to_string().contains("atlas"), "{err}");
}

/// P0 #2b: MorphSet::read rejects rectangles with `rect.x = usize::MAX` (past grid).
#[test]
fn morph_set_rect_x_usize_max_returns_err() {
    let kv = kv3::decode(&kv3_file(&K::Obj(vec![
        ("m_nWidth", K::UInt(4)),
        ("m_nHeight", K::UInt(4)),
        (
            "m_bundleTypes",
            K::Arr(vec![s("MORPH_BUNDLE_TYPE_POSITION_SPEED")]),
        ),
        (
            "m_morphDatas",
            K::Arr(vec![K::Obj(vec![(
                "m_morphRectDatas",
                K::Arr(vec![K::Obj(vec![
                    ("m_nXLeftDst", K::UInt(usize::MAX as u64)),
                    ("m_nYTopDst", K::UInt(0)),
                    ("m_flUWidthSrc", K::Float(0.25)),
                    ("m_flVHeightSrc", K::Float(0.25)),
                    ("m_bundleDatas", K::Arr(vec![])),
                ])]),
            )])]),
        ),
    ])))
    .unwrap();
    let read_result = MorphSet::read(&kv.root, "atlas.vtex");
    match read_result {
        Err(err) => assert!(err.to_string().contains("morph set"), "{err}"),
        Ok(set) => {
            let err = set.targets(&[0u8; 4 * 4 * 4], (4, 4), 16).unwrap_err();
            assert!(err.to_string().contains("morph set"), "{err}");
        }
    }
}

/// P0 #2c: MorphSet::targets caps total target buffer memory so 256 x 4M vertices cannot OOM,
/// and `targets_in` enforces a caller-provided `Budget`.
#[test]
fn morph_set_oversized_flex_vertex_count_returns_err() {
    let flexes: Vec<K> = (0..256)
        .map(|_| K::Obj(vec![("m_morphRectDatas", K::Arr(vec![]))]))
        .collect();
    let kv = kv3::decode(&kv3_file(&K::Obj(vec![
        ("m_nWidth", K::UInt(2048)),
        ("m_nHeight", K::UInt(2048)),
        ("m_morphDatas", K::Arr(flexes)),
    ])))
    .unwrap();
    let set = MorphSet::read(&kv.root, "atlas.vtex").unwrap();
    let err = set.targets(&[0u8; 4], (1, 1), 1 << 22).unwrap_err();
    assert!(err.to_string().contains("morph set"), "{err}");

    let mut tiny = Budget::new(16);
    let err = set
        .targets_in(&[0u8; 4], (1, 1), 16, &mut tiny)
        .unwrap_err();
    assert!(err.to_string().contains("more than it may"), "{err}");
}

/// P0 #3a: `Texture::parse_in` checks PNG output size against `Budget` before allocating.
#[test]
fn vtex_png_respects_budget_before_allocating() {
    // Build a tiny valid 64x64 RGBA PNG (16 KiB decoded) and parse it with a 1 KiB budget.
    let small = Texture::from_rgba8(64, 64, vec![0u8; 64 * 64 * 4]).unwrap();
    let png = small.to_png().unwrap();
    let bytes = vtex_bytes(64, 64, 16, &[], &png);

    let mut tight = Budget::new(1024);
    let err = Texture::parse_in(&bytes, &mut tight)
        .expect_err("must reject PNG exceeding budget before allocation");
    assert!(err.to_string().contains("more than it may"), "{err}");

    // With sufficient budget, the same PNG vtex parses cleanly.
    let parsed = Texture::parse(&bytes).expect("valid PNG vtex");
    assert_eq!((parsed.width(), parsed.height()), (64, 64));
}

/// P0 #3b: `Texture::first_frame` with an empty or inverted frame rect whose `fx0`/`fy0` is
/// off the block grid must return the whole texture instead of underflowing or cropping a sliver.
#[test]
fn vtex_first_frame_empty_or_inverted_rect_with_nonzero_origin_returns_whole() {
    for rect in [
        [5.0 / 16.0, 5.0 / 16.0, 5.0 / 16.0, 5.0 / 16.0],
        [6.0 / 16.0, 6.0 / 16.0, 5.0 / 16.0, 5.0 / 16.0],
    ] {
        let bytes = vtex_bytes(16, 16, 1, &[(2, sheet_extra(rect))], &[0xAB; 128]);
        let texture = Texture::parse(&bytes).unwrap();
        let frame = texture.first_frame();
        assert_eq!((frame.width(), frame.height()), (16, 16), "rect={rect:?}");
        assert!(!frame.is_sheet());
    }
}

/// A mock `Source` that reports a huge 4 GiB file size and panics if `read_at` is asked to
/// allocate more than a tiny header.
struct HugeMockSource {
    dir_header: Vec<u8>,
    reported_size: u64,
    excessive_read: AtomicBool,
}

impl Source for HugeMockSource {
    fn size(&self, _path: &str) -> heatel::Result<u64> {
        Ok(self.reported_size)
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> heatel::Result<Vec<u8>> {
        if len > 1024 * 1024 {
            self.excessive_read.store(true, Ordering::SeqCst);
            return Err(Error::OutOfRange {
                path: path.to_owned(),
                offset,
                len,
                size: self.reported_size,
            });
        }
        let start = offset as usize;
        let end = start + len;
        if end <= self.dir_header.len() {
            Ok(self.dir_header[start..end].to_vec())
        } else {
            Ok(vec![0u8; len])
        }
    }
}

/// P0 #4a: `Vpk::open` rejects an untrusted 4 GiB `tree_len` before calling `src.read_at`.
#[test]
fn vpk_open_caps_untrusted_tree_len() {
    let mut header = Vec::new();
    for w in [0x55AA_1234u32, 2, u32::MAX, 0, 0, 0, 0] {
        header.extend_from_slice(&w.to_le_bytes());
    }
    let src = HugeMockSource {
        dir_header: header,
        reported_size: u64::from(u32::MAX) + 64,
        excessive_read: AtomicBool::new(false),
    };
    let err = Vpk::open(&src, "pak01_dir.vpk")
        .err()
        .expect("must reject huge tree_len");
    assert!(
        !src.excessive_read.load(Ordering::SeqCst),
        "must reject before read_at"
    );
    assert!(err.to_string().contains("VPK"), "{err}");
}

/// P0 #4b: `Vpk::read` rejects an untrusted 4 GiB `archive_len` before calling `src.read_at`.
#[test]
fn vpk_read_caps_untrusted_archive_len() {
    let mut tree = Vec::new();
    tree.extend_from_slice(b"bin\0");
    tree.extend_from_slice(b" \0");
    tree.extend_from_slice(b"huge\0");
    tree.extend_from_slice(&0u32.to_le_bytes()); // crc
    tree.extend_from_slice(&0u16.to_le_bytes()); // preload
    tree.extend_from_slice(&0x7FFFu16.to_le_bytes()); // archive_index = in dir
    tree.extend_from_slice(&0u32.to_le_bytes()); // offset
    tree.extend_from_slice(&u32::MAX.to_le_bytes()); // archive_len = 4 GiB - 1
    tree.extend_from_slice(&0xFFFFu16.to_le_bytes()); // terminator
    tree.extend_from_slice(&[0, 0, 0]);

    let mut dir = Vec::new();
    for w in [0x55AA_1234u32, 2, tree.len() as u32, 0, 0, 0, 0] {
        dir.extend_from_slice(&w.to_le_bytes());
    }
    dir.extend_from_slice(&tree);

    let src = HugeMockSource {
        dir_header: dir,
        reported_size: u64::from(u32::MAX) + 1024,
        excessive_read: AtomicBool::new(false),
    };
    let vpk = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let err = vpk.read_file(&src, "huge.bin").unwrap_err();
    assert!(
        !src.excessive_read.load(Ordering::SeqCst),
        "must reject before read_at"
    );
    assert!(err.to_string().contains("VPK"), "{err}");
}

/// P0 #5a: `particle::System` `Deserialize` must enforce `validate()`.
#[cfg(feature = "serde")]
#[test]
fn particle_system_deserialize_enforces_validate() {
    use heatel::particle::System;

    let doc = kv3::decode(&kv3_file(&K::Obj(vec![(
        "_class",
        s("CParticleSystemDefinition"),
    )])))
    .unwrap();
    let valid = System::from_document(&doc).unwrap();
    let base = serde_json::to_value(&valid).unwrap();
    // Sanity check: valid round-trip works.
    let roundtrip: System = serde_json::from_value(base.clone()).unwrap();
    assert_eq!(roundtrip, valid);

    for (field, bad) in [
        ("max_particles", serde_json::json!(0)),
        ("max_particles", serde_json::json!(999_999)),
        ("max_time_step", serde_json::json!(-1.0)),
        ("sleep_after", serde_json::json!(-5.0)),
        ("snapshot_file", serde_json::json!("../etc/passwd")),
    ] {
        let mut mutated = base.clone();
        mutated[field] = bad;
        assert!(
            serde_json::from_value::<System>(mutated).is_err(),
            "Deserialize accepted invalid {field}"
        );
    }
}

/// P0 #5b: `world::lighting` must reject `NaN` in `origin` / `outer_radius` rather than returning
/// `NaN` weights through `.clamp(0.0, 1.0)`.
#[test]
fn world_lighting_rejects_nan_light_radius_and_origin() {
    let make_entities = |outer: f64| {
        let kv = kv3::decode(&kv3_file(&K::Obj(vec![(
            "entities",
            K::Arr(vec![
                K::Obj(vec![
                    ("classname", s("ent_dota_lightinfo")),
                    ("targetname", s("[PR#]default_main")),
                    (
                        "origin",
                        K::Arr(vec![K::Float(0.0), K::Float(0.0), K::Float(0.0)]),
                    ),
                    ("inner_radius", K::Float(0.0)),
                    ("outer_radius", K::Float(outer)),
                    (
                        "color_day",
                        K::Arr(vec![K::Float(255.0), K::Float(255.0), K::Float(255.0)]),
                    ),
                    ("light_scale_day", K::Float(1.0)),
                    (
                        "color_night",
                        K::Arr(vec![K::Float(100.0), K::Float(100.0), K::Float(100.0)]),
                    ),
                    ("light_scale_night", K::Float(1.0)),
                    (
                        "ambient_color_day",
                        K::Arr(vec![K::Float(128.0), K::Float(128.0), K::Float(128.0)]),
                    ),
                    ("ambient_scale_day", K::Float(1.0)),
                    (
                        "ambient_color_night",
                        K::Arr(vec![K::Float(64.0), K::Float(64.0), K::Float(64.0)]),
                    ),
                    ("ambient_scale_night", K::Float(1.0)),
                ]),
                K::Obj(vec![
                    ("classname", s("env_fog_controller")),
                    ("foglerptime", K::Float(5.0)),
                ]),
            ]),
        )])))
        .unwrap();
        kv.root
            .get("entities")
            .and_then(kv3::Value::as_array)
            .unwrap()
            .to_vec()
    };
    let entities = make_entities(f64::NAN);
    assert!(
        lighting(&entities).is_err(),
        "NaN outer_radius must not produce Ok(NaN)"
    );
}

/// P0 #5c: `sound::Lipsync` must guard `NaN` ease values before `.clamp(0.0, 60.0)`.
#[test]
fn sound_lipsync_nan_ease_is_guarded() {
    let ctrl = K::Obj(vec![(
        "m_vSound",
        K::Obj(vec![(
            "m_Sentences",
            K::Arr(vec![K::Obj(vec![(
                "m_morphData",
                K::Obj(vec![
                    ("m_times", K::Arr(vec![K::Float(0.0)])),
                    ("m_nameHashCodes", K::Arr(vec![K::UInt(1)])),
                    ("m_samples", K::Arr(vec![K::Arr(vec![K::Float(0.5)])])),
                    ("m_flEaseIn", K::Float(f64::NAN)),
                    ("m_flEaseOut", K::Float(f64::NAN)),
                ]),
            )])]),
        )]),
    )]);
    let bytes = resource(&[(b"CTRL", kv3_file(&ctrl))]);
    let parsed = Lipsync::parse(&bytes, &mut Budget::default()).unwrap();
    let morph = parsed.morph.expect("morph curves");
    assert!(morph.ease_in.is_finite(), "ease_in was {}", morph.ease_in);
    assert!(
        morph.ease_out.is_finite(),
        "ease_out was {}",
        morph.ease_out
    );
    assert_eq!(morph.ease_in, 0.0);
    assert_eq!(morph.ease_out, 0.0);
}
