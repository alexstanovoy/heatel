//! The main Dota map's navigation grid and ground collision, read from a local install.

use heatel::kv3::Value;
use heatel::physics::{CornerStore, Physics};
use heatel::world::GridNav;
use heatel::{Budget, DirSource, Resource, Vpk, find_game_dir};

fn map_package() -> (DirSource, Vpk) {
    let dota = std::env::var_os("HEATEL_DOTA_PATH").map(std::path::PathBuf::from);
    let game = find_game_dir(dota.as_deref()).expect("Dota 2 not found; set HEATEL_DOTA_PATH");
    let source = DirSource::new(game.join("maps"));
    let package = Vpk::open(&source, "dota.vpk").unwrap();
    (source, package)
}

#[test]
#[ignore = "needs a local Dota 2 install"]
fn the_maps_grid_covers_the_playable_square_in_64_unit_cells() {
    let (source, package) = map_package();
    let nav = GridNav::read(&package.read_file(&source, "maps/dota.gnv").unwrap()).unwrap();
    // The values the main map's grid of the pinned packages holds: 64-unit cells (Dota's
    // `@gridnav(64, ...)`), centred, over a rectangle reaching past the playable square of +-9216
    // on every side.
    assert_eq!(nav.edge, 64.0);
    assert_eq!(nav.center, [32.0, 32.0]);
    assert_eq!((nav.width, nav.height), (320, 328));
    assert_eq!(nav.min, [-160, -168]);
    // The river at the map's centre and the ground where Radiant heroes appear are walkable; the
    // map's corner beyond its edge is not.
    assert!(nav.walkable(0, 0));
    assert!(nav.walkable(
        (-7176f32 / 64.0).floor() as i32,
        (-6658f32 / 64.0).floor() as i32
    ));
    assert!(!nav.walkable(nav.min[0], nav.min[1]));
}

#[test]
#[ignore = "needs a local Dota 2 install"]
fn the_maps_ground_and_water_are_collision_meshes() {
    let (source, package) = map_package();
    let bytes = package
        .read_file(&source, "maps/dota/world_physics.vmdl_c")
        .unwrap();
    let physics = Physics::read(&Resource::parse(&bytes).unwrap(), &mut Budget::default()).unwrap();
    let ground: Vec<_> = physics
        .meshes
        .iter()
        .filter(|m| physics.attributes[m.attributes].group == "default")
        .collect();
    assert_eq!(ground.len(), 1);
    assert!(ground[0].triangles.len() > 100_000);
    assert!(physics.meshes.iter().any(|m| {
        physics.attributes[m.attributes]
            .interacts_as
            .iter()
            .any(|a| a == "water")
    }));
}

/// Every collision block of the pinned packages reads: the main package's models and every map's
/// own models and worlds, the shipped addons' maps included, none refused; the shapes it read are
/// the shapes the documents declare, counted apart from the reader, so that a reader which silently
/// reads less fails on any install.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn every_collision_block_of_the_install_reads() {
    let dota = std::env::var_os("HEATEL_DOTA_PATH").map(std::path::PathBuf::from);
    let game = find_game_dir(dota.as_deref()).expect("Dota 2 not found; set HEATEL_DOTA_PATH");
    let mut packages = vec![(DirSource::new(&game), "pak01_dir.vpk".to_owned())];
    // The game's maps, and the maps of the addons it ships beside it (the hero demo's).
    let mut folders = vec![game.join("maps")];
    if let Ok(addons) = std::fs::read_dir(game.with_file_name("dota_addons")) {
        let mut addons: Vec<_> = addons
            .filter_map(|e| Some(e.ok()?.path().join("maps")))
            .collect();
        addons.sort();
        folders.extend(addons.into_iter().filter(|maps| maps.is_dir()));
    }
    for maps in folders {
        let mut names: Vec<String> = std::fs::read_dir(&maps)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|name| name.ends_with(".vpk") && !name.ends_with("_dir.vpk"))
            .collect();
        names.sort();
        for name in names {
            packages.push((DirSource::new(&maps), name));
        }
    }
    let mut counts = Counts::default();
    let mut refused = Vec::new();
    for (source, name) in &packages {
        let package = Vpk::open(source, name).unwrap();
        let models: Vec<_> = package.entries().filter(|e| e.ext() == "vmdl_c").collect();
        for entry in models {
            let bytes = package.read(source, entry).unwrap();
            let resource = match Resource::parse(&bytes) {
                Ok(resource) => resource,
                Err(e) => {
                    refused.push(format!("{name}: {}: {e}", entry.path()));
                    continue;
                }
            };
            if resource.block(b"PHYS").is_none() {
                counts.without_collision += 1;
                continue;
            }
            match Physics::read(&resource, &mut Budget::default()) {
                Ok(physics) => {
                    let document = resource.kv3(b"PHYS").unwrap().expect("a PHYS block");
                    let actual = Shapes::read(&physics);
                    let declared = Shapes::declared(&document.root);
                    assert_eq!(actual, declared, "{name}: {}", entry.path());
                    counts.blocks += 1;
                    counts.shapes.meshes += actual.meshes;
                    counts.shapes.hulls += actual.hulls;
                    counts.shapes.older_hulls += actual.older_hulls;
                    counts.shapes.vector_hulls += actual.vector_hulls;
                    counts.shapes.spheres_and_capsules += actual.spheres_and_capsules;
                }
                Err(e) => refused.push(format!("{name}: {}: {e}", entry.path())),
            }
        }
    }
    println!("{} packages: {counts:?}", packages.len());
    assert!(
        refused.is_empty(),
        "{} refused:\n{}",
        refused.len(),
        refused.join("\n")
    );
    // What the reader read against what the install's own documents declare, counted without
    // the reader: a reader that silently reads less fails here, whatever the install holds.
    assert!(packages.len() > 1, "the main package and the map packages");
    assert!(counts.blocks > 0);
}

