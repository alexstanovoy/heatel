//! A map's world: its entity lumps, what they place and how the map is lit, its navigation grid,
//! and the world's geometry exported as glTF.

mod crop;
mod entities;
mod export;
mod gridnav;
mod light;
mod place;
mod placements;
mod scene;

pub use crate::material::{GroundLayerLook, WaterLook};
pub use crop::Band;
pub use entities::EntityLump;
pub use export::{ExportedWorld, SkippedModel, WorldOptions, export_world};
pub use gridnav::GridNav;
pub use light::{
    Light, LightZone, Lighting, MAX_LIGHT_ZONES, Rgb, Shade, ZoneShade, lighting, lighting_in,
};
pub use placements::{Placement, placements};
pub use scene::{MAX_ENTITY_LUMPS, active_entity_lumps, active_entity_lumps_in};
