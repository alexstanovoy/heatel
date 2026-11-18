//! The main Dota map's navigation grid and ground collision, read from a local install.

use heatel::physics::Physics;
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
    // The values the 7.41 and 7.42 maps' grids hold: 64-unit cells (Dota's `@gridnav(64, ...)`),
    // centred, over a rectangle reaching past the playable square of +-9216 on every side.
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
