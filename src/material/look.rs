//! Neutral shading parameter structs extracted from water, multi-layer terrain, and character
//! materials, plus Fresnel lookup-table sampling.

use serde_json::{Value as Json, json};

use super::vmat::Material;
use crate::error::{Result, malformed};
use crate::texture::Texture;

/// Number of uniformly spaced samples along `u` in a Fresnel lookup table (`0, 1/16, ..., 1`).
pub const FRESNEL_LUT_POINTS: usize = 17;

/// Neutral surface rendering parameters extracted from a water material.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct WaterLook {
    /// Normal-map UV tiling scale.
    pub normal_uv_scale: f32,
    /// Flow cycle period in seconds.
    pub flow_interval: f32,
    /// Flow UV scroll distance per cycle.
    pub flow_scroll: f32,
    /// Flow noise UV scale.
    pub noise_uv_scale: f32,
    /// Flow noise strength.
    pub noise_strength: f32,
    /// Normal-map bump strength.
    pub bump_strength: f32,
    /// Screen-space refraction strength.
    pub refraction_amount: f32,
    /// RGB tint applied to refracted light.
    pub refraction_tint: [f32; 3],
    /// Reflection blend factor.
    pub reflection_amount: f32,
    /// Specular reflection exponent.
    pub reflection_power: f32,
    /// Reflection RGB colour.
    pub reflection_color: [f32; 3],
    /// Reflection direction vector.
    pub reflection_dir: [f32; 3],
    /// Fresnel reflectance factor.
    pub reflectance: f32,
    /// Depth scale in world units over which underwater fog saturates.
    pub depth: f32,
    /// Underwater fog RGB colour.
    pub fog_color: [f32; 3],
    /// Forced fog density range `[min, max]`.
    pub fog_override: [f32; 2],
    /// Default 2D flow vector when no flow-map texture is present.
    pub default_flow: [f32; 2],
    /// Fallback surface RGB colour for non-refraction paths.
    pub fallback_surface_color: [f32; 3],
    /// Fallback surface opacity.
    pub fallback_opacity: f32,
    /// Fallback surface lighting factor.
    pub fallback_surface_factor: f32,
    /// Fallback minimum surface brightness.
    pub fallback_min_color: f32,
    /// Fallback reflection RGB colour.
    pub fallback_reflection_color: [f32; 3],
    /// Fallback reflection exponent.
    pub fallback_reflection_power: f32,
    /// Fallback reflection direction vector.
    pub fallback_reflection_dir: [f32; 3],
    /// Fallback bump strength.
    pub fallback_bump_strength: f32,
}

/// Neutral per-layer parameters extracted from a multi-layer terrain material.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct GroundLayerLook {
    /// Primary layer RGB tint.
    pub tint_a: [f32; 3],
    /// Secondary layer RGB tint (selected by the layer's tint-mask channel).
    pub tint_b: [f32; 3],
    /// World-space UV tiling scale (`> 0.0`).
    pub scale: f32,
    /// UV rotation in degrees.
    pub rotation: f32,
}

/// Neutral shading parameters extracted from a character material.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct FigureLook {
    /// Rim highlight intensity scale.
    pub rim_scale: f32,
    /// Rim highlight RGB colour.
    pub rim_color: [f32; 3],
    /// Specular highlight exponent.
    pub spec_exp: f32,
    /// Specular highlight intensity scale.
    pub spec_scale: f32,
    /// Specular highlight RGB colour.
    pub spec_color: [f32; 3],
    /// Hemisphere ambient multiplier.
    pub ambient_scale: f32,
    /// Self-illumination RGB tint.
    pub glow_tint: [f32; 3],
    /// Self-illumination intensity scale.
    pub glow_scale: f32,
    /// Minimum floor values for mask channels: `[diffuse_warp, env_map, metalness, glow, spec,
    /// spec_exp]`.
    pub blend_full: [f32; 6],
}

impl Material {
    /// Extracts neutral water rendering parameters when this material uses a water shader.
    ///
    /// # Errors
    ///
    /// Returns an error if any water parameter is non-finite.
    pub fn water_look(&self) -> Result<Option<WaterLook>> {
        WaterLook::from_material(self)
    }

