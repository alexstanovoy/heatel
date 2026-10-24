//! The functions that work with a model: particles made on and locked to the model a control
//! point carries, and particles drawn as models.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::{Result, malformed};

use super::field::Field;
use super::input::{FloatInput, VectorInput};
use super::read::{MAX_CONTROL_POINT, MAX_NAME, Obj, WHAT};
use super::transform::{ControlPoint, Transform};

/// Most models a model renderer picks from.
pub(super) const MAX_MODELS: usize = 64;

/// `C_INIT_CreateOnModel`'s parameters: a new particle at a place in the hitboxes (or bones) of
/// the model a control point carries.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OnModel {
    /// The control point that carries the model.
    pub model: ControlPoint,
    /// What the direction bias is turned by, when local.
    pub transform: Transform,
    /// Inside the hitboxes rather than on them (0), or the model's volume (1).
    pub force_in_model: i32,
    /// One hitbox rather than any (-1 any).
    pub desired_hitbox: FloatInput,
    /// A control point whose X names the hitbox instead, if any.
    pub hitbox_from_control_point: Option<ControlPoint>,
    /// The hitboxes' scale.
    pub scale: VectorInput,
    /// The share of the hitbox's own velocity the particle starts with.
    pub bone_velocity: f32,
    /// That velocity's longest (0 no limit).
    pub max_bone_velocity: f32,
    /// A bias of the place within the hitbox.
    pub direction_bias: VectorInput,
    /// The hitbox set.
    pub set: String,
    /// The bias is in the transform's frame.
    pub local: bool,
    /// The bones rather than the hitboxes.
    pub use_bones: bool,
    /// The share of the hitbox's depth the place is within, from its surface.
    pub shell_size: FloatInput,
    /// On the model's render mesh rather than its hitboxes.
    pub use_mesh: bool,
}

/// `C_OP_LockToBone`'s parameters: particles moved with the hitbox (or bone) they were made on.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LockToBone {
    /// The control point that carries the model.
    pub model: ControlPoint,
    /// What a jump is measured on.
    pub transform: Transform,
    /// The age (seconds) the lock starts fading at, and is gone at (an end of 0 never).
    pub fade: [f32; 2],
    /// A move of the transform longer than this is a jump: followed whole.
    pub jump_threshold: f32,
    /// The share of the move the previous position follows.
    pub previous_scale: f32,
    /// The hitbox set.
    pub set: String,
    /// Particles are put at their place rather than moved by the hitbox's move.
    pub rigid: bool,
    /// The bones rather than the hitboxes.
    pub use_bones: bool,
    /// The attribute moved.
    pub output: Field,
    /// Its previous value, moved with it.
    pub output_previous: Field,
    /// What of the hitbox's turn the particle takes: 0 none, 1 its rotations, 2 its normal.
    pub rotation: u8,
}

/// `C_OP_RenderModels`' parameters: a model per particle, placed, turned and scaled by it.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Models {
    /// The models (`models/.../x.vmdl`) a particle picks one of, each with its relative chance.
    pub models: Vec<(String, f32)>,
    /// A control point whose model replaces the list, if any.
    pub model_input: Option<ControlPoint>,
    /// That model as its entity has it.
    pub original_model: bool,
    /// The particle's normal does not turn the model.
    pub ignore_normal: bool,
    /// The normal is the model's Z axis rather than its X.
    pub orient_z: bool,
    /// The radius does not scale the model.
    pub ignore_radius: bool,
    /// The scale applies before the turn.
    pub local_scale: bool,
    /// The particle's colour does not tint the model.
    pub suppress_tint: bool,
    /// The model's bounds' centre is put on the particle.
    pub center_offset: bool,
    /// A control point whose position scales the model per axis, if any.
    pub scale_control_point: Option<ControlPoint>,
    /// An offset in the model's frame.
    pub local_offset: VectorInput,
    /// A turn in the model's frame, pitch, yaw and roll in degrees.
    pub local_rotation: VectorInput,
    /// The scale per axis.
    pub component_scale: VectorInput,
    /// Which model of the list, by index (outside it: by chance).
    pub manual_selection: FloatInput,
    /// The skin (material group).
    pub skin: FloatInput,
    /// A material that replaces the model's (`materials/.../x.vmat`), if any.
    pub override_material: Option<String>,
    /// The model plays its animation.
    pub animated: bool,
    /// The radius' scale (the model's scale is the radius times it).
    pub radius_scale: FloatInput,
    /// The roll's scale.
    pub roll_scale: FloatInput,
}

