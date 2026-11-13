//! Synthetic snapshot resources: no installed-file bytes.
#[path = "common/kv3.rs"]
mod fixture;
use fixture::{K, kv3_file, s};
use heatel::{Resource, particle::Snapshot};
fn resource(data: Vec<u8>, snap: Vec<u8>) -> Vec<u8> {
    let blocks = [(b"DATA", data), (b"SNAP", snap)];
    let end = 40;
    let mut out = Vec::new();
    out.extend((end + blocks.iter().map(|(_, b)| b.len()).sum::<usize>() as u32).to_le_bytes());
    out.extend(12u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(8u32.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    let mut offset = end as usize;
    for (kind, bytes) in &blocks {
        out.extend(*kind);
        out.extend(((offset - out.len()) as u32).to_le_bytes());
        out.extend((bytes.len() as u32).to_le_bytes());
        offset += bytes.len();
    }
    for (_, bytes) in blocks {
        out.extend(bytes);
    }
    out
}
fn file(count: i64, offset: i64, values: &[f32], raw: bool) -> Vec<u8> {
    let root = K::Obj(vec![
        ("num_particles", K::Int(count)),
        (
            "attributes",
            K::Arr(vec![K::Obj(vec![
                ("name", s("position")),
                ("type", s("float3")),
                ("data_offset", K::Int(offset)),
                ("data_size", K::Int(values.len() as i64 * 4)),
            ])]),
        ),
        ("string_list", K::Arr(vec![])),
    ]);
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut packed = Vec::new();
    packed.extend((bytes.len() as u32 | if raw { 0x8000_0000 } else { 0 }).to_le_bytes());
    if raw {
        packed.extend(bytes);
    } else {
        for chunk in bytes.chunks(16) {
            packed.extend(0u16.to_le_bytes());
            packed.extend(chunk);
        }
    }
    resource(kv3_file(&root), packed)
}
#[test]
fn raw_and_compressed_snapshot_positions_match() {
    for raw in [false, true] {
        let bytes = file(2, 0, &[1.0, 2.0, 3.0, -4.0, 5.0, 6.0], raw);
        assert_eq!(
            Snapshot::read(&Resource::parse(&bytes).expect("resource")).expect("snapshot"),
            Snapshot::Points {
                positions: vec![[1.0, 2.0, 3.0], [-4.0, 5.0, 6.0]],
                normals: None
            }
        );
    }
}
#[test]
fn hostile_snapshot_counts_offsets_numbers_and_backreferences_are_errors() {
    for (bytes, expected) in [
        (file(5001, 0, &[], true), "invalid particle count"),
        (
            file(1, i64::MAX, &[0.0; 3], true),
            "invalid attribute offset",
        ),
        (file(1, 0, &[f32::NAN, 0.0, 0.0], true), "non-finite number"),
    ] {
        let error = Snapshot::read(&Resource::parse(&bytes).expect("resource"))
            .expect_err("hostile snapshot");
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut bytes = file(1, 0, &[0.0; 3], false);
    bytes.truncate(bytes.len() - 1);
    assert!(Resource::parse(&bytes).is_err());
}

#[test]
fn snapshot_backreferences_and_declared_output_are_bounded_before_allocation() {
    let base = file(1, 0, &[0.0; 3], true);
    let metadata = Resource::parse(&base)
        .expect("resource")
        .block(b"DATA")
        .expect("DATA")
        .to_vec();
    for (packed, expected) in [
        (vec![3, 0, 0, 0, 1, 0, 0, 0], "invalid SNAP back-reference"),
        (u32::MAX.to_le_bytes().to_vec(), "SNAP exceeds 1 MiB"),
    ] {
        let bytes = resource(metadata.clone(), packed);
        let error =
            Snapshot::read(&Resource::parse(&bytes).expect("resource")).expect_err("hostile SNAP");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn overlapping_backreferences_decode_without_reading_unwritten_data() {
    let base = file(1, 0, &[0.0; 3], true);
    let metadata = Resource::parse(&base)
        .expect("resource")
        .block(b"DATA")
        .expect("DATA")
        .to_vec();
    let bytes = resource(metadata, vec![12, 0, 0, 0, 2, 0, 0, 8, 0]);
    assert_eq!(
        Snapshot::read(&Resource::parse(&bytes).expect("resource")).expect("snapshot"),
        Snapshot::Points {
            positions: vec![[0.0; 3]],
            normals: None
        }
    );
    for end in 0..bytes.len() {
        assert!(
            Resource::parse(&bytes[..end])
                .and_then(|r| Snapshot::read(&r))
                .is_err(),
            "truncated at {end}"
        );
    }
}

#[test]
fn root_skinning_and_normals_are_retained_and_other_bones_are_named() {
    for name in ["root", "spine"] {
        let mut stream = Vec::new();
        for value in [10.0f32, 20.0, 30.0, 0.0, 0.0, 1.0] {
            stream.extend(value.to_le_bytes());
        }
        stream.extend([0u8; 8]);
        for value in [1.0f32, 0.0, 0.0, 0.0] {
            stream.extend(value.to_le_bytes());
        }
        let attributes = [
            ("position", "float3", 0, 12),
            ("normal", "float3", 12, 12),
            ("skinning", "skinning", 24, 24),
        ]
        .into_iter()
        .map(|(name, kind, offset, size)| {
            K::Obj(vec![
                ("name", s(name)),
                ("type", s(kind)),
                ("data_offset", K::Int(offset)),
                ("data_size", K::Int(size)),
            ])
        })
        .collect();
        let metadata = kv3_file(&K::Obj(vec![
            ("num_particles", K::Int(1)),
            ("attributes", K::Arr(attributes)),
            ("string_list", K::Arr(vec![s(name)])),
        ]));
        let packed = (0x8000_0000 | stream.len() as u32)
            .to_le_bytes()
            .into_iter()
            .chain(stream)
            .collect();
        let bytes = resource(metadata, packed);
        let decoded =
            Snapshot::read(&Resource::parse(&bytes).expect("resource")).expect("snapshot");
        if name == "root" {
            assert_eq!(
                decoded,
                Snapshot::Points {
                    positions: vec![[10.0, 20.0, 30.0]],
                    normals: Some(vec![[0.0, 0.0, 1.0]])
                }
            );
        } else {
            assert_eq!(decoded, Snapshot::Unsupported("skinning bone spine".into()));
        }
    }
}

fn attribute(name: &str, kind: &str, offset: i64, size: i64) -> K {
    K::Obj(vec![
        ("name", s(name)),
        ("type", s(kind)),
        ("data_offset", K::Int(offset)),
        ("data_size", K::Int(size)),
    ])
}
fn raw_snapshot(count: i64, attributes: Vec<K>, strings: Vec<K>, stream: &[u8]) -> Vec<u8> {
    let root = K::Obj(vec![
        ("num_particles", K::Int(count)),
        ("attributes", K::Arr(attributes)),
        ("string_list", K::Arr(strings)),
    ]);
    let mut packed = (stream.len() as u32 | 0x8000_0000).to_le_bytes().to_vec();
    packed.extend(stream);
    resource(kv3_file(&root), packed)
}
fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn read(bytes: &[u8]) -> Result<Snapshot, String> {
    Resource::parse(bytes)
        .and_then(|r| Snapshot::read(&r))
        .map_err(|e| e.to_string())
}
fn read_error(bytes: &[u8]) -> String {
    read(bytes).expect_err("hostile snapshot")
}
fn position() -> K {
    attribute("position", "float3", 0, 12)
}

#[test]
fn unsupported_attribute_names_of_any_readable_length_pass_the_readers_own_validation() {
    for length in [1, 100, 255, 256] {
        let long = "n".repeat(length);
        let bytes = raw_snapshot(
            1,
            vec![position(), attribute(&long, &long, 0, 12)],
            vec![],
            &floats(&[0.0; 3]),
        );
        let snapshot = read(&bytes).expect("unsupported attribute is not an error");
        assert!(matches!(snapshot, Snapshot::Unsupported(_)), "{length}");
        assert!(
            snapshot.validate().is_ok(),
            "name and type of {length} bytes"
        );
    }
}

#[test]
fn unsupported_names_are_cut_at_a_character_boundary() {
    let long = "é".repeat(128);
    let bytes = raw_snapshot(
        1,
        vec![position(), attribute(&long, &long, 0, 12)],
        vec![],
        &floats(&[0.0; 3]),
    );
    let Snapshot::Unsupported(name) = read(&bytes).expect("unsupported") else {
        panic!("not unsupported")
    };
    assert!(name.len() <= heatel::particle::MAX_UNSUPPORTED_NAME);
    assert!(name.starts_with("attribute é"));
}

#[test]
fn counts_outside_the_limit_are_errors_with_the_count_named() {
    for count in [i64::MAX, -1, 5001, i64::from(u32::MAX), 1 << 40] {
        let bytes = raw_snapshot(count, vec![position()], vec![], &floats(&[0.0; 3]));
        assert!(
            read_error(&bytes).contains("invalid particle count"),
            "{count}"
        );
    }
}

#[test]
fn attribute_ranges_outside_the_stream_or_overflowing_are_errors() {
    for (offset, size, expected) in [
        (0, 13, "attribute outside SNAP"),
        (1, 12, "attribute outside SNAP"),
        (1 << 20, 1 << 20, "attribute outside SNAP"),
        (i64::MAX, i64::MAX, "invalid attribute offset"),
        (-1, 12, "invalid attribute offset"),
        (0, -12, "invalid attribute size"),
        (0, 0, "float3 stream size mismatch"),
    ] {
        let bytes = raw_snapshot(
            1,
            vec![attribute("position", "float3", offset, size)],
            vec![],
            &floats(&[0.0; 3]),
        );
        assert!(read_error(&bytes).contains(expected), "{offset} {size}");
    }
}

#[test]
fn float3_streams_whose_size_lies_about_the_count_are_errors() {
    for size in [4, 8, 11, 13, 24] {
        let bytes = raw_snapshot(
            1,
            vec![attribute("position", "float3", 0, size)],
            vec![],
            &floats(&[0.0; 6]),
        );
        assert!(
            read_error(&bytes).contains("float3 stream size mismatch"),
            "{size}"
        );
    }
    let two = raw_snapshot(2, vec![position()], vec![], &floats(&[0.0; 6]));
    assert!(read_error(&two).contains("float3 stream size mismatch"));
}

#[test]
fn skinning_streams_whose_size_lies_about_the_count_are_errors() {
    for size in [12, 20, 23, 25, 48] {
        let bytes = raw_snapshot(
            1,
            vec![position(), attribute("skinning", "skinning", 12, size)],
            vec![s("root")],
            &floats(&[0.0; 15]),
        );
        assert!(
            read_error(&bytes).contains("skinning stream size mismatch"),
            "{size}"
        );
    }
}

#[test]
fn metadata_tables_over_their_limits_are_errors() {
    let nine: Vec<K> = (0..9).map(|_| position()).collect();
    let strings = |n: usize| (0..n).map(|_| s("root")).collect::<Vec<_>>();
    let stream = floats(&[0.0; 3]);
    let cases = [
        (
            raw_snapshot(1, nine, vec![], &stream),
            "more than eight attributes",
        ),
        (
            raw_snapshot(1, vec![position()], strings(129), &stream),
            "invalid string table",
        ),
        (
            raw_snapshot(1, vec![position()], vec![s(&"a".repeat(257))], &stream),
            "invalid string table",
        ),
        (
            raw_snapshot(1, vec![position()], vec![K::Int(3)], &stream),
            "invalid string table",
        ),
    ];
    for (bytes, expected) in cases {
        assert!(read_error(&bytes).contains(expected), "{expected}");
    }
}

#[test]
fn skinning_with_an_index_outside_the_table_or_a_bad_weight_is_an_error() {
    let attributes = || vec![position(), attribute("skinning", "skinning", 12, 24)];
    let stream = |bone: u16, weight: f32| {
        let mut stream = floats(&[0.0; 3]);
        stream.extend(bone.to_le_bytes());
        stream.extend([0u8; 6]);
        stream.extend(floats(&[weight, 0.0, 0.0, 0.0]));
        stream
    };
    for (bone, weight, expected) in [
        (5, 1.0, "skinning index outside string table"),
        (0, f32::NAN, "non-finite number"),
        (0, f32::INFINITY, "non-finite number"),
        (0, 2.0, "invalid skinning weights"),
    ] {
        let bytes = raw_snapshot(1, attributes(), vec![s("root")], &stream(bone, weight));
        assert!(read_error(&bytes).contains(expected), "{bone} {weight}");
    }
}

#[test]
fn overlapping_streams_and_empty_snapshots_are_accepted_as_authored() {
    let overlap = raw_snapshot(
        1,
        vec![position(), attribute("normal", "float3", 0, 12)],
        vec![],
        &floats(&[1.0, 2.0, 3.0]),
    );
    assert_eq!(
        read(&overlap),
        Ok(Snapshot::Points {
            positions: vec![[1.0, 2.0, 3.0]],
            normals: Some(vec![[1.0, 2.0, 3.0]])
        })
    );
    let empty = raw_snapshot(0, vec![attribute("position", "float3", 0, 0)], vec![], &[]);
    assert_eq!(
        read(&empty),
        Ok(Snapshot::Points {
            positions: vec![],
            normals: None
        })
    );
    assert!(read_error(&raw_snapshot(1, vec![], vec![], &[])).contains("missing positions"));
}

#[test]
fn packed_garbage_and_every_truncation_are_errors_not_panics() {
    let base = raw_snapshot(1, vec![position()], vec![], &floats(&[0.0; 3]));
    let metadata = Resource::parse(&base)
        .expect("resource")
        .block(b"DATA")
        .expect("DATA")
        .to_vec();
    let mut state = 0x1234_5678_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state as u8
    };
    for round in 0..4000_usize {
        let packed: Vec<u8> = (0..round % 300).map(|_| next()).collect();
        let _ = read(&resource(metadata.clone(), packed));
    }
    let declared = read_error(&resource(metadata.clone(), vec![0xff, 0xff, 0xff, 0x7f]));
    assert!(declared.contains("exceeds 1 MiB"), "{declared}");
    let big = read_error(&resource(metadata, vec![0; 3 * 1024 * 1024]));
    assert!(big.contains("packed SNAP too large"), "{big}");
    for end in 0..base.len() {
        assert!(read(&base[..end]).is_err(), "truncated at {end}");
    }
}

/// Small synthetic snapshots the particle fuzz target starts from, checked in under
/// `fuzz/seeds/particle`; `HEATEL_WRITE_SEEDS=1 cargo test` rewrites them.
fn synthetic_seeds() -> Vec<(&'static str, Vec<u8>)> {
    let three = floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    let mut skinned = floats(&[1.0, 2.0, 3.0, 0.0, 0.0, 1.0]);
    skinned.extend([0u8; 8]);
    skinned.extend(floats(&[1.0, 0.0, 0.0, 0.0]));
    let long = "n".repeat(256);
    vec![
        (
            "raw_positions",
            raw_snapshot(
                3,
                vec![attribute("position", "float3", 0, 36)],
                vec![],
                &three,
            ),
        ),
        (
            "packed_positions",
            file(2, 0, &[1.0, 2.0, 3.0, -4.0, 5.0, 6.0], false),
        ),
        (
            "position_normal_root_skinning",
            raw_snapshot(
                1,
                vec![
                    position(),
                    attribute("normal", "float3", 12, 12),
                    attribute("skinning", "skinning", 24, 24),
                ],
                vec![s("root")],
                &skinned,
            ),
        ),
        (
            "unsupported_long_attribute",
            raw_snapshot(
                1,
                vec![position(), attribute(&long, &long, 0, 12)],
                vec![],
                &floats(&[0.0; 3]),
            ),
        ),
        (
            "empty",
            raw_snapshot(0, vec![attribute("position", "float3", 0, 0)], vec![], &[]),
        ),
    ]
}

#[test]
fn checked_in_fuzz_seeds_are_current_and_read_as_snapshots() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds/particle");
    for (name, bytes) in synthetic_seeds() {
        let path = folder.join(format!("{name}.vsnap_c"));
        if std::env::var_os("HEATEL_WRITE_SEEDS").is_some() {
            std::fs::create_dir_all(&folder).expect("seed folder");
            std::fs::write(&path, &bytes).expect("write seed");
        }
        assert_eq!(
            std::fs::read(&path).expect("checked-in seed"),
            bytes,
            "{name} is stale"
        );
        let snapshot = read(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(snapshot.validate().is_ok(), "{name}");
    }
}
