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
#[path = "support/wire_builders.rs"]
mod wire;
use fixture::{K, kv3_file, s};
use wire::{
    minimal_png, png_chunk, resource, sheet_extra, vpk_v1, vpk_v2, vtex_bytes, words, zlib_stored,
};

fn anim_resource(group: K, anim: K) -> Vec<u8> {
    let ctrl = K::Obj(vec![(
        "embedded_animation",
        K::Obj(vec![
            ("group_data_block", K::Int(2)),
            ("anim_data_block", K::Int(3)),
        ]),
    )]);
    let data = K::Obj(vec![("m_modelSkeleton", K::Obj(vec![]))]);
    resource(&[
        (b"CTRL", kv3_file(&ctrl)),
        (b"DATA", kv3_file(&data)),
        (b"AGRP", kv3_file(&group)),
        (b"ANIM", kv3_file(&anim)),
    ])
}

/// P0 #1: A clip with `m_nFrames = u64::MAX`, 0 bones, and 0 frame blocks must fail
/// instantaneously instead of spinning for 2^64 iterations.
#[test]
fn anim_clip_zero_bones_u64_max_frames_returns_err_immediately() {
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
    let bytes = anim_resource(K::Obj(vec![("m_decodeKey", K::Obj(vec![]))]), anim);
    let err = read_clips(&Resource::parse(&bytes).expect("valid resource"), &["spin"])
        .err()
        .expect("must reject u64::MAX frames");
    assert!(err.to_string().contains("animation"), "{err}");
}

/// P0 #1b: Repeated references to a cached `None` segment in `m_frameblockArray` must charge
/// `budget.work` so they cannot spin uncharged.
#[test]
fn anim_clip_cached_none_segments_charge_work_budget() {
    let group = K::Obj(vec![(
        "m_decodeKey",
        K::Obj(vec![(
            "m_dataChannelArray",
            K::Arr(vec![K::Obj(vec![
                ("m_szVariableName", s("Unknown")),
                ("m_nElementNameArray", K::Arr(vec![])),
                ("m_nElementIndexArray", K::Arr(vec![])),
            ])]),
        )]),
    )]);
    // 1000 frames * 300_000 segment visits = 3e8 > MAX_WORK (2^28 = 2.68e8).
    let seg_indices: Vec<K> = (0..300_000).map(|_| K::UInt(0)).collect();
    let anim = K::Obj(vec![
        (
            "m_decoderArray",
            K::Arr(vec![K::Obj(vec![(
                "m_szName",
                s("CCompressedFullVector3"),
            )])]),
        ),
        (
            "m_segmentArray",
            K::Arr(vec![K::Obj(vec![
                ("m_container", K::Blob(vec![0; 8])),
                ("m_nLocalChannel", K::UInt(0)),
            ])]),
        ),
        (
            "m_animArray",
            K::Arr(vec![K::Obj(vec![
                ("m_name", s("none_segs")),
                ("fps", K::Float(30.0)),
                (
                    "m_pData",
                    K::Obj(vec![
                        ("m_nFrames", K::UInt(1000)),
                        (
                            "m_frameblockArray",
                            K::Arr(vec![K::Obj(vec![
                                ("m_nStartFrame", K::UInt(0)),
                                ("m_nEndFrame", K::UInt(1000)),
                                ("m_segmentIndexArray", K::Arr(seg_indices)),
                            ])]),
                        ),
                    ]),
                ),
            ])]),
        ),
    ]);
    let bytes = anim_resource(group, anim);
    let err = read_clips(
        &Resource::parse(&bytes).expect("valid resource"),
        &["none_segs"],
    )
    .err()
    .expect("must exhaust work budget on repeated cached None segments");
    assert!(err.to_string().contains("segment visits"), "{err}");
}