/// The control point a function's `m_modelInput` reads (a missing one reads control point 0,
/// a model before `vpcf57` the control point `legacy_key`); none when it reads a named value or
/// an entity this crate does not read.
pub(super) fn model_control_point(obj: &Obj<'_>, legacy_key: &str) -> Result<Option<ControlPoint>> {
    match obj.object("m_modelInput")? {
        None => Ok(Some(obj.control_point(legacy_key, 0)?)),
        Some(model)
            if model
                .str("m_nType")?
                .is_none_or(|t| t == "PM_TYPE_CONTROL_POINT") =>
        {
            Ok(Some(model.control_point("m_nControlPoint", 0)?))
        }
        Some(_) => Ok(None),
    }
}

/// A hitbox set's name, `default` when absent.
fn set_name(obj: &Obj<'_>) -> Result<String> {
    let set = obj.str("m_HitboxSetName")?.unwrap_or("default");
    if set.len() > MAX_NAME {
        return Err(malformed(WHAT, "a hitbox set's name is too long"));
    }
    Ok(set.to_owned())
}

fn optional_point(obj: &Obj<'_>, key: &str) -> Result<Option<ControlPoint>> {
    let index = obj.int_in(key, -1, -1, i64::from(MAX_CONTROL_POINT))?;
    Ok(u8::try_from(index).ok().and_then(ControlPoint::new))
}

impl OnModel {
    /// The parameters, or none when its model is not a control point's.
    pub(super) fn read(obj: &Obj<'_>) -> Result<Option<Self>> {
        let Some(model) = model_control_point(obj, "m_nControlPointNumber")? else {
            return Ok(None);
        };
        Ok(Some(Self {
            model,
            transform: Transform::read_member(
                obj,
                "m_transformInput",
                "m_nControlPointNumber",
                true,
            )?,
            force_in_model: obj.int_clamped("m_nForceInModel", 0, -1, 2)? as i32,
            desired_hitbox: FloatInput::read(obj, "m_nDesiredHitbox", -1.0)?,
            hitbox_from_control_point: optional_point(obj, "m_nHitboxValueFromControlPointIndex")?,
            scale: VectorInput::read(obj, "m_vecHitBoxScale", [1.0; 3])?,
            bone_velocity: obj.float("m_flBoneVelocity", 0.0)?,
            max_bone_velocity: obj.float("m_flMaxBoneVelocity", 0.0)?,
            direction_bias: VectorInput::read(obj, "m_vecDirectionBias", [0.0; 3])?,
            set: set_name(obj)?,
            local: obj.bool("m_bLocalCoords", false)?,
            use_bones: obj.bool("m_bUseBones", false)?,
            shell_size: FloatInput::read(obj, "m_flShellSize", 1.0)?,
            use_mesh: obj.bool("m_bUseMesh", false)?,
        }))
    }
}

impl LockToBone {
    /// The parameters, or none when its model is not a control point's.
    pub(super) fn read(obj: &Obj<'_>) -> Result<Option<Self>> {
        let Some(model) = model_control_point(obj, "m_nControlPointNumber")? else {
            return Ok(None);
        };
        let rotations = [
            "PARTICLE_ROTATION_LOCK_NONE",
            "PARTICLE_ROTATION_LOCK_ROTATIONS",
            "PARTICLE_ROTATION_LOCK_NORMAL",
        ];
        Ok(Some(Self {
            model,
            transform: Transform::read_member(
                obj,
                "m_transformInput",
                "m_nControlPointNumber",
                true,
            )?,
            fade: [
                obj.float("m_flLifeTimeFadeStart", 0.0)?,
                obj.float("m_flLifeTimeFadeEnd", 0.0)?,
            ],
            jump_threshold: obj.float("m_flJumpThreshold", 100.0)?,
            previous_scale: obj.float("m_flPrevPosScale", 1.0)?,
            set: set_name(obj)?,
            rigid: obj.bool("m_bRigid", false)?,
            use_bones: obj.bool("m_bUseBones", false)?,
            output: obj.field("m_nFieldOutput", Field::POSITION)?,
            output_previous: obj.field("m_nFieldOutputPrev", Field::POSITION_PREVIOUS)?,
            rotation: obj.choice("m_nRotationSetType", &rotations, 0)? as u8,
        }))
    }
}