#[derive(Debug, Default)]
struct Counts {
    blocks: usize,
    without_collision: usize,
    shapes: Shapes,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Shapes {
    meshes: usize,
    hulls: usize,
    older_hulls: usize,
    vector_hulls: usize,
    spheres_and_capsules: usize,
}

impl Shapes {
    fn declared(root: &Value) -> Self {
        let mut shapes = Self::default();
        for part in root
            .get("m_parts")
            .and_then(Value::as_array)
            .expect("parts")
        {
            let shape = part.get("m_rnShape").expect("part shape");
            let list = |key| shape.get(key).map(|value| value.as_array().expect(key));
            shapes.meshes += list("m_meshes").expect("meshes").len();
            for hull in list("m_hulls").expect("hulls") {
                shapes.hulls += 1;
                let body = hull.get("m_Hull").expect("hull body");
                if body.get("m_VertexPositions").is_some() {
                    continue;
                }
                let vertices = body.get("m_Vertices").expect("legacy hull corners");
                if vertices
                    .as_array()
                    .and_then(|items| items.first())
                    .and_then(Value::as_array)
                    .is_some()
                {
                    shapes.vector_hulls += 1;
                } else {
                    shapes.older_hulls += 1;
                }
            }
            for key in ["m_spheres", "m_capsules"] {
                shapes.spheres_and_capsules += list(key).map_or(0, <[_]>::len);
            }
        }
        shapes
    }

