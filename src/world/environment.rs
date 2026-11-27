use super::light::{default_zone, read_lighting};
use super::{EntityLump, Lighting, Rgb, Shade, active_entity_lumps_in};
use crate::kv3::Value;
use crate::{Budget, Result, Source, Vpk};

/// Authored lighting and atmospheric resources from the active entity lumps.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct WorldEnvironment {
    /// Dota's default-weather day/night zones, including hemisphere shade, distance fog,
    /// and fog-of-war tint and darkness.
    pub lighting: Option<Lighting>,
    /// Angles, skies and map textures for each zone, in the same order as `lighting.zones`.
    pub zones: Vec<ZoneEnvironment>,
    /// The global light controller, when present.
    pub global_light: Option<GlobalLight>,
    /// Global distance fog controller values. Dota's per-zone values are in `lighting`.
    pub fog: Option<DistanceFog>,
    /// Sky material or sky name from an `env_sky` entity or the worldspawn.
    pub sky: Option<String>,
    /// Sky used for Dota reflections, when authored.
    pub reflection_sky: Option<String>,
    /// Authored colour correction references, including weather-specific Dota colour warps.
    pub color_grading: Vec<ColorGradingRef>,
    /// Dota fog-of-war bounds and map markers.
    pub fog_of_war: FogOfWar,
}

/// A Source pitch, yaw and roll, in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightAngles(pub [f64; 3]);

impl LightAngles {
    /// Unit direction of the rays in Source's Z-up frame. Pitch increases toward the ground.
    pub fn direction(self) -> [f64; 3] {
        let [pitch, yaw, _] = self.0.map(f64::to_radians);
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        [cos_pitch * cos_yaw, cos_pitch * sin_yaw, -sin_pitch]
    }
}

/// Authored global light and hemisphere values.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct GlobalLight {
    /// Directional sun angles.
    pub sun_angles: Option<LightAngles>,
    /// Shadow angles when explicit, otherwise the global light's angles if shadows are enabled.
    pub shadow_angles: Option<LightAngles>,
    /// Whether the global light enables shadows.
    pub shadows_enabled: Option<bool>,
    /// Authored global sun colour and intensity.
    pub sun: Option<Shade>,
    /// Authored global hemisphere sky.
    pub sky: Option<Shade>,
    /// Authored global hemisphere ground/shadow lobe.
    pub ground: Option<Shade>,
    /// Authored colour of the secondary ambient lobe.
    pub secondary_ambient: Option<Rgb>,
    /// Orientation of the hemisphere.
    pub ambient_angles: Option<LightAngles>,
    /// Specular angles.
    pub specular_angles: Option<LightAngles>,
}

/// Source map lighting metadata beside one default-weather zone's shading values.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ZoneEnvironment {
    /// Day and night directions, in that order.
    pub sun_angles: [Option<LightAngles>; 2],
    /// Day and night directions, in that order.
    pub ambient_angles: [Option<LightAngles>; 2],
    /// Day and night directions, in that order.
    pub specular_angles: [Option<LightAngles>; 2],
    /// Day and night sky materials, in that order.
    pub sky_materials: [Option<String>; 2],
    /// Height-fog opacity texture.
    pub fog_opacity_map: Option<String>,
    /// Height-fog flow texture.
    pub fog_flow_map: Option<String>,
    /// World-space water flow texture.
    pub water_flow_map: Option<String>,
}

/// Primary colour and distances from an `env_fog_controller`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct DistanceFog {
    /// Fog enable flag.
    pub enabled: Option<bool>,
    /// Start distance in Source units.
    pub start: Option<f64>,
    /// End distance in Source units.
    pub end: Option<f64>,
    /// RGB colour in 0..255.
    pub color: Option<Rgb>,
}

/// An entity's reference to a colour-grading lookup table.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ColorGradingRef {
    /// Authored asset path. Legacy colour correction can refer to an uncompiled .raw table.
    pub path: String,
    /// Class of the entity that names it.
    pub classname: String,
    /// Name of the entity or weather zone, when set.
    pub targetname: Option<String>,
    /// Dota colour-warp blend or the colour correction controller's maximum weight.
    pub weight: Option<f64>,
    /// Colour correction controller's StartDisabled, when authored.
    pub start_disabled: Option<bool>,
}

/// Authored fog-of-war map markers and extents.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct FogOfWar {
    /// Worldspawn fog grid extents `[min, max]` in Source units.
    pub bounds: Option<[[f64; 3]; 2]>,
    /// Named obstruction nodes, in active entity order.
    pub blockers: Vec<FogBlocker>,
    /// Sight sources, in active entity order.
    pub revealers: Vec<FogRevealer>,
}

