use super::{PlacementSource, WorldDescriptors};
use crate::{DirSource, Vpk, find_game_dir};

#[test]
#[ignore = "requires build-6997 Dota VPKs (HEATEL_DOTA_PATH)"]
fn build_6997_world_descriptors_resolve_active_geometry_and_environment() {
    let root = std::env::var_os("HEATEL_DOTA_PATH").map(std::path::PathBuf::from);
    let source = DirSource::new(find_game_dir(root.as_deref()).expect("set HEATEL_DOTA_PATH"));
    let map = Vpk::open(&source, "maps/dota.vpk").unwrap();
    let game = Vpk::open(&source, "pak01_dir.vpk").unwrap();
    let world = WorldDescriptors::read(&source, &[&map, &game], "maps/dota/world.vwrld_c").unwrap();
    let environment = &world.environment;
    let geometry = &world.geometry;
    let mut placements = [0; 3];
    for placement in &geometry.placements {
        assert!(
            !placement
                .layer
                .as_deref()
                .is_some_and(|layer| layer.ends_with("_destruction"))
        );
        assert!(
            placement
                .gltf_matrix()
                .iter()
                .all(|component| component.is_finite())
        );
        let index = match placement.source {
            PlacementSource::SceneObject { .. } => 0,
            PlacementSource::Aggregate { .. } => 1,
            PlacementSource::Entity { .. } => 2,
        };
        placements[index] += 1;
    }
    eprintln!(
        "nodes={}, placements={placements:?}, terrain={} ({} materials), water={} ({} materials), particles={}, missing={:?}",
        geometry.nodes.len(),
        geometry.terrain.len(),
        geometry.terrain_materials.len(),
        geometry.water.len(),
        geometry.water_materials.len(),
        world.particles.len(),
        geometry.missing_resources
    );
    eprintln!(
        "lighting={:?}, zone maps={:?}, grading={:?}",
        environment.global_light,
        environment.zones.first(),
        environment.color_grading
    );
    assert_eq!(geometry.nodes.len(), 1);
    assert_eq!(placements, [2026, 11925, 3066]);
    assert_eq!(geometry.terrain.len(), 540);
    assert_eq!(geometry.terrain_materials.len(), 13);
    assert_eq!(geometry.water_materials.len(), 2);
    assert!(geometry.missing_resources.is_empty());
    assert_eq!(geometry.water.len(), 89);
    assert_eq!(environment.fog_of_war.blockers.len(), 393);
    assert!(
        environment
            .zones
            .iter()
            .any(|zone| zone.water_flow_map.as_deref() == Some("maps/dota/water_flow_map.vtex"))
    );
    assert_eq!(environment.fog_of_war.revealers.len(), 19);
    assert_eq!(environment.fog.unwrap().start, Some(1250.0));
    assert_eq!(environment.fog.unwrap().end, Some(4500.0));
    assert!(environment.color_grading.is_empty());
    assert_eq!(world.particles.len(), 972);
}