    fn read(physics: &Physics) -> Self {
        let mut shapes = Self {
            meshes: physics.meshes.len(),
            hulls: physics.hulls.len(),
            spheres_and_capsules: physics.unread,
            ..Self::default()
        };
        for hull in &physics.hulls {
            match hull.stored {
                CornerStore::Positions => {}
                CornerStore::Vertices => shapes.older_hulls += 1,
                CornerStore::VertexVectors => shapes.vector_hulls += 1,
                _ => panic!("a corner store this test does not know"),
            }
        }
        shapes
    }
}

#[test]
fn fresnel_lut_round_trips_synthetic_256x16_texture() {
    let expected: [[u8; 3]; 17] = [
        [139, 207, 255],
        [222, 195, 239],
        [218, 175, 218],
        [148, 155, 194],
        [75, 135, 169],
        [25, 118, 143],
        [4, 100, 114],
        [2, 82, 89],
        [0, 67, 69],
        [0, 53, 52],
        [0, 38, 39],
        [0, 27, 31],
        [0, 14, 24],
        [0, 7, 19],
        [0, 4, 16],
        [0, 2, 14],
        [0, 0, 12],
    ];
    let (w, h) = (256usize, 16usize);
    let mut rgba = vec![0u8; w * h * 4];
    let row = h / 2;
    for (i, rgb) in expected.iter().enumerate() {
        let x = if i == 16 { w - 1 } else { i * w / 16 };
        let offset = (row * w + x) * 4;
        rgba[offset..offset + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    let texture = heatel::Texture::from_rgba8(w as u32, h as u32, rgba).unwrap();
    assert_eq!(
        heatel::material::sample_fresnel_lut(&texture).unwrap(),
        expected
    );
}

#[test]
#[ignore = "requires build-6997 Dota VPKs"]
fn real_maps_export_lighting_zones_water_ground_and_fresnel_lut() {
    let raw = std::env::var_os("HEATEL_DOTA_PATH").expect("set HEATEL_DOTA_PATH");
    let game = find_game_dir(Some(&std::path::PathBuf::from(raw))).expect("Dota 2 not found");
    check_map_lighting(&game);
    check_map_materials(&game);
    check_fresnel_lut(&game);
}

fn check_map_lighting(game: &std::path::Path) {
    // Main map: 4 zones, RF sun (216,208,207)x5, Dire sky (73,94,200), fog end 4500.
    let maps_src = DirSource::new(game.join("maps"));
    let dota_vpk = Vpk::open(&maps_src, "dota.vpk").unwrap();
    let dota_bytes = dota_vpk
        .read_file(&maps_src, "maps/dota/entities/default_ents.vents_c")
        .unwrap();
    let dota_lump =
        heatel::world::EntityLump::read(&Resource::parse(&dota_bytes).unwrap()).unwrap();
    let dota_light = heatel::world::lighting(&dota_lump.entities).unwrap();
    assert_eq!(dota_light.zones.len(), 4);
    assert_eq!(dota_light.sun_dir, [66.0, 0.0]);
    assert_eq!(dota_light.spec_dir, [64.0, 283.0]);
    assert_eq!(dota_light.fog_exponent, 2.0);
    assert_eq!(dota_light.turn_secs, 5.0);
    assert_eq!(dota_light.turn, 5.0);

    let rf = &dota_light.zones[0];
    assert_eq!(rf.center, [-6784.0, -6304.0]);
    assert_eq!((rf.inner, rf.outer), (7000.0, 13000.0));
    assert_eq!(rf.day.sun, [216.0, 208.0, 207.0]);
    assert_eq!(rf.day.sun_scale, 5.0);
    assert_eq!(rf.day.fog_end, 4500.0);

    let dire = &dota_light.zones[2];
    assert_eq!(dire.day.sky, [73.0, 94.0, 200.0]);
    assert_eq!(dire.day.fog_end, 4500.0);

    // Hero demo map: 2 zones (RF and DF).
    let demo_dir = game.with_file_name("dota_addons").join("hero_demo/maps");
    let demo_src = DirSource::new(&demo_dir);
    let demo_vpk = Vpk::open(&demo_src, "hero_demo_main.vpk").unwrap();
    let demo_bytes = demo_vpk
        .read_file(
            &demo_src,
            "maps/hero_demo_main/entities/default_ents.vents_c",
        )
        .unwrap();
    let demo_lump =
        heatel::world::EntityLump::read(&Resource::parse(&demo_bytes).unwrap()).unwrap();
    let demo_light = heatel::world::lighting(&demo_lump.entities).unwrap();
    assert_eq!(demo_light.zones.len(), 2);
    assert_eq!(demo_light.zones[0].center, [-2904.0, -2320.0]);
    assert_eq!(demo_light.zones[1].center, [1920.0, 1216.0]);
}

fn check_map_materials(game: &std::path::Path) {
    // Real water, ground blend and hero material look from pak01.
    let pak_src = DirSource::new(game);
    let pak = Vpk::open(&pak_src, "pak01_dir.vpk").unwrap();

    let load_mat = |path: &str| {
        let bytes = pak.read_file(&pak_src, &format!("{path}_c")).unwrap();
        let data = Resource::parse(&bytes).unwrap().data().unwrap().unwrap();
        heatel::material::Material::from_vmat(path, &data).unwrap()
    };

    let water = load_mat("materials/water/water_generic_000.vmat")
        .water_look()
        .unwrap()
        .expect("water look");
    assert_eq!(water.normal_uv_scale, 500.0);
    assert_eq!(water.depth, 35.0);
    assert!((water.refraction_tint[0] - 0.34902).abs() < 1e-4);
    assert!((water.fallback_opacity - 0.875).abs() < 1e-4);

    let radiant = load_mat("materials/blends/mod_radiant_000.vmat")
        .ground_layers()
        .unwrap()
        .expect("ground layers");
    assert_eq!(radiant[0].scale, 5.0);
    assert_eq!(radiant[1].scale, 3.0);
    assert_eq!(radiant[1].rotation, 7.0);
    assert_eq!(radiant[2].rotation, 8.0);
    assert_eq!(radiant[3].scale, 6.0);

    let hero = load_mat("materials/models/heroes/pudge/pudge2_apron.vmat")
        .figure_look()
        .unwrap()
        .expect("figure look");
    assert_eq!(hero.rim_scale, 10.0);
    assert_eq!(hero.spec_exp, 30.0);
    assert_eq!(hero.spec_scale, 10.0);
    assert_eq!(hero.blend_full, [0.0; 6]);
}

fn check_fresnel_lut(game: &std::path::Path) {
    let pak_src = DirSource::new(game);
    let pak = Vpk::open(&pak_src, "pak01_dir.vpk").unwrap();
    let lut_bytes = pak
        .read_file(
            &pak_src,
            "materials/default/default_fresnelwarprim_tga_d9279d65.vtex_c",
        )
        .unwrap();
    let lut_tex = heatel::Texture::parse(&lut_bytes).unwrap();
    let lut = heatel::material::sample_fresnel_lut(&lut_tex).unwrap();
    let expected_r = [139, 222, 218, 148, 75, 25, 4, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let expected_g = [
        207, 195, 175, 155, 135, 118, 100, 82, 67, 53, 38, 27, 14, 7, 4, 2, 0,
    ];
    let expected_b = [
        255, 239, 218, 194, 169, 143, 114, 89, 69, 52, 39, 31, 24, 19, 16, 14, 12,
    ];
    for i in 0..17 {
        assert_eq!(
            lut[i],
            [expected_r[i], expected_g[i], expected_b[i]],
            "point {i}"
        );
    }
}
