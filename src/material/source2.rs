//! Authored material parameters, including values outside the glTF material model.

use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::{Budget, Resource};

use super::look::shader_stem;
use super::vmat::param_entries;

/// The authored `DATA` of a compiled Source 2 material.
#[derive(Clone, Debug, PartialEq)]
pub struct Source2Material {
    data: Value,
}

/// One value in a material's parameter or attribute table.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum MaterialParameter<'a> {
    /// An integer, including shader switches stored as integers.
    Int(i64),
    /// A shader scalar.
    Float(f32),
    /// A four-component vector, including constant values for texture inputs.
    Vector([f32; 4]),
    /// A texture resource path.
    Texture(&'a str),
    /// An authored string.
    String(&'a str),
    /// Compiled dynamic parameter expression bytes.
    Dynamic(&'a [u8]),
    /// Compiled dynamic texture expression bytes.
    DynamicTexture(&'a [u8]),
}

type DecodeParameter = for<'a> fn(&'a Value) -> Option<MaterialParameter<'a>>;

const PARAMETERS: &[(&str, &str, DecodeParameter)] = &[
    ("m_intParams", "m_nValue", int),
    ("m_floatParams", "m_flValue", float),
    ("m_vectorParams", "m_value", vector),
    ("m_textureParams", "m_pValue", texture),
    ("m_dynamicParams", "m_value", |v| {
        v.as_blob().map(MaterialParameter::Dynamic)
    }),
    ("m_dynamicTextureParams", "m_value", |v| {
        v.as_blob().map(MaterialParameter::DynamicTexture)
    }),
];

const ATTRIBUTES: &[(&str, &str, DecodeParameter)] = &[
    ("m_intAttributes", "m_nValue", int),
    ("m_floatAttributes", "m_flValue", float),
    ("m_vectorAttributes", "m_value", vector),
    ("m_textureAttributes", "m_pValue", texture),
    ("m_stringAttributes", "m_value", |v| {
        v.as_str().map(MaterialParameter::String)
    }),
];

fn int(value: &Value) -> Option<MaterialParameter<'_>> {
    value.as_i64().map(MaterialParameter::Int)
}

fn float(value: &Value) -> Option<MaterialParameter<'_>> {
    value.finite_f32().map(MaterialParameter::Float)
}

fn vector(value: &Value) -> Option<MaterialParameter<'_>> {
    value.f32_array().map(MaterialParameter::Vector)
}

fn texture(value: &Value) -> Option<MaterialParameter<'_>> {
    value.as_str().map(MaterialParameter::Texture)
}

impl Source2Material {
    /// Reads the material's KV3 or NTRO `DATA` within the caller's budget.
    pub fn read(resource: &Resource, budget: &mut Budget) -> Result<Self> {
        let data = resource
            .data_in(budget)
            .ok_or_else(|| malformed("material", "no KV3 or NTRO DATA block"))??;
        Self::from_vmat(data)
    }

    /// Takes ownership of decoded material `DATA` with a shader name.
    pub fn from_vmat(data: Value) -> Result<Self> {
        if data.str_field("m_shaderName").is_none_or(str::is_empty) {
            return Err(malformed("material", "missing m_shaderName"));
        }
        Ok(Self { data })
    }

    /// The original decoded material data, including any extra fields.
    pub fn data(&self) -> &Value {
        &self.data
    }

    /// The exact shader name as stored in the resource.
    pub fn shader_name(&self) -> &str {
        self.data.str_field("m_shaderName").unwrap_or_default()
    }

    /// The material resource name as stored in the file.
    pub fn material_name(&self) -> Option<&str> {
        self.data.str_field("m_materialName")
    }

    /// All typed shader parameters, in table and file order, including repeated names.
    pub fn parameters(&self) -> impl Iterator<Item = (&str, MaterialParameter<'_>)> {
        self.entries(PARAMETERS)
    }

    /// All typed material attributes, in table and file order.
    pub fn attributes(&self) -> impl Iterator<Item = (&str, MaterialParameter<'_>)> {
        self.entries(ATTRIBUTES)
    }

    /// Attribute names the shader uses at render time.
    pub fn render_attributes_used(&self) -> impl Iterator<Item = &str> {
        self.data
            .array("m_renderAttributesUsed")
            .iter()
            .filter_map(Value::as_str)
    }

    /// All authored texture slots in file order, including repeated names.
    pub fn textures(&self) -> impl DoubleEndedIterator<Item = (&str, &str)> {
        param_entries(&self.data, "m_textureParams", "m_pValue")
            .filter_map(|(key, value)| Some((key, value.as_str()?)))
    }