/// A map-authored fog obstruction node.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct FogBlocker {
    /// World-space position in Source units.
    pub origin: [f64; 3],
    /// Name of the node.
    pub name: Option<String>,
    /// Name of the next node, when set.
    pub next: Option<String>,
}

/// A map-authored source of sight.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct FogRevealer {
    /// World-space position in Source units.
    pub origin: [f64; 3],
    /// Raw Source team number.
    pub team: Option<i64>,
    /// Vision range in Source units.
    pub range: Option<f64>,
}

impl WorldEnvironment {
    /// Reads environment metadata from the world's active entity lumps.
    pub fn read(src: &(impl Source + ?Sized), packages: &[&Vpk], world: &str) -> Result<Self> {
        let mut budget = Budget::named("world environment");
        let lumps = active_entity_lumps_in(src, packages, world, &mut budget)?;
        Self::from_lumps_in(&lumps, &mut budget)
    }

    /// Reads the environment from entity lumps already selected by the caller.
    pub fn from_lumps(lumps: &[EntityLump]) -> Result<Self> {
        Self::from_lumps_in(lumps, &mut Budget::named("world environment"))
    }

    pub(super) fn from_lumps_in(lumps: &[EntityLump], budget: &mut Budget) -> Result<Self> {
        let entities = lumps.iter().flat_map(|lump| &lump.entities);
        let find = |class| {
            entities
                .clone()
                .find(|entity| entity.str_field("classname") == Some(class))
        };
        let world = find("worldspawn");
        let sky = find("env_sky")
            .and_then(|e| text(e, "skyname").or_else(|| text(e, "skyMaterial")))
            .or_else(|| world.and_then(|e| text(e, "skyname")));
        let mut result = Self {
            global_light: find("env_global_light").map(global_light),
            fog: find("env_fog_controller").map(distance_fog),
            sky: path_in(sky, budget)?,
            reflection_sky: path_in(
                find("dota_reflection_skybox").and_then(|e| text(e, "skyname")),
                budget,
            )?,
            ..Self::default()
        };
        if entities.clone().any(default_zone) {
            result.lighting = Some(read_lighting(entities.clone(), budget)?);
            for entity in entities.clone().filter(|entity| default_zone(entity)) {
                budget.slots::<ZoneEnvironment>(1)?;
                result.zones.push(zone_environment(entity, budget)?);
            }
        }
        result.fog_of_war.bounds = world.and_then(|entity| {
            Some([
                vector(entity, "_dotatilegrid_fogbounds_min")?,
                vector(entity, "_dotatilegrid_fogbounds_max")?,
            ])
        });
        for entity in entities {
            if let Some(reference) = color_grading(entity, budget)? {
                budget.slots::<ColorGradingRef>(1)?;
                result.color_grading.push(reference);
            }
            result.fog_of_war.read_marker(entity, budget)?;
        }
        Ok(result)
    }
}

impl FogOfWar {
    fn read_marker(&mut self, entity: &Value, budget: &mut Budget) -> Result<()> {
        match (entity.str_field("classname"), vector(entity, "origin")) {
            (Some("ent_fow_blocker_node"), Some(origin)) => {
                budget.slots::<FogBlocker>(1)?;
                self.blockers.push(FogBlocker {
                    origin,
                    name: copy_in(text(entity, "targetname"), budget)?,
                    next: copy_in(text(entity, "TargetNode"), budget)?,
                });
            }
            (Some("ent_fow_revealer"), Some(origin)) => {
                budget.slots::<FogRevealer>(1)?;
                self.revealers.push(FogRevealer {
                    origin,
                    team: super::scene::entity_int(entity, "teamnumber"),
                    range: scalar(entity, "visionrange"),
                });
            }
            _ => {}
        }
        Ok(())
    }
}

fn global_light(entity: &Value) -> GlobalLight {
    let shade = |color, scale| {
        Some(Shade {
            color: vector(entity, color)?,
            scale: scalar(entity, scale)?,
        })
    };
    let sun_angles = angles(entity, "angles");
    let shadows_enabled = boolean(entity, "enableshadows");
    let shadow_angles = angles(entity, "shadowangles").or(if shadows_enabled == Some(true) {
        sun_angles
    } else {
        None
    });
    GlobalLight {
        sun_angles,
        shadow_angles,
        shadows_enabled,
        sun: shade("color", "lightscale"),
        sky: shade("ambientcolor1", "ambientscale1"),
        ground: shade("ambientcolor2", "ambientscale2"),
        secondary_ambient: vector(entity, "ambientcolor3"),
        ambient_angles: angles(entity, "ambientangles"),
        specular_angles: angles(entity, "specularangles"),
    }
}

fn distance_fog(entity: &Value) -> DistanceFog {
    DistanceFog {
        enabled: boolean(entity, "fogenable"),
        start: scalar(entity, "fogstart"),
        end: scalar(entity, "fogend"),
        color: vector(entity, "fogcolor"),
    }
}

