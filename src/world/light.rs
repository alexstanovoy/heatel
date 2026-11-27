//! A map's regional lighting zones by day and by night, plus its sun/specular directions and
//! fog controller settings.

use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::kv3::Value;

/// Maximum number of lighting zones accepted from a single map.
pub const MAX_LIGHT_ZONES: usize = 16;

/// Red, green, blue colour channels (0 to 255 per channel).
pub type Rgb = [f64; 3];

/// The default weather prefix used to select the map's base lighting zones.
const WEATHER: &str = "[PR#]default_";

/// A light's colour (0 to 255 a channel) and its intensity scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shade {
    /// Red, green, blue, 0 to 255.
    pub color: Rgb,
    /// Intensity multiplier.
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

/// All shading, fog, and visibility-veil parameters for one time of day inside a [`LightZone`].
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct ZoneShade {
    /// Direct sun RGB colour (0 to 255).
    pub sun: Rgb,
    /// Direct sun intensity scale.
    pub sun_scale: f64,
    /// Hemisphere sky ambient RGB colour (0 to 255).
    pub sky: Rgb,
    /// Hemisphere sky ambient intensity scale.
    pub sky_scale: f64,
    /// Secondary shade-lobe RGB colour (0 to 255).
    pub shade: Rgb,
    /// Secondary shade-lobe intensity scale.
    pub shade_scale: f64,
    /// Hemisphere ground-side ambient scale.
    pub ground_scale: f64,
    /// Specular highlight RGB colour (0 to 255).
    pub specular: Rgb,
    /// Distance fog RGB colour (0 to 255).
    pub fog: Rgb,
    /// Distance fog start distance in world units.
    pub fog_start: f64,
    /// Distance fog end distance in world units.
    pub fog_end: f64,
    /// Visibility-veil darkness exponent.
    pub veil_darkness: f64,
    /// Visibility-veil tint weights `[r, g, b]`.
    pub veil_cast: [f64; 3],
    /// Skybox tint RGB colour (0 to 255).
    pub sky_tint: Rgb,
}

/// A radial regional lighting volume on the map.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct LightZone {
    /// 2D world-space centre `[x, y]` of the volume.
    pub center: [f64; 2],
    /// Radius within which the zone has full weight (`>= 0.0`).
    pub inner: f64,
    /// Radius beyond which the zone has zero weight (`> inner`).
    pub outer: f64,
    /// Daytime shading parameters.
    pub day: ZoneShade,
    /// Nighttime shading parameters.
    pub night: ZoneShade,
}

impl LightZone {
    /// Unnormalized blend weight in `[0.0, 1.0]` at 2D world position `focus`: `1.0` inside
    /// `inner`, falling linearly to `0.0` at `outer`.
    pub fn weight_at(&self, focus: [f64; 2]) -> f64 {
        let distance = (focus[0] - self.center[0]).hypot(focus[1] - self.center[1]);
        let span = self.outer - self.inner;
        if !distance.is_finite() || !(span > 0.0 && span.is_finite()) {
            return 0.0;
        }
        ((self.outer - distance) / span).clamp(0.0, 1.0)
    }
}

/// A map's regional lighting zones, global light directions, and fog transition settings.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Lighting {
    /// Regional lighting zones (at most [`MAX_LIGHT_ZONES`]).
    pub zones: Vec<LightZone>,
    /// Sun direction as `[pitch, yaw]` in degrees.
    pub sun_dir: [f64; 2],
    /// Specular light direction as `[pitch, yaw]` in degrees.
    pub spec_dir: [f64; 2],
    /// Distance fog curve exponent.
    pub fog_exponent: f64,
    /// Seconds taken to transition between day and night.
    pub turn_secs: f64,
    /// Map-centre blended sun light (compatibility summary).
    pub sun: Light,
    /// Map-centre blended sky ambient light (compatibility summary).
    pub ambient: Light,
    /// Seconds taken to transition between day and night (compatibility alias for `turn_secs`).
    pub turn: f64,
}