    /// Extracts the four neutral terrain blend layers when this material uses a multi-layer
    /// terrain shader.
    ///
    /// # Errors
    ///
    /// Returns an error if any layer parameter is non-finite or has a non-positive UV scale.
    pub fn ground_layers(&self) -> Result<Option<[GroundLayerLook; 4]>> {
        GroundLayerLook::from_material(self)
    }

    /// Extracts neutral character shading parameters when this material uses a character shader.
    ///
    /// # Errors
    ///
    /// Returns an error if any shading parameter is non-finite.
    pub fn figure_look(&self) -> Result<Option<FigureLook>> {
        FigureLook::from_material(self)
    }
}

impl WaterLook {
    /// Extracts water rendering parameters from `material`, returning `Ok(None)` for non-water
    /// materials.
    pub fn from_material(material: &Material) -> Result<Option<Self>> {
        if shader_stem(&material.shader) != "water_dota" {
            return Ok(None);
        }
        let f = |key: &str, default: f32, label: &str| {
            finite_float(material, key, default, "water look", label)
        };
        let v3 = |key: &str, default: [f32; 3], label: &str| {
            finite_vec3(material, key, default, "water look", label)
        };
        let v2 = |key: &str, default: [f32; 2], label: &str| {
            finite_vec2(material, key, default, "water look", label)
        };
        Ok(Some(Self {
            normal_uv_scale: f("g_flNormalUvScale", 500.0, "normal uv scale")?,
            flow_interval: f("g_flNormalFlowTimeIntervalInSeconds", 0.6, "flow interval")?,
            flow_scroll: f("g_flNormalFlowUvScrollDistance", 0.18, "flow scroll")?,
            noise_uv_scale: f("g_flNoiseUvScale", 0.0005, "noise uv scale")?,
            noise_strength: f("g_flNoiseStrength", 2.0, "noise strength")?,
            bump_strength: f("g_flBumpStrength", 1.0, "bump strength")?,
            refraction_amount: f("g_flRefractionAmount", 0.06, "refraction amount")?,
            refraction_tint: v3("g_vRefractionTint", [1.0; 3], "refraction tint")?,
            reflection_amount: f("g_flReflectionAmount", 0.2, "reflection amount")?,
            reflection_power: f("g_flReflectionPower", 2.403, "reflection power")?,
            reflection_color: v3("g_vReflectionColor", [1.0; 3], "reflection colour")?,
            reflection_dir: v3("g_vReflectionDir", [0.0, -1.0, 0.0], "reflection direction")?,
            reflectance: f("g_flReflectance", 0.33, "reflectance")?,
            depth: f("g_flWaterDepth", 35.0, "water depth")?,
            fog_color: v3("g_vWaterFogColor", [0.0; 3], "fog colour")?,
            fog_override: [
                f("g_flForceFogOverrideMin", 0.0, "fog override min")?,
                f("g_flForceFogOverrideMax", 0.0, "fog override max")?,
            ],
            default_flow: v2("TextureFlow", [0.0, 0.0], "default flow")?,
            fallback_surface_color: v3(
                "g_vLowEndSurfaceColor",
                [0.184, 0.275, 0.243],
                "fallback surface colour",
            )?,
            fallback_opacity: f("g_flLowEndSurfaceOpacity", 0.875, "fallback opacity")?,
            fallback_surface_factor: f(
                "g_flLowEndSurfaceFactor",
                0.224,
                "fallback surface factor",
            )?,
            fallback_min_color: f(
                "g_flLowEndSurfaceMinimumColor",
                0.104,
                "fallback minimum colour",
            )?,
            fallback_reflection_color: v3(
                "g_vLowEndReflectionColor",
                [0.729; 3],
                "fallback reflection colour",
            )?,
            fallback_reflection_power: f(
                "g_flLowEndReflectionPower",
                2.247,
                "fallback reflection power",
            )?,
            fallback_reflection_dir: v3(
                "g_vLowEndReflectionDir",
                [-0.03, -1.0, 0.273],
                "fallback reflection direction",
            )?,
            fallback_bump_strength: f("g_flLowEndBumpStrength", 0.15, "fallback bump strength")?,
        }))
    }