fn zone_environment(entity: &Value, budget: &mut Budget) -> Result<ZoneEnvironment> {
    let day_night = |day, night| [angles(entity, day), angles(entity, night)];
    let mut path = |key| path_in(text(entity, key), budget);
    Ok(ZoneEnvironment {
        sun_angles: day_night("light_direction_day", "light_direction_night"),
        ambient_angles: day_night("ambient_direction_day", "ambient_direction_night"),
        specular_angles: day_night("specular_direction_day", "specular_direction_night"),
        sky_materials: [path("skybox_material_day")?, path("skybox_material_night")?],
        fog_opacity_map: path("height_fog_textureOpacity")?,
        fog_flow_map: path("fog_flow_map_texture")?,
        water_flow_map: path("water_flow_map_texture")?,
    })
}

fn color_grading(entity: &Value, budget: &mut Budget) -> Result<Option<ColorGradingRef>> {
    let class = entity.str_field("classname").unwrap_or_default();
    let (path, weight) = match class {
        "color_correction" | "color_correction_volume" => (
            text(entity, "filename").or_else(|| text(entity, "lookup_table")),
            scalar(entity, "maxweight"),
        ),
        "ent_dota_lightinfo" => (
            text(entity, "color_warp_texture"),
            scalar(entity, "color_warp_blend_to_full"),
        ),
        _ => return Ok(None),
    };
    let Some(path) = path_in(path, budget)? else {
        return Ok(None);
    };
    Ok(Some(ColorGradingRef {
        path,
        classname: budget.copy(class)?,
        targetname: copy_in(text(entity, "targetname"), budget)?,
        weight,
        start_disabled: boolean(entity, "StartDisabled")
            .or_else(|| boolean(entity, "startdisabled")),
    }))
}

pub(super) fn text<'a>(entity: &'a Value, key: &str) -> Option<&'a str> {
    entity.str_field(key).filter(|text| !text.is_empty())
}

pub(super) fn copy_in(text: Option<&str>, budget: &mut Budget) -> Result<Option<String>> {
    text.map(|text| budget.copy(text)).transpose()
}

pub(super) fn path_in(text: Option<&str>, budget: &mut Budget) -> Result<Option<String>> {
    text.map(|path| {
        budget.string_of(path.len())?;
        Ok(path.replace('\\', "/"))
    })
    .transpose()
}

pub(super) fn scalar(entity: &Value, key: &str) -> Option<f64> {
    match entity.get(key)? {
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite()),
        value => value.finite_f64(),
    }
}

pub(super) fn vector(entity: &Value, key: &str) -> Option<[f64; 3]> {
    match entity.get(key)? {
        Value::String(text) => {
            let mut parts = text.split_ascii_whitespace();
            let mut result = [0.0; 3];
            for component in &mut result {
                *component = parts
                    .next()?
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())?;
            }
            parts.next().is_none().then_some(result)
        }
        value => {
            let values = value.as_array()?;
            Some([
                values.first()?.finite_f64()?,
                values.get(1)?.finite_f64()?,
                values.get(2)?.finite_f64()?,
            ])
        }
    }
}

pub(super) fn boolean(entity: &Value, key: &str) -> Option<bool> {
    match entity.get(key)? {
        Value::String(text) => match text.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => text.trim().parse::<i64>().ok().map(|n| n != 0),
        },
        value => value.as_bool().or_else(|| value.as_i64().map(|n| n != 0)),
    }
}

fn angles(entity: &Value, key: &str) -> Option<LightAngles> {
    vector(entity, key).map(LightAngles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_lookup_paths_are_read_from_corrections_and_weather_zones() {
        let correction = Value::object([
            ("classname", Value::from("color_correction")),
            (
                "filename",
                Value::from("materials\\corrections\\sunset.raw"),
            ),
            ("maxweight", Value::from("0.6")),
            ("StartDisabled", Value::Bool(true)),
        ]);
        let weather = Value::object([
            ("classname", Value::from("ent_dota_lightinfo")),
            ("targetname", Value::from("rain")),
            ("color_warp_texture", Value::from("materials/grade.vtex")),
            ("color_warp_blend_to_full", Value::Float(0.25)),
        ]);
        let environment = WorldEnvironment::from_lumps(&[EntityLump {
            entities: vec![correction, weather],
            children: Vec::new(),
        }])
        .unwrap();
        let [correction, weather] = environment.color_grading.as_slice() else {
            panic!("expected two authored lookups");
        };
        assert_eq!(correction.path, "materials/corrections/sunset.raw");
        assert_eq!(correction.weight, Some(0.6));
        assert_eq!(correction.start_disabled, Some(true));
        assert_eq!(weather.path, "materials/grade.vtex");
        assert_eq!(weather.targetname.as_deref(), Some("rain"));
    }
}
