//! A map's world: its entity lumps, what they place and how the map is lit, its navigation grid,
//! and the world's geometry exported as glTF.

mod crop;
mod entities;
mod environment;
mod export;
mod geometry;
mod gridnav;
mod light;
mod place;
mod placements;
mod scene;
mod surface;

pub use crate::material::{GroundLayerLook, WaterLook};
pub use crop::Band;
pub use entities::EntityLump;
pub use environment::{
    ColorGradingRef, DistanceFog, FogBlocker, FogOfWar, FogRevealer, GlobalLight, LightAngles,
    WorldEnvironment, ZoneEnvironment,
};
pub use export::{ExportedWorld, SkippedModel, WorldOptions, export_world};
pub use geometry::{
    PlacementSource, WorldDescriptors, WorldGeometry, WorldMaterialOverride, WorldNodeRef,
    WorldParticle, WorldPropPlacement,
};
pub use gridnav::GridNav;
pub use light::{
    Light, LightZone, Lighting, MAX_LIGHT_ZONES, Rgb, Shade, ZoneShade, lighting, lighting_in,
};
pub use placements::{Placement, placements};
pub use scene::{MAX_ENTITY_LUMPS, active_entity_lumps, active_entity_lumps_in};
pub use surface::{
    TerrainLayer, TerrainMaterial, TerrainMeshRef, WaterMaterial, WaterSurfaceRef, WorldDrawRef,
};

#[cfg(all(test, any(unix, windows)))]
mod audit;