    /// A texture slot's last path.
    pub fn texture(&self, name: &str) -> Option<&str> {
        self.textures()
            .rfind(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    /// An integer parameter's last value.
    pub fn int(&self, name: &str) -> Option<i64> {
        self.parameter(name, "m_intParams", "m_nValue", Value::as_i64)
    }

    /// A float parameter's last finite value.
    pub fn float(&self, name: &str) -> Option<f32> {
        self.parameter(name, "m_floatParams", "m_flValue", Value::finite_f32)
    }

    /// The last finite scalar for a name, accepting integer storage when no float is present.
    pub fn scalar(&self, name: &str) -> Option<f32> {
        self.float(name)
            .or_else(|| self.int(name).map(|n| n as f32))
    }

    /// A vector parameter's last finite value.
    pub fn vector(&self, name: &str) -> Option<[f32; 4]> {
        self.parameter(name, "m_vectorParams", "m_value", Value::f32_array)
    }

    /// A shader switch as a boolean, with `None` for an unauthored switch.
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.int(name).map(|n| n != 0)
    }

    fn entries(
        &self,
        lists: &'static [(&str, &str, DecodeParameter)],
    ) -> impl Iterator<Item = (&str, MaterialParameter<'_>)> {
        lists.iter().flat_map(|&(list, field, decode)| {
            param_entries(&self.data, list, field)
                .filter_map(move |(name, value)| Some((name, decode(value)?)))
        })
    }

    fn parameter<'a, T>(
        &'a self,
        name: &str,
        list: &str,
        field: &str,
        decode: impl Fn(&'a Value) -> Option<T>,
    ) -> Option<T> {
        param_entries(&self.data, list, field)
            .rev()
            .filter(|(key, _)| *key == name)
            .find_map(|(_, value)| decode(value))
    }
}

/// Common inputs of the hero shader. Each optional field represents an authored value.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct HeroShaderParams<'a> {
    /// Base color texture.
    pub color: Option<&'a str>,
    /// Normal map.
    pub normal: Option<&'a str>,
    /// First packed hero mask texture.
    pub masks1: Option<&'a str>,
    /// Second packed hero mask texture.
    pub masks2: Option<&'a str>,
    /// Diffuse response lookup texture.
    pub diffuse_warp: Option<&'a str>,
    /// Packed Fresnel warp texture. The hero shader reads rim, color and specular from RGB.
    pub fresnel_warp: Option<&'a str>,
    /// Separate color Fresnel texture, when authored.
    pub fresnel_warp_color: Option<&'a str>,
    /// Separate rim Fresnel texture, when authored.
    pub fresnel_warp_rim: Option<&'a str>,
    /// Three-dimensional Fresnel color lookup texture.
    pub fresnel_color_warp_3d: Option<&'a str>,
    /// Specular response lookup texture.
    pub specular_warp: Option<&'a str>,
    /// Environment reflection texture.
    pub cube_map: Option<&'a str>,
    /// Separate mask for environment reflections.
    pub cube_map_separate_mask: Option<&'a str>,
    /// First detail texture.
    pub detail: Option<&'a str>,
    /// Second detail texture.
    pub detail2: Option<&'a str>,
    /// Color tint mask texture.
    pub tint_mask: Option<&'a str>,
    /// Specular intensity.
    pub specular_scale: Option<f32>,
    /// Specular highlight exponent.
    pub specular_exponent: Option<f32>,
    /// Rim light intensity.
    pub rim_light_scale: Option<f32>,
    /// Authored `g_flSelfIllumScale`.
    pub self_illum_scale: Option<f32>,
    /// Authored `g_flIlluminationScale`.
    pub illumination_scale: Option<f32>,
    /// Authored scalar `g_flDetail1Scale`.
    pub detail1_scale: Option<f32>,
    /// First detail blend factor, including the `g_flDetailBlendFactor` spelling.
    pub detail1_blend_factor: Option<f32>,
    /// Specular light color.
    pub specular_color: Option<[f32; 4]>,
    /// Rim light color.
    pub rim_light_color: Option<[f32; 4]>,
    /// First detail UV scale vector.
    pub detail_tex_coord_scale: Option<[f32; 4]>,
    /// Whether alpha testing is enabled.
    pub alpha_test: Option<bool>,
    /// Whether translucent blending is enabled.
    pub translucent: Option<bool>,
    /// Whether shadow casting is suppressed.
    pub do_not_cast_shadows: Option<bool>,
    /// Whether specular lighting uses a cube map.
    pub specular_cube_map: Option<bool>,
    /// Whether metalness masks the cube map contribution.
    pub mask_cube_map_by_metalness: Option<bool>,
}

/// Common inputs of a four-layer terrain shader.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TerrainShaderParams<'a> {
    /// Layers 0 through 3 in index order.
    pub layers: [TerrainLayerParams<'a>; 4],
    /// First packed tint mask shared by the layers.
    pub tint_masks: Option<&'a str>,
    /// Second packed tint mask shared by the layers.
    pub tint2_masks: Option<&'a str>,
    /// Packed per-layer parallax mask.
    pub parallax_mask: Option<&'a str>,
    /// Whether the texture coordinates are in world space.
    pub worldspace_uvs: Option<bool>,
    /// Color tint applied across layers.
    pub global_tint: Option<[f32; 4]>,
}

