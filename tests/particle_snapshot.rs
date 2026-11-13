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