/// P0 #1c: Per-frame `m_movementArray` scans must charge `budget.work`.
#[test]
fn anim_clip_many_movements_charge_work_budget() {
    // 10_000 frames * 30_000 movements = 3e8 > MAX_WORK (2^28).
    let moves: Vec<K> = (0..30_000)
        .map(|i| {
            K::Obj(vec![
                ("endframe", K::Int(i)),
                ("angle", K::Float(0.0)),
                ("position", K::Arr(vec![K::Float(0.0); 3])),
            ])
        })
        .collect();
    let anim = K::Obj(vec![(
        "m_animArray",
        K::Arr(vec![K::Obj(vec![
            ("m_name", s("walk")),
            ("fps", K::Float(30.0)),
            ("m_movementArray", K::Arr(moves)),
            (
                "m_pData",
                K::Obj(vec![
                    ("m_nFrames", K::UInt(10_000)),
                    ("m_frameblockArray", K::Arr(vec![])),
                ]),
            ),
        ])]),
    )]);
    let bytes = anim_resource(K::Obj(vec![("m_decodeKey", K::Obj(vec![]))]), anim);
    let err = read_clips(&Resource::parse(&bytes).expect("valid resource"), &["walk"])
        .err()
        .expect("must exhaust work budget on many movements");
    assert!(err.to_string().contains("movements"), "{err}");
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

fn morph_rect(x: u64, y: u64, w: f64, h: f64, u: f64, v: f64) -> K {
    K::Obj(vec![
        ("m_nXLeftDst", K::UInt(x)),
        ("m_nYTopDst", K::UInt(y)),
        ("m_flUWidthSrc", K::Float(w)),
        ("m_flVHeightSrc", K::Float(h)),
        (
            "m_bundleDatas",
            K::Arr(vec![K::Obj(vec![
                ("m_flULeftSrc", K::Float(u)),
                ("m_flVTopSrc", K::Float(v)),
                ("m_offsets", K::Arr(vec![K::Float(0.0); 4])),
                ("m_ranges", K::Arr(vec![K::Float(1.0); 4])),
            ])]),
        ),
    ])
}

fn morph_doc(rects: Vec<K>) -> kv3::Document {
    kv3::decode(&kv3_file(&K::Obj(vec![
        ("m_nWidth", K::UInt(4)),
        ("m_nHeight", K::UInt(4)),
        (
            "m_bundleTypes",
            K::Arr(vec![s("MORPH_BUNDLE_TYPE_POSITION_SPEED")]),
        ),
        (
            "m_morphDatas",
            K::Arr(vec![K::Obj(vec![("m_morphRectDatas", K::Arr(rects))])]),
        ),
    ])))
    .unwrap()
}

/// P0 #2b: MorphSet::read rejects rectangles with `rect.x = usize::MAX` directly, and `place()`
/// rejects rectangles that start inside the grid but extend past it.
#[test]
fn morph_set_rect_bounds_and_uv_validation() {
    let kv = morph_doc(vec![morph_rect(usize::MAX as u64, 0, 0.25, 0.25, 0.0, 0.0)]);
    let err = MorphSet::read(&kv.root, "atlas.vtex").unwrap_err();
    assert!(err.to_string().contains("past the grid"), "{err}");

    for (u, v) in [(-0.1, 0.0), (0.0, -0.1), (1.1, 0.0), (0.0, 1.1)] {
        let kv = morph_doc(vec![morph_rect(0, 0, 0.25, 0.25, u, v)]);
        let err = MorphSet::read(&kv.root, "atlas.vtex").unwrap_err();
        assert!(err.to_string().contains("outside the atlas"), "{err}");
    }

    let kv = morph_doc(vec![morph_rect(3, 0, 0.5, 0.5, 0.0, 0.0)]);
    let set = MorphSet::read(&kv.root, "atlas.vtex").unwrap();
    let err = set.targets(&[0u8; 4 * 4 * 4], (4, 4), 16).unwrap_err();
    assert!(err.to_string().contains("past the grid"), "{err}");
}

/// P0 #2c: MorphSet::targets caps total target buffer memory AND charges overlapping rectangles
/// in `place()` against `Budget`.
#[test]
fn morph_set_oversized_flex_vertex_count_and_overlapping_rects_respect_budget() {
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

    let rects: Vec<K> = (0..100)
        .map(|_| morph_rect(0, 0, 1.0, 1.0, 0.0, 0.0))
        .collect();
    let kv = morph_doc(rects);
    let set = MorphSet::read(&kv.root, "atlas.vtex").unwrap();
    let mut budget = Budget::new(2048);
    let err = set
        .targets_in(&[0u8; 4 * 4 * 4], (4, 4), 16, &mut budget)
        .unwrap_err();
    assert!(err.to_string().contains("more than it may"), "{err}");
}

/// P0 #3a: `Texture::parse_in` enforces `Budget` on PNG decode, `zTXt` metadata chunks, in-place
/// Grayscale/RGB/GrayAlpha expansion, and non-PNG LZ4 mip levels.
#[test]
fn vtex_png_and_non_png_respect_budget_and_expand_in_place() {
    // 1) PNG output buffer exceeds tight budget.
    let small = Texture::from_rgba8(64, 64, vec![0u8; 64 * 64 * 4]).unwrap();
    let png = small.to_png().unwrap();
    let bytes = vtex_bytes(64, 64, 16, &[], &png);
    let mut tight = Budget::new(1024);
    let err = Texture::parse_in(&bytes, &mut tight)
        .expect_err("must reject PNG exceeding budget before allocation");
    assert!(err.to_string().contains("more than it may"), "{err}");

    // 2) PNG with large zTXt chunk rejected by png::Limits during read_info().
    let mut ztxt = b"comment\0\0".to_vec();
    ztxt.extend_from_slice(&zlib_stored(&vec![b'A'; 4096]));
    let ztxt_png = minimal_png(1, 1, 6, &[&[1, 2, 3, 4]], &[png_chunk(b"zTXt", &ztxt)]);
    let ztxt_vtex = vtex_bytes(1, 1, 16, &[], &ztxt_png);
    let mut tiny = Budget::new(512);
    assert!(
        Texture::parse_in(&ztxt_vtex, &mut tiny).is_err(),
        "zTXt chunk exceeding budget must fail during read_info"
    );

    // 3) In-place back-to-front expansion for RGB, GrayscaleAlpha, and Grayscale PNGs.
    let rgb_png = minimal_png(2, 1, 2, &[&[10, 20, 30, 40, 50, 60]], &[]);
    let rgb = Texture::parse(&vtex_bytes(2, 1, 16, &[], &rgb_png)).unwrap();
    assert_eq!(rgb.mips()[0], [10, 20, 30, 255, 40, 50, 60, 255]);

    let ga_png = minimal_png(2, 1, 4, &[&[11, 22, 33, 44]], &[]);
    let ga = Texture::parse(&vtex_bytes(2, 1, 16, &[], &ga_png)).unwrap();
    assert_eq!(ga.mips()[0], [11, 11, 11, 22, 33, 33, 33, 44]);

    let gray_png = minimal_png(2, 1, 0, &[&[77, 88]], &[]);
    let gray = Texture::parse(&vtex_bytes(2, 1, 16, &[], &gray_png)).unwrap();
    assert_eq!(gray.mips()[0], [77, 77, 77, 255, 88, 88, 88, 255]);

    // 4) Non-PNG LZ4-compressed texture charges `Budget` before decompressing.
    let plain = vec![0x5Au8; 64 * 64 * 4];
    let compressed = lz4_flex::block::compress(&plain);
    let sizes = words(&[1, 8, 1, compressed.len() as u32]);
    let lz4_vtex = vtex_bytes(64, 64, 4, &[(4, sizes)], &compressed);
    let mut tiny = Budget::new(1024);
    let err = Texture::parse_in(&lz4_vtex, &mut tiny)
        .expect_err("must reject non-PNG LZ4 texture exceeding budget");
    assert!(err.to_string().contains("more than it may"), "{err}");
    let ok = Texture::parse(&lz4_vtex).expect("valid LZ4 vtex");
    assert_eq!(ok.mips()[0], plain);
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

    let dir = vpk_v2(&tree, &[]);

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

/// P0 #4c: Synthetic NTRO + DATA tests covering past-EOF and before-BOF pointers/arrays
/// (including `i32::MIN` and `i32::MAX`) and `field.offset` past-EOF.
#[test]
fn ntro_rejects_out_of_bounds_pointers_arrays_and_field_offsets() {
    // Minimal NTRO block with 1 struct and 1 INT32 field, optional indirection
    // (3 = RESOURCE_POINTER, 4 = RESOURCE_ARRAY) and `field_offset`.
    let build_ntro = |field_offset: u16, indirection: Option<u8>, data_block: Vec<u8>| -> Vec<u8> {
        let mut ntro = Vec::new();
        ntro.extend_from_slice(&4u32.to_le_bytes());
        ntro.extend_from_slice(&16u32.to_le_bytes()); // structs offset from pos 4 -> 20
        ntro.extend_from_slice(&1u32.to_le_bytes()); // 1 struct
        ntro.extend_from_slice(&0u32.to_le_bytes()); // enums offset
        ntro.extend_from_slice(&0u32.to_le_bytes()); // 0 enums

        // Struct at pos 20 (40 bytes -> ends at 60).
        ntro.extend_from_slice(&0u32.to_le_bytes()); // version
        ntro.extend_from_slice(&1u32.to_le_bytes()); // id
        ntro.extend_from_slice(&0u32.to_le_bytes()); // name = ""
        ntro.extend_from_slice(&0u32.to_le_bytes()); // crc
        ntro.extend_from_slice(&0u32.to_le_bytes()); // user_ver
        ntro.extend_from_slice(&8u16.to_le_bytes()); // size
        ntro.extend_from_slice(&4u16.to_le_bytes()); // align
        ntro.extend_from_slice(&0u32.to_le_bytes()); // base
        ntro.extend_from_slice(&12u32.to_le_bytes()); // fields offset from pos 48 -> 60
        ntro.extend_from_slice(&1u32.to_le_bytes()); // 1 field
        ntro.extend_from_slice(&[0u8; 4]); // flags + pad

        // Field at pos 60 (24 bytes -> ends at 84).
        ntro.extend_from_slice(&0u32.to_le_bytes()); // name = ""
        ntro.extend_from_slice(&0u16.to_le_bytes()); // count
        ntro.extend_from_slice(&field_offset.to_le_bytes());
        if let Some(ind) = indirection {
            ntro.extend_from_slice(&16u32.to_le_bytes()); // ind offset from pos 68 -> 84
            ntro.extend_from_slice(&1u32.to_le_bytes());
            ntro.extend_from_slice(&0u32.to_le_bytes()); // type_data
            ntro.extend_from_slice(&14u16.to_le_bytes()); // ty::INT32
            ntro.extend_from_slice(&0u16.to_le_bytes());
            ntro.push(ind);
        } else {
            ntro.extend_from_slice(&0u32.to_le_bytes());
            ntro.extend_from_slice(&0u32.to_le_bytes());
            ntro.extend_from_slice(&0u32.to_le_bytes());
            ntro.extend_from_slice(&14u16.to_le_bytes()); // ty::INT32
            ntro.extend_from_slice(&0u16.to_le_bytes());
        }
        resource(&[(b"NTRO", ntro), (b"DATA", data_block)])
    };

    for ptr_offset in [i32::MAX, i32::MIN] {
        let bytes = build_ntro(0, Some(3), ptr_offset.to_le_bytes().to_vec());
        let res = Resource::parse(&bytes).unwrap();
        let err = res.data().unwrap().unwrap_err();
        assert!(err.to_string().contains("NTRO"), "{err}");
    }

    for arr_offset in [i32::MAX, i32::MIN] {
        let mut data = arr_offset.to_le_bytes().to_vec();
        data.extend_from_slice(&1u32.to_le_bytes());
        let bytes = build_ntro(0, Some(4), data);
        let res = Resource::parse(&bytes).unwrap();
        let err = res.data().unwrap().unwrap_err();
        assert!(err.to_string().contains("NTRO"), "{err}");
    }

    let bytes = build_ntro(30_000, None, vec![0; 8]);
    let res = Resource::parse(&bytes).unwrap();
    let err = res.data().unwrap().unwrap_err();
    assert!(err.to_string().contains("NTRO"), "{err}");
}

/// P0 #5a: `particle::System` `Deserialize` must enforce `validate()`, including rejecting
/// non-finite floats (such as JSON `1e300` -> `f32::INFINITY`), `Look` layer/path bounds,
/// and `Curve::keys` / `Gradient::stops` caps.
#[cfg(feature = "serde")]
#[test]
fn particle_system_deserialize_enforces_validate() {
    use heatel::particle::System;

    let doc = kv3::decode(&kv3_file(&K::Obj(vec![
        ("_class", s("CParticleSystemDefinition")),
        (
            "m_Renderers",
            K::Arr(vec![K::Obj(vec![("_class", s("C_OP_RenderSprites"))])]),
        ),
        (
            "m_Operators",
            K::Arr(vec![K::Obj(vec![
                ("_class", s("C_OP_RemapSpeed")),
                ("m_nFieldOutput", K::Int(4)),
            ])]),
        ),
    ])))
    .unwrap();
    let valid = System::from_document(&doc).unwrap();
    let base = serde_json::to_value(&valid).unwrap();
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

    // JSON 1e300 overflows f32 to INFINITY: rejected in Timing, FloatInput, and Operator.
    let mut inf_timing = base.clone();
    inf_timing["operators"][0]["timing"]["fade_in"] = serde_json::json!([1e300, 0.0]);
    assert!(serde_json::from_value::<System>(inf_timing).is_err());

    let mut inf_input = base.clone();
    inf_input["operators"][0]["timing"]["strength"] = serde_json::json!({"Literal": 1e300});
    assert!(serde_json::from_value::<System>(inf_input).is_err());

    let mut inf_op = base.clone();
    inf_op["operators"][0]["class"]["RemapSpeed"]["output"] = serde_json::json!([0.0, 1e300]);
    assert!(serde_json::from_value::<System>(inf_op).is_err());

    // Look layer count (> 8) and texture path length (> 256) rejected on Sprites.
    let mut too_many_layers = base.clone();
    let layer = serde_json::json!({
        "texture": "materials/a.vtex",
        "channels": "Rgba",
        "blend": "Multiply",
        "transform": [1.0, 0.0, 1.0, 0.0],
    });
    too_many_layers["renderers"][0]["class"]["Sprites"]["look"]["layers"] =
        serde_json::Value::Array(vec![layer; 9]);
    assert!(serde_json::from_value::<System>(too_many_layers).is_err());

    let mut long_tex = base.clone();
    long_tex["renderers"][0]["class"]["Sprites"]["look"]["texture"] =
        serde_json::json!("a".repeat(257));
    assert!(serde_json::from_value::<System>(long_tex).is_err());

    // Curve::keys (> 64) and VectorInput::Gradient::stops (> 64) rejected.
    let mut huge_curve = base.clone();
    huge_curve["operators"][0]["timing"]["strength"] = serde_json::json!({
        "CollectionAge": {
            "Curve": {
                "keys": vec![[0.0, 0.0, 0.0, 0.0]; 65],
                "domain_min": [0.0, 0.0],
                "domain_max": [1.0, 1.0],
                "looped": false,
            }
        }
    });
    assert!(serde_json::from_value::<System>(huge_curve).is_err());

    let mut huge_gradient = base.clone();
    huge_gradient["renderers"][0]["class"]["Sprites"]["look"]["color_scale"] = serde_json::json!({
        "Gradient": {
            "input": {"Literal": 0.5},
            "range": [0.0, 1.0],
            "stops": vec![[0.0, 1.0, 1.0, 1.0]; 65],
        }
    });
    assert!(serde_json::from_value::<System>(huge_gradient).is_err());
}

/// P0 #5b & LOW-1: `world::lighting` must reject `NaN` across all numeric fields and reject
/// `f64::MAX` weighted-sum overflow in `blend()`.
#[test]
fn world_lighting_rejects_nan_fields_and_blend_overflow() {
    let make_light = |origin_x: f64, inner: f64, outer: f64, color_r: f64| {
        K::Obj(vec![
            ("classname", s("ent_dota_lightinfo")),
            ("targetname", s("[PR#]default_main")),
            (
                "origin",
                K::Arr(vec![K::Float(origin_x), K::Float(0.0), K::Float(0.0)]),
            ),
            ("inner_radius", K::Float(inner)),
            ("outer_radius", K::Float(outer)),
            (
                "color_day",
                K::Arr(vec![K::Float(color_r), K::Float(255.0), K::Float(255.0)]),
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
        ])
    };
    let decode_entities = |mut list: Vec<K>, fog: f64| {
        list.push(K::Obj(vec![
            ("classname", s("env_fog_controller")),
            ("foglerptime", K::Float(fog)),
        ]));
        let kv = kv3::decode(&kv3_file(&K::Obj(vec![("entities", K::Arr(list))]))).unwrap();
        kv.root
            .get("entities")
            .and_then(kv3::Value::as_array)
            .unwrap()
            .to_vec()
    };

    for (label, entities) in [
        (
            "nan origin",
            decode_entities(vec![make_light(f64::NAN, 0.0, 100.0, 255.0)], 5.0),
        ),
        (
            "nan inner",
            decode_entities(vec![make_light(0.0, f64::NAN, 100.0, 255.0)], 5.0),
        ),
        (
            "nan outer",
            decode_entities(vec![make_light(0.0, 0.0, f64::NAN, 255.0)], 5.0),
        ),
        (
            "nan color",
            decode_entities(vec![make_light(0.0, 0.0, 100.0, f64::NAN)], 5.0),
        ),
        (
            "nan fog",
            decode_entities(vec![make_light(0.0, 0.0, 100.0, 255.0)], f64::NAN),
        ),
    ] {
        assert!(lighting(&entities).is_err(), "{label} must return Err");
    }

    // 3 lights with weight 0.3 each: total = 0.3 + 0.3 + 0.3 < 0.9 in IEEE-754, so summing
    // 3 * (f64::MAX * 0.3 / total) overflows to INFINITY and must be rejected.
    let overflow = decode_entities(
        vec![
            make_light(70.0, 0.0, 100.0, f64::MAX),
            make_light(70.0, 0.0, 100.0, f64::MAX),
            make_light(70.0, 0.0, 100.0, f64::MAX),
        ],
        5.0,
    );
    assert!(
        lighting(&overflow).is_err(),
        "f64::MAX weighted blend overflow must return Err"
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

/// End-to-end `Budget` threading: `Vpk::open_in`/`read_file_in`, `Texture::from_resource_in`,
/// `Resource::data_in`, `Model::read_in`, transient `zstd` peak headroom, `export_model_in_group`,
/// and `world::export_world`.
#[test]
fn end_to_end_budget_is_shared_across_readers_and_exporters() {
    use heatel::export::{ModelOptions, export_model_in_group};
    use heatel::world::{WorldOptions, export_world};
    use heatel::{Model, SliceSource};

    // 1) Vpk::open_in and Vpk::read_file_in enforce caller budget.
    let payload = vec![0x42u8; 2048];
    let (pkg, _) = vpk_v1(&[("dir/data.bin", payload.clone())]);
    let mut src = SliceSource::default();
    src.insert("pak01_dir.vpk", pkg);
    assert!(Vpk::open_in(&src, "pak01_dir.vpk", &mut Budget::new(8)).is_err());
    let vpk = Vpk::open_in(&src, "pak01_dir.vpk", &mut Budget::new(4096)).unwrap();
    assert!(
        vpk.read_file_in(&src, "dir/data.bin", &mut Budget::new(512))
            .is_err()
    );
    let mut enough = Budget::new(4096);
    assert_eq!(
        vpk.read_file_in(&src, "dir/data.bin", &mut enough).unwrap(),
        payload
    );
    assert_eq!(enough.spent(), payload.len());

    // 2) Texture::from_resource_in and Resource::data_in enforce caller budget.
    let vtex = vtex_bytes(8, 8, 4, &[], &vec![0u8; 8 * 8 * 4]);
    let res = Resource::parse(&vtex).unwrap();
    assert!(Texture::from_resource_in(&res, &mut Budget::new(64)).is_err());
    let mut tex_budget = Budget::new(1024);
    assert!(Texture::from_resource_in(&res, &mut tex_budget).is_ok());
    assert!(tex_budget.spent() >= 8 * 8 * 4);

    let big_data = resource(&[(
        b"DATA",
        kv3_file(&K::Obj(vec![("s", s(&"a".repeat(4096)))])),
    )]);
    let res = Resource::parse(&big_data).unwrap();
    assert!(res.data_in(&mut Budget::new(512)).unwrap().is_err());
    assert!(res.data_in(&mut Budget::new(32 * 1024)).unwrap().is_ok());

    // 3) Model::read_in charges MDAT KV3 decoding and checks zstd window as transient peak
    //    (two consecutive zstd vertex buffers fit in 8 MiB even though zstd_worst is ~6 MiB each).
    let zstd_frame = |raw: &[u8]| {
        let mut f = 0xFD2F_B528u32.to_le_bytes().to_vec();
        f.extend([0x20, raw.len() as u8]); // single-segment, 1-byte content size
        let hdr = ((raw.len() as u32) << 3) | 1; // raw last block
        f.extend(&hdr.to_le_bytes()[..3]);
        f.extend_from_slice(raw);
        f
    };
    let pos_raw = [0.0f32; 3]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect::<Vec<_>>();
    let buf_desc = |blk: i64| {
        K::Obj(vec![
            ("m_nElementCount", K::UInt(1)),
            ("m_nElementSizeInBytes", K::UInt(12)),
            ("m_nBlockIndex", K::Int(blk)),
            ("m_bCompressedZSTD", K::Bool(true)),
            (
                "m_inputLayoutFields",
                K::Arr(vec![K::Obj(vec![
                    ("m_pSemanticName", s("POSITION")),
                    ("m_Format", K::UInt(6)),
                    ("m_nOffset", K::UInt(0)),
                ])]),
            ),
        ])
    };
    let ctrl = K::Obj(vec![(
        "embedded_meshes",
        K::Arr(vec![K::Obj(vec![
            ("name", s("m")),
            ("mesh_index", K::Int(0)),
            ("data_block", K::Int(2)),
            ("m_vertexBuffers", K::Arr(vec![buf_desc(3), buf_desc(4)])),
            ("m_indexBuffers", K::Arr(vec![])),
        ])]),
    )]);
    let mdat = K::Obj(vec![("pad", s(&"m".repeat(4096)))]);
    let mdl = resource(&[
        (b"DATA", kv3_file(&K::Obj(vec![]))),
        (b"CTRL", kv3_file(&ctrl)),
        (b"MDAT", kv3_file(&mdat)),
        (b"MVTX", zstd_frame(&pos_raw)),
        (b"MVTX", zstd_frame(&pos_raw)),
    ]);
    let res = Resource::parse(&mdl).unwrap();
    assert!(Model::read_in(&res, &mut Budget::new(1024)).is_err());
    let mut zstd_budget = Budget::new(8 << 20);
    assert!(Model::read_in(&res, &mut zstd_budget).is_ok());
    assert!(
        zstd_budget.spent() < 1 << 20,
        "zstd scratch must not be permanently charged"
    );

    // 4) export_model_in_group and world::export_world share budgets cleanly end-to-end.
    let world_data = resource(&[(b"DATA", kv3_file(&K::Obj(vec![])))]);
    let (pkg, _) = vpk_v1(&[
        ("models/m.vmdl_c", mdl),
        ("maps/w/world.vwrld_c", world_data),
    ]);
    let mut src = SliceSource::default();
    src.insert("pak01_dir.vpk", pkg);
    let vpk = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let uri = |p: &str| p.to_owned();
    let opts = ModelOptions::new(&uri);
    assert!(export_model_in_group(&src, &[&vpk], "models/m.vmdl_c", &opts, None).is_ok());
    let wopts = WorldOptions {
        band: None,
        exclude: &[],
        scene: "w",
        bin_uri: "w.bin",
        texture_uri: &uri,
        generator: "test",
    };
    assert!(export_world(&src, &[&vpk], "maps/w/world.vwrld_c", &wopts).is_ok());
}
