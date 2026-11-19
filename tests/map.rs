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