    pub(crate) fn to_json(self) -> Json {
        json!({
            "water": {
                "normal_uv_scale": self.normal_uv_scale,
                "flow_interval": self.flow_interval,
                "flow_scroll": self.flow_scroll,
                "noise_uv_scale": self.noise_uv_scale,
                "noise_strength": self.noise_strength,
                "bump_strength": self.bump_strength,
                "refraction_amount": self.refraction_amount,
                "refraction_tint": self.refraction_tint,
                "reflection_amount": self.reflection_amount,
                "reflection_power": self.reflection_power,
                "reflection_color": self.reflection_color,
                "reflection_dir": self.reflection_dir,
                "reflectance": self.reflectance,
                "depth": self.depth,
                "fog_color": self.fog_color,
                "fog_override": self.fog_override,
                "default_flow": self.default_flow,
                "fallback_surface_color": self.fallback_surface_color,
                "fallback_opacity": self.fallback_opacity,
                "fallback_surface_factor": self.fallback_surface_factor,
                "fallback_min_color": self.fallback_min_color,
                "fallback_reflection_color": self.fallback_reflection_color,
                "fallback_reflection_power": self.fallback_reflection_power,
                "fallback_reflection_dir": self.fallback_reflection_dir,
                "fallback_bump_strength": self.fallback_bump_strength,
            }
        })
    }
}

impl GroundLayerLook {
    /// Extracts the four terrain blend layers from `material`, returning `Ok(None)` for
    /// non-terrain materials.
    pub fn from_material(material: &Material) -> Result<Option<[Self; 4]>> {
        if shader_stem(&material.shader) != "multiblend" {
            return Ok(None);
        }
        let mut layers = [Self {
            tint_a: [1.0; 3],
            tint_b: [1.0; 3],
            scale: 1.0,
            rotation: 0.0,
        }; 4];
        for (i, slot) in layers.iter_mut().enumerate() {
            let tint_a = finite_vec3(
                material,
                &format!("g_vColorTint{i}"),
                [1.0; 3],
                "ground layer",
                &format!("layer {i} primary tint"),
            )?;
            let tint_b = finite_vec3(
                material,
                &format!("g_vColorTintB{i}"),
                [1.0; 3],
                "ground layer",
                &format!("layer {i} secondary tint"),
            )?;
            let scale = finite_float(
                material,
                &format!("g_flTexCoordScale{i}"),
                1.0,
                "ground layer",
                &format!("layer {i} scale"),
            )?;
            if scale <= 0.0 {
                return Err(malformed(
                    "ground layer",
                    format!("layer {i} scale must be positive"),
                ));
            }
            let rotation = finite_float(
                material,
                &format!("g_flTexCoordRotate{i}"),
                0.0,
                "ground layer",
                &format!("layer {i} rotation"),
            )?;
            *slot = Self {
                tint_a,
                tint_b,
                scale,
                rotation,
            };
        }
        Ok(Some(layers))
    }

    pub(crate) fn layers_to_json(layers: &[Self; 4]) -> Json {
        let items: Vec<Json> = layers
            .iter()
            .map(|l| {
                json!({
                    "tint_a": l.tint_a,
                    "tint_b": l.tint_b,
                    "scale": l.scale,
                    "rotation": l.rotation,
                })
            })
            .collect();
        json!({ "ground_layers": items })
    }
}

