use std::collections::BTreeMap;

use super::{EmbeddedImage, Rgba8Options, TextureMeta, VTex, VTexFormat, embedded_image};
use crate::vpk::Entry;
use crate::{DirSource, Error, Result, Vpk, find_game_dir};

#[derive(Debug, Default)]
struct Audit {
    formats: BTreeMap<VTexFormat, usize>,
    decoded: usize,
    jpeg: usize,
    sheets: usize,
    cubes: usize,
    volumes: usize,
    images: usize,
    texels: u64,
}

#[test]
#[ignore = "requires build-6997 Dota VPKs (HEATEL_DOTA_PATH)"]
fn build_6997_texture_corpus_decodes_all_supported_images() {
    let root = std::env::var_os("HEATEL_DOTA_PATH").map(std::path::PathBuf::from);
    let source = DirSource::new(find_game_dir(root.as_deref()).expect("set HEATEL_DOTA_PATH"));
    let mut failures = 0;
    for (name, expected, expected_jpeg) in
        [("pak01_dir.vpk", 130_316, 181), ("maps/dota.vpk", 3, 0)]
    {
        let package = Vpk::open(&source, name).unwrap();
        let mut audit = Audit::default();
        for entry in package.entries().filter(|e| e.ext() == "vtex_c") {
            if let Err(error) = audit_entry(&source, &package, entry, &mut audit) {
                failures += 1;
                if failures <= 30 {
                    eprintln!("{}: {error}", entry.path());
                }
            }
        }
        eprintln!("{name}: {audit:#?}");
        assert_eq!(audit.formats.values().sum::<usize>(), expected);
        assert_eq!(audit.jpeg, expected_jpeg);
    }
    assert_eq!(failures, 0, "{failures} unexpected decode failures");
}

fn audit_entry(
    source: &DirSource,
    package: &Vpk,
    entry: Entry<'_>,
    audit: &mut Audit,
) -> Result<()> {
    let bytes = package.read(source, entry)?;
    let meta = TextureMeta::read(&bytes)?;
    *audit.formats.entry(meta.format).or_default() += 1;
    audit.sheets += usize::from(meta.sheet.is_some());
    audit.cubes += usize::from(meta.flags.is_cube());
    audit.volumes += usize::from(meta.flags.is_volume());
    let texture = match VTex::parse(&bytes) {
        Ok(texture) => texture,
        Err(Error::Malformed {
            what: "vtex",
            detail,
        }) if matches!(meta.format, VTexFormat::JpegDxt5 | VTexFormat::JpegRgba8888)
            && detail.starts_with("unsupported format id") =>
        {
            assert!(matches!(
                embedded_image(&bytes)?,
                Some(EmbeddedImage::Jpeg(_))
            ));
            audit.jpeg += 1;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    assert_eq!(*texture.meta(), meta);
    for level in 0..meta.mip_count {
        let mip = texture.mip(level).unwrap();
        let [w, h, depth] = mip.dimensions();
        for layer in 0..depth {
            for face in 0..meta.flags.face_count() {
                let rgba = mip.decode_rgba8(layer, face, Rgba8Options::default())?;
                assert_eq!(rgba.len(), w as usize * h as usize * 4);
                audit.images += 1;
                audit.texels += u64::from(w) * u64::from(h);
            }
        }
    }
    audit.decoded += 1;
    Ok(())
}