/// Reads a map's default-weather lighting zones, global light directions, and fog transition
/// settings from its entity list.
///
/// # Errors
///
/// Returns an error if there are no lighting zones, more than [`MAX_LIGHT_ZONES`] zones, any
/// required key is missing or non-finite, any zone has invalid radii, or the fog controller is
/// missing or malformed.
pub fn lighting(entities: &[Value]) -> Result<Lighting> {
    lighting_in(entities, &mut Budget::named("lighting"))
}

/// Like [`lighting`], charging allocated zone storage against `budget`.
pub fn lighting_in(entities: &[Value], budget: &mut Budget) -> Result<Lighting> {
    read_lighting(entities.iter(), budget)
}

pub(super) fn default_zone(entity: &Value) -> bool {
    class(entity) == "ent_dota_lightinfo"
        && entity
            .str_field("targetname")
            .is_some_and(|name| name.starts_with(WEATHER))
}

pub(super) fn read_lighting<'a>(
    mut entities: impl Iterator<Item = &'a Value> + Clone,
    budget: &mut Budget,
) -> Result<Lighting> {
    let raw_zones: Vec<_> = entities
        .clone()
        .filter(|entity| default_zone(entity))
        .take(MAX_LIGHT_ZONES + 1)
        .collect();
    if raw_zones.is_empty() {
        return Err(malformed("lighting", "no light zones"));
    }
    if raw_zones.len() > MAX_LIGHT_ZONES {
        return Err(malformed(
            "lighting",
            format!("more than {MAX_LIGHT_ZONES} light zones"),
        ));
    }
    budget.slots::<LightZone>(raw_zones.len())?;

    let mut zones = Vec::with_capacity(raw_zones.len());
    for (index, entity) in raw_zones.iter().enumerate() {
        zones.push(read_zone(entity, index)?);
    }

    let global_light = entities.clone().find(|e| class(e) == "env_global_light");
    let sun_dir = vec2(raw_zones[0], "light_direction_day")
        .or_else(|| global_light.and_then(|e| vec2(e, "angles")))
        .ok_or_else(|| malformed("lighting", "light zone 0: sun direction"))?;
    let spec_dir = vec2(raw_zones[0], "specular_direction_day")
        .or_else(|| global_light.and_then(|e| vec2(e, "specularangles")))
        .ok_or_else(|| malformed("lighting", "light zone 0: specular direction"))?;

    let fog_ctrl = entities
        .find(|e| class(e) == "env_fog_controller")
        .ok_or_else(|| malformed("lighting", "missing fog controller"))?;
    let turn_secs = fog_ctrl
        .get("foglerptime")
        .and_then(Value::finite_f64)
        .filter(|&v| v >= 0.0)
        .ok_or_else(|| malformed("lighting", "fog controller: transition time"))?;
    let fog_exponent = fog_ctrl
        .get("fogexponent")
        .and_then(Value::finite_f64)
        .filter(|&v| v > 0.0)
        .ok_or_else(|| malformed("lighting", "fog controller: fog exponent"))?;

    let (sun, ambient) = blend_center(&zones)?;

    Ok(Lighting {
        zones,
        sun_dir,
        spec_dir,
        fog_exponent,
        turn_secs,
        sun,
        ambient,
        turn: turn_secs,
    })
}

fn read_zone(entity: &Value, index: usize) -> Result<LightZone> {
    let center = vec2(entity, "origin")
        .ok_or_else(|| malformed("lighting", format!("light zone {index}: origin")))?;
    let inner = scalar(entity, "inner_radius")
        .ok_or_else(|| malformed("lighting", format!("light zone {index}: inner radius")))?;
    let outer = scalar(entity, "outer_radius")
        .ok_or_else(|| malformed("lighting", format!("light zone {index}: outer radius")))?;
    if !(inner >= 0.0 && outer > inner) {
        return Err(malformed("lighting", format!("light zone {index}: radius")));
    }
    let day = read_zone_shade(entity, index, "day")?;
    let night = read_zone_shade(entity, index, "night")?;
    Ok(LightZone {
        center,
        inner,
        outer,
        day,
        night,
    })
}

