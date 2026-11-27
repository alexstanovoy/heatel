mod convert;
mod inputs;
mod look;
mod pack;
mod source2;
mod vmat;

pub(crate) use convert::{
    GltfMaterial, Images, convert, convert_world, extensions_used, linear_tint,
};
pub use look::{FRESNEL_LUT_POINTS, FigureLook, GroundLayerLook, WaterLook, sample_fresnel_lut};
pub use pack::{Pack, orm_uri};
pub use source2::{
    HeroShaderParams, MaterialParameter, Source2Material, TerrainLayerParams, TerrainShaderParams,
};
pub use vmat::{Alpha, Material};

#[cfg(test)]
mod source2_tests;
