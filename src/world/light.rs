//! A map's lighting by day and by night, from its `ent_dota_lightinfo` entities.

use crate::error::{Result, malformed};
use crate::kv3::Value;

/// The weather whose lights the map starts with.
const WEATHER: &str = "[PR#]default_";

/// A light's colour (0 to 255 a channel) and Dota's scale of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shade {
    /// Red, green, blue, 0 to 255.
    pub color: [f64; 3],
    /// How bright Dota draws it.
    pub scale: f64,
}

/// A light by day and by night.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// By day.
    pub day: Shade,
    /// By night.
    pub night: Shade,
}

/// How a map is lit at its centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    /// The sun (`env_global_light`'s light, as the light infos set it).
    pub sun: Light,
    /// The ambient light.
    pub ambient: Light,
    /// The seconds the fog controller takes to turn day into night.
    pub turn: f64,
}

/// The sun's and the ambient colour and scale by day and by night, as the map's default lights
/// (its `ent_dota_lightinfo` entities of the default weather) blend at its centre (each fades
/// out between its `inner_radius` and `outer_radius`), and the seconds the fog controller takes
/// to turn. An error when no default light reaches the centre, a light lacks a value, or there
/// is no fog controller.
pub fn lighting(entities: &[Value]) -> Result<Lighting> {
    let lights: Vec<(&Value, f64)> = entities
        .iter()
        .filter(|e| class(e) == "ent_dota_lightinfo")
        .filter(|e| {
            e.get("targetname")
                .and_then(Value::as_str)
                .is_some_and(|n| n.starts_with(WEATHER))
        })
        .filter_map(|e| Some((e, weight(e)?)))
        .collect();
    let total: f64 = lights.iter().map(|l| l.1).sum();
    if lights.is_empty() || total <= 0.0 {
        return Err(malformed("lighting", "no default light reaches the centre"));
    }
    let blend = |key: &str, n: usize| -> Result<Vec<f64>> {
        let mut sum = vec![0.0; n];
        for (light, w) in &lights {
            let value = numbers(light, key).filter(|v| v.len() >= n);
            let value = value.ok_or_else(|| malformed("lighting", key))?;
            for (s, v) in sum.iter_mut().zip(&value) {
                *s += v * w / total;
            }
        }
        Ok(sum)
    };
    let shade = |color: &str, scale: &str, time: &str| -> Result<Shade> {
        let c = blend(&format!("{color}_{time}"), 3)?;
        Ok(Shade {
            color: [c[0], c[1], c[2]],
            scale: blend(&format!("{scale}_{time}"), 1)?[0],
        })
    };
    let light = |color: &str, scale: &str| -> Result<Light> {
        Ok(Light {
            day: shade(color, scale, "day")?,
            night: shade(color, scale, "night")?,
        })
    };
    let turn = entities
        .iter()
        .find(|e| class(e) == "env_fog_controller")
        .and_then(|e| e.get("foglerptime")?.as_f64())
        .ok_or_else(|| malformed("lighting", "no fog controller"))?;
    let sun = light("color", "light_scale")?;
    let ambient = light("ambient_color", "ambient_scale")?;
    Ok(Lighting { sun, ambient, turn })
}

/// How much `light` counts at the map's centre: 1 within its inner radius, falling to 0 at the
/// outer one.
fn weight(light: &Value) -> Option<f64> {
    let at = numbers(light, "origin")?;
    let distance = at.first()?.hypot(*at.get(1)?);
    let inner = light.get("inner_radius")?.as_f64()?;
    let outer = light.get("outer_radius")?.as_f64()?;
    Some(((outer - distance) / (outer - inner).max(f64::EPSILON)).clamp(0.0, 1.0))
}

/// The numbers of the array (or the one number) `key`.
fn numbers(entity: &Value, key: &str) -> Option<Vec<f64>> {
    let value = entity.get(key)?;
    match value.as_array() {
        Some(list) => list.iter().map(Value::as_f64).collect(),
        None => Some(vec![value.as_f64()?]),
    }
}

fn class(entity: &Value) -> &str {
    entity
        .get("classname")
        .and_then(Value::as_str)
        .unwrap_or("")
}