/// Authored textures, UV controls and tints for one terrain layer.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct TerrainLayerParams<'a> {
    /// Base color texture.
    pub color: Option<&'a str>,
    /// Normal map.
    pub normal: Option<&'a str>,
    /// Packed specular texture for this layer.
    pub specular: Option<&'a str>,
    /// Layer UV scale.
    pub tex_coord_scale: Option<f32>,
    /// Layer UV rotation.
    pub tex_coord_rotation: Option<f32>,
    /// Layer UV offset.
    pub tex_coord_offset: Option<[f32; 4]>,
    /// Layer UV scrolling vector.
    pub tex_coord_scroll: Option<[f32; 4]>,
    /// Primary layer tint.
    pub tint: Option<[f32; 4]>,
    /// Secondary layer tint.
    pub tint_b: Option<[f32; 4]>,
    /// Third layer tint.
    pub tint_c: Option<[f32; 4]>,
    /// Fourth layer tint.
    pub tint_d: Option<[f32; 4]>,
}

impl Source2Material {
    /// Returns the hero inputs when the material uses a hero shader.
    pub fn hero_params(&self) -> Option<HeroShaderParams<'_>> {
        if !matches!(shader_stem(self.shader_name()), "hero" | "hero_fluid") {
            return None;
        }
        Some(HeroShaderParams {
            color: self.texture("g_tColor"),
            normal: self.texture("g_tNormal"),
            masks1: self.texture("g_tMasks1"),
            masks2: self.texture("g_tMasks2"),
            diffuse_warp: self.texture("g_tDiffuseWarp"),
            fresnel_warp: self.texture("g_tFresnelWarp"),
            fresnel_warp_color: self.texture("g_tFresnelWarpColor"),
            fresnel_warp_rim: self.texture("g_tFresnelWarpRim"),
            fresnel_color_warp_3d: self.texture("g_tFresnelColorWarp3D"),
            specular_warp: self.texture("g_tSpecularWarp"),
            cube_map: self.texture("g_tCubeMap"),
            cube_map_separate_mask: self.texture("g_tCubeMapSeparateMask"),
            detail: self.texture("g_tDetail"),
            detail2: self.texture("g_tDetail2"),
            tint_mask: self.texture("g_tTintMask"),
            specular_scale: self.scalar("g_flSpecularScale"),
            specular_exponent: self.scalar("g_flSpecularExponent"),
            rim_light_scale: self.scalar("g_flRimLightScale"),
            self_illum_scale: self.scalar("g_flSelfIllumScale"),
            illumination_scale: self.scalar("g_flIlluminationScale"),
            detail1_scale: self.scalar("g_flDetail1Scale"),
            detail1_blend_factor: self
                .scalar("g_flDetail1BlendFactor")
                .or_else(|| self.scalar("g_flDetailBlendFactor")),
            specular_color: self.vector("g_vSpecularColor"),
            rim_light_color: self.vector("g_vRimLightColor"),
            detail_tex_coord_scale: self.vector("g_vDetailTexCoordScale"),
            alpha_test: self.flag("F_ALPHA_TEST"),
            translucent: self.flag("F_TRANSLUCENT"),
            do_not_cast_shadows: self.flag("F_DO_NOT_CAST_SHADOWS"),
            specular_cube_map: self.flag("F_SPECULAR_CUBE_MAP"),
            mask_cube_map_by_metalness: self.flag("F_MASK_CUBE_MAP_BY_METALNESS"),
        })
    }

    /// Returns the indexed layers when the material uses a terrain shader.
    pub fn terrain_params(&self) -> Option<TerrainShaderParams<'_>> {
        if !matches!(
            shader_stem(self.shader_name()),
            "multiblend" | "dota_terrain"
        ) {
            return None;
        }
        Some(TerrainShaderParams {
            layers: std::array::from_fn(|index| self.terrain_layer(index)),
            tint_masks: self.texture("g_tTintMasks"),
            tint2_masks: self.texture("g_tTint2Masks"),
            parallax_mask: self.texture("g_tParallaxMask"),
            worldspace_uvs: self.flag("F_WORLDSPACE_UVS"),
            global_tint: self.vector("g_vGlobalTint"),
        })
    }

    fn terrain_layer(&self, index: usize) -> TerrainLayerParams<'_> {
        TerrainLayerParams {
            color: self.texture(&format!("g_tColor{index}")),
            normal: self.texture(&format!("g_tNormal{index}")),
            specular: self.texture(&format!("g_tSpecular{index}")),
            tex_coord_scale: self.scalar(&format!("g_flTexCoordScale{index}")),
            tex_coord_rotation: self.scalar(&format!("g_flTexCoordRotate{index}")),
            tex_coord_offset: self.vector(&format!("g_vTexCoordOffset{index}")),
            tex_coord_scroll: self.vector(&format!("g_vTexCoordScroll{index}")),
            tint: self.vector(&format!("g_vColorTint{index}")),
            tint_b: self.vector(&format!("g_vColorTintB{index}")),
            tint_c: self.vector(&format!("g_vColorTintC{index}")),
            tint_d: self.vector(&format!("g_vColorTintD{index}")),
        }
    }
}