impl Models {
    pub(super) fn read(obj: &Obj<'_>, format: u32) -> Result<Self> {
        let mut models = Vec::new();
        for entry in obj.array("m_ModelList", MAX_MODELS)? {
            let entry = Obj::new(entry)?;
            let path = entry.str("m_model")?.unwrap_or_default();
            if path.len() > MAX_NAME {
                return Err(malformed(WHAT, "a model's path is too long"));
            }
            let weight = entry.float("m_flRelativeProbabilityOfSpawn", 1.0)?;
            models.push((path.to_owned(), weight));
        }
        let override_material = obj.str("m_hOverrideMaterial")?.filter(|m| !m.is_empty());
        if override_material.is_some_and(|m| m.len() > MAX_NAME) {
            return Err(malformed(WHAT, "an override material's path is too long"));
        }
        Ok(Self {
            models,
            model_input: model_input(obj, format)?,
            original_model: obj.bool("m_bOriginalModel", false)?,
            ignore_normal: obj.bool("m_bIgnoreNormal", false)?,
            orient_z: obj.bool("m_bOrientZ", false)?,
            ignore_radius: obj.bool("m_bIgnoreRadius", false)?,
            local_scale: obj.bool("m_bLocalScale", false)?,
            suppress_tint: obj.bool("m_bSuppressTint", false)?,
            center_offset: obj.bool("m_bCenterOffset", false)?,
            scale_control_point: optional_point(obj, "m_nModelScaleCP")?,
            local_offset: VectorInput::read(obj, "m_vecLocalOffset", [0.0; 3])?,
            local_rotation: VectorInput::read(obj, "m_vecLocalRotation", [0.0; 3])?,
            component_scale: VectorInput::read(obj, "m_vecComponentScale", [1.0; 3])?,
            manual_selection: FloatInput::read(obj, "m_flManualModelSelection", -1.0)?,
            skin: skin(obj, format)?,
            override_material: override_material.map(str::to_owned),
            animated: obj.bool("m_bAnimated", false)?,
            radius_scale: FloatInput::read(obj, "m_flRadiusScale", 1.0)?,
            roll_scale: FloatInput::read(obj, "m_flRollScale", 1.0)?,
        })
    }

    /// The models it draws, by path, without the empty entries.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.models
            .iter()
            .map(|(path, _)| path.as_str())
            .filter(|p| !p.is_empty())
    }
}

/// The format that turned a model renderer's control point into a model input (`vpcf55`).
const MODEL_INPUT: u32 = 55;

/// A model renderer's model input: a control point only when it names one (the renderer's
/// default reads none); before `vpcf55` the control point `m_nModelCP` when set and in range.
fn model_input(obj: &Obj<'_>, format: u32) -> Result<Option<ControlPoint>> {
    if format < MODEL_INPUT && !obj.has("m_modelInput") {
        let cp = obj.int("m_nModelCP", -1)?;
        return Ok(u8::try_from(cp).ok().and_then(ControlPoint::new));
    }
    let Some(input) = obj.object("m_modelInput")? else {
        return Ok(None);
    };
    if input.str("m_nType")? != Some("PM_TYPE_CONTROL_POINT") {
        return Ok(None);
    }
    Ok(Some(input.control_point("m_nControlPoint", 0)?))
}

/// The format that turned a model renderer's skin control point into a skin input (`vpcf58`).
const SKIN_INPUT: u32 = 58;

/// The skin: before `vpcf58` a control point's X when `m_nSkinCP` named one.
fn skin(obj: &Obj<'_>, format: u32) -> Result<FloatInput> {
    if format < SKIN_INPUT && obj.int("m_nSkinCP", -1)? != -1 {
        return Ok(FloatInput::ControlPointComponent {
            control_point: obj.control_point("m_nSkinCP", 0)?,
            component: 0,
            mapping: super::input::Mapping::Direct,
        });
    }
    FloatInput::read(obj, "m_nSkin", 0.0)
}