fn read_zone_shade(entity: &Value, index: usize, time: &str) -> Result<ZoneShade> {
    let err = |label: &str| malformed("lighting", format!("light zone {index}: {time} {label}"));
    let color = |prefix: &str, label: &str| {
        vec3(entity, &format!("{prefix}_{time}")).ok_or_else(|| err(label))
    };
    let num = |prefix: &str, label: &str| {
        scalar(entity, &format!("{prefix}_{time}")).ok_or_else(|| err(label))
    };

    let sun = color("color", "sun colour")?;
    let sun_scale = num("light_scale", "sun scale")?;
    let sky = color("ambient_color", "sky colour")?;
    let sky_scale = num("ambient_scale", "sky scale")?;
    let shade = color("shadow_color", "shade colour")?;
    let shade_scale = num("shadow_scale", "shade scale")?;
    let ground_scale = num("shadow_ground_scale", "ground scale")?;
    let specular = color("specular_color", "specular colour")?;
    let fog = color("fog_color", "fog colour")?;
    let fog_start = num("fog_start", "fog start")?;
    let fog_end = num("fog_end", "fog end")?;
    let veil_darkness = scalar(entity, &format!("fow_darkess_{time}"))
        .or_else(|| scalar(entity, &format!("fow_darkness_{time}")))
        .ok_or_else(|| err("veil darkness"))?;
    let veil_cast = [
        num("fow_color_r", "veil cast")?,
        num("fow_color_g", "veil cast")?,
        num("fow_color_b", "veil cast")?,
    ];
    let sky_tint = color("skybox_tint_color", "sky tint")?;

    Ok(ZoneShade {
        sun,
        sun_scale,
        sky,
        sky_scale,
        shade,
        shade_scale,
        ground_scale,
        specular,
        fog,
        fog_start,
        fog_end,
        veil_darkness,
        veil_cast,
        sky_tint,
    })
}

fn blend_center(zones: &[LightZone]) -> Result<(Light, Light)> {
    let weights: Vec<f64> = zones.iter().map(|z| z.weight_at([0.0, 0.0])).collect();
    let total: f64 = weights.iter().sum();
    if !(total > 0.0 && total.is_finite()) {
        let first = &zones[0];
        return Ok((
            Light {
                day: Shade {
                    color: first.day.sun,
                    scale: first.day.sun_scale,
                },
                night: Shade {
                    color: first.night.sun,
                    scale: first.night.sun_scale,
                },
            },
            Light {
                day: Shade {
                    color: first.day.sky,
                    scale: first.day.sky_scale,
                },
                night: Shade {
                    color: first.night.sky,
                    scale: first.night.sky_scale,
                },
            },
        ));
    }

    let blend_rgb = |pick: fn(&LightZone) -> Rgb| -> Result<Rgb> {
        let mut out = [0.0f64; 3];
        for (zone, &w) in zones.iter().zip(&weights) {
            if w == 0.0 {
                continue;
            }
            let c = pick(zone);
            for i in 0..3 {
                out[i] += c[i] * w / total;
            }
        }
        out.iter()
            .all(|v| v.is_finite())
            .then_some(out)
            .ok_or_else(|| malformed("lighting", "centre blend colour is not finite"))
    };
    let blend_scalar = |pick: fn(&LightZone) -> f64| -> Result<f64> {
        let mut out = 0.0f64;
        for (zone, &w) in zones.iter().zip(&weights) {
            if w == 0.0 {
                continue;
            }
            out += pick(zone) * w / total;
        }
        out.is_finite()
            .then_some(out)
            .ok_or_else(|| malformed("lighting", "centre blend scale is not finite"))
    };

    let sun = Light {
        day: Shade {
            color: blend_rgb(|z| z.day.sun)?,
            scale: blend_scalar(|z| z.day.sun_scale)?,
        },
        night: Shade {
            color: blend_rgb(|z| z.night.sun)?,
            scale: blend_scalar(|z| z.night.sun_scale)?,
        },
    };
    let ambient = Light {
        day: Shade {
            color: blend_rgb(|z| z.day.sky)?,
            scale: blend_scalar(|z| z.day.sky_scale)?,
        },
        night: Shade {
            color: blend_rgb(|z| z.night.sky)?,
            scale: blend_scalar(|z| z.night.sky_scale)?,
        },
    };
    Ok((sun, ambient))
}

