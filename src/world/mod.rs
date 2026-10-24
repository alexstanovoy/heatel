//! A map's world: its entity lumps, what they place and how the map is lit, and the world's
//! geometry exported as glTF.

mod crop;
mod entities;
mod export;
mod light;
mod place;
mod placements;
mod scene;

pub use crop::Band;
pub use entities::EntityLump;
pub use export::{ExportedWorld, WorldOptions, export_world};
pub use light::{Light, Lighting, Shade, lighting};
pub use placements::{Placement, placements};