impl FigureLook {
    /// Extracts character shading parameters from `material`, returning `Ok(None)` for
    /// non-character materials.
    pub fn from_material(material: &Material) -> Result<Option<Self>> {
        if shader_stem(&material.shader) != "hero" {
            return Ok(None);
        }
        let f = |key: &str, default: f32, label: &str| {
            finite_float(material, key, default, "figure look", label)
        };
        let v3 = |key: &str, default: [f32; 3], label: &str| {
            finite_vec3(material, key, default, "figure look", label)
        };
        Ok(Some(Self {
            rim_scale: f("g_flRimLightScale", 1.0, "rim scale")?,
            rim_color: v3("g_vRimLightColor", [1.0; 3], "rim colour")?,
            spec_exp: f("g_flSpecularExponent", 32.0, "specular exponent")?,
            spec_scale: f("g_flSpecularScale", 1.0, "specular scale")?,
            spec_color: v3("g_vSpecularColor", [1.0; 3], "specular colour")?,
            ambient_scale: f("g_flAmbientScale", 1.0, "ambient scale")?,
            glow_tint: v3("g_vSelfIllumTint", [1.0; 3], "glow tint")?,
            glow_scale: f("g_flSelfIllumScale", 1.0, "glow scale")?,
            blend_full: [
                f("g_flDiffuseWarpBlendToFull", 0.0, "diffuse warp floor")?,
                f("g_flEnvMapBlendToFull", 0.0, "env map floor")?,
                f("g_flMetalnessBlendToFull", 0.0, "metalness floor")?,
                f("g_flSelfIllumBlendToFull", 0.0, "glow floor")?,
                f("g_flSpecularBlendToFull", 0.0, "specular floor")?,
                f(
                    "g_flSpecularExponentBlendToFull",
                    0.0,
                    "specular exponent floor",
                )?,
            ],
        }))
    }

    pub(crate) fn to_json(self) -> Json {
        json!({
            "rim": {
                "scale": self.rim_scale,
                "color": self.rim_color,
            },
            "spec": {
                "exp": self.spec_exp,
                "scale": self.spec_scale,
                "color": self.spec_color,
            },
            "ambient": self.ambient_scale,
            "glow": {
                "tint": self.glow_tint,
                "scale": self.glow_scale,
            },
            "blend_full": self.blend_full,
        })
    }
}

/// Samples a 2D Fresnel lookup texture along its middle row (`v = 0.5`) at [`FRESNEL_LUT_POINTS`]
/// evenly spaced `u` positions (`0, 1/16, ..., 15/16, 1`), returning `[rim, color, spec]` RGB
/// bytes per sample.
///
/// # Errors
///
/// Returns an error if the texture is empty or cannot be decoded to RGBA8.
pub fn sample_fresnel_lut(texture: &Texture) -> Result<[[u8; 3]; FRESNEL_LUT_POINTS]> {
    let width = texture.width() as usize;
    let height = texture.height() as usize;
    if width == 0 || height == 0 {
        return Err(malformed("fresnel lut", "empty texture"));
    }
    let rgba = texture.to_rgba8()?;
    let row = height / 2;
    let last = FRESNEL_LUT_POINTS - 1;
    let mut table = [[0u8; 3]; FRESNEL_LUT_POINTS];
    for (i, entry) in table.iter_mut().enumerate() {
        let x = if i == last {
            width - 1
        } else {
            i * width / last
        };
        let offset = (row * width + x) * 4;
        let pixel = rgba
            .get(offset..offset + 3)
            .ok_or_else(|| malformed("fresnel lut", "pixel index out of bounds"))?;
        *entry = [pixel[0], pixel[1], pixel[2]];
    }
    Ok(table)
}

pub(super) fn shader_stem(shader: &str) -> &str {
    shader.rsplit_once('.').map_or(shader, |(stem, _)| stem)
}

fn finite_float(
    material: &Material,
    key: &str,
    default: f32,
    what: &'static str,
    label: &str,
) -> Result<f32> {
    match material.float(key) {
        Some(v) if v.is_finite() => Ok(v),
        Some(_) => Err(malformed(what, format!("non-finite {label}"))),
        None => Ok(default),
    }
}

fn finite_vec2(
    material: &Material,
    key: &str,
    default: [f32; 2],
    what: &'static str,
    label: &str,
) -> Result<[f32; 2]> {
    match material.vector(key) {
        Some(v) if v[0].is_finite() && v[1].is_finite() => Ok([v[0], v[1]]),
        Some(_) => Err(malformed(what, format!("non-finite {label}"))),
        None => Ok(default),
    }
}

fn finite_vec3(
    material: &Material,
    key: &str,
    default: [f32; 3],
    what: &'static str,
    label: &str,
) -> Result<[f32; 3]> {
    match material.vector(key) {
        Some(v) if v[0].is_finite() && v[1].is_finite() && v[2].is_finite() => {
            Ok([v[0], v[1], v[2]])
        }
        Some(_) => Err(malformed(what, format!("non-finite {label}"))),
        None => Ok(default),
    }
}