fn scalar(entity: &Value, key: &str) -> Option<f64> {
    let value = entity.get(key)?;
    match value.as_array() {
        Some(list) => list.first()?.finite_f64(),
        None => value.finite_f64(),
    }
}

fn vec2(entity: &Value, key: &str) -> Option<[f64; 2]> {
    let list = entity.get(key)?.as_array()?;
    Some([list.first()?.finite_f64()?, list.get(1)?.finite_f64()?])
}

fn vec3(entity: &Value, key: &str) -> Option<Rgb> {
    let list = entity.get(key)?.as_array()?;
    Some([
        list.first()?.finite_f64()?,
        list.get(1)?.finite_f64()?,
        list.get(2)?.finite_f64()?,
    ])
}

fn class(entity: &Value) -> &str {
    entity
        .get("classname")
        .and_then(Value::as_str)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv3::Object;

    fn zone_with_radii(center: [f64; 2], inner: f64, outer: f64) -> LightZone {
        let shade = ZoneShade {
            sun: [200.0; 3],
            sun_scale: 5.0,
            sky: [100.0; 3],
            sky_scale: 5.0,
            shade: [150.0; 3],
            shade_scale: 1.0,
            ground_scale: 1.0,
            specular: [120.0; 3],
            fog: [180.0; 3],
            fog_start: 1250.0,
            fog_end: 4500.0,
            veil_darkness: 2.7,
            veil_cast: [1.0, 2.0, 4.0],
            sky_tint: [255.0; 3],
        };
        LightZone {
            center,
            inner,
            outer,
            day: shade,
            night: shade,
        }
    }

    #[test]
    fn zone_weight_interpolates_linearly_between_inner_and_outer() {
        let zone = zone_with_radii([100.0, 0.0], 200.0, 600.0);
        assert_eq!(zone.weight_at([100.0, 0.0]), 1.0);
        assert_eq!(zone.weight_at([300.0, 0.0]), 1.0);
        assert!((zone.weight_at([500.0, 0.0]) - 0.5).abs() < 1e-12);
        assert_eq!(zone.weight_at([700.0, 0.0]), 0.0);
        assert_eq!(zone.weight_at([900.0, 0.0]), 0.0);
        assert_eq!(zone.weight_at([f64::NAN, 0.0]), 0.0);
    }

    #[test]
    fn missing_zone_key_reports_neutral_error_with_zone_index() {
        let mut obj = Object::default();
        obj.push(
            "classname".into(),
            Value::String("ent_dota_lightinfo".into()),
        );
        obj.push("targetname".into(), Value::String("[PR#]default_0".into()));
        obj.push(
            "origin".into(),
            Value::Array(vec![Value::Float(0.0), Value::Float(0.0)]),
        );
        obj.push("inner_radius".into(), Value::Float(100.0));
        obj.push("outer_radius".into(), Value::Float(200.0));
        let err = lighting(&[Value::Object(obj)]).unwrap_err().to_string();
        assert!(err.contains("light zone 0: day sun colour"), "got: {err}");
        assert!(!err.contains("color_day"), "must not leak raw key: {err}");
    }
}
