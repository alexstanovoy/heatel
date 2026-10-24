//! What a map's entities place: each entity with a model, where it stands, how it turns and
//! its scale (trees, buildings, props).

use crate::kv3::Value;

/// An entity that places a model.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The model's file stem (`models/props_tree/tree_pine01.vmdl` is `tree_pine01`).
    pub model: String,
    /// Its position in Dota units; a coordinate that is not finite reads as 0.
    pub origin: [f64; 3],
    /// Its yaw in degrees, in (-180, 180].
    pub yaw: f64,
    /// Its scale (the first of `scales`), 1 where unset.
    pub scale: f64,
}

/// The placement of every entity of `entities` (an [`EntityLump`](super::EntityLump)'s) that
/// names a model, in their order.
pub fn placements(entities: &[Value]) -> Vec<Placement> {
    entities.iter().filter_map(placed).collect()
}

fn placed(entity: &Value) -> Option<Placement> {
    let model = entity.get("model")?.as_str()?;
    let file = model.rsplit('/').next()?;
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let numbers = |key, default: f64| -> [f64; 3] {
        let list = entity
            .get(key)
            .and_then(Value::as_array)
            .unwrap_or_default();
        std::array::from_fn(|i| list.get(i).and_then(Value::as_f64).unwrap_or(default))
    };
    let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
    let yaw = numbers("angles", 0.0)[1].to_radians();
    Some(Placement {
        model: stem.to_owned(),
        origin: numbers("origin", 0.0).map(finite),
        yaw: finite(yaw.sin().atan2(yaw.cos()).to_degrees()),
        scale: finite(numbers("scales", 1.0)[0]),
    })
}
