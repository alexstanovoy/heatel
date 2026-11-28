//! Skeleton and attachment data for Source-space playback and wearable bone merging.

use std::collections::HashMap;

use super::{SourceBone, floats, number, source_skeleton};
use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::items_game::HeroItem;
use crate::kv3::{self, Value};
use crate::resource::Resource;

/// The model's rig, read independently of the render meshes.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRig {
    /// The skeleton in model order, with Source-space rest transforms.
    pub bones: Vec<SourceBone>,
    /// Named attachment transforms from the model and its embedded meshes.
    pub attachments: Vec<WeightedAttachment>,
    /// VMDL resource dependencies, in RERL order. This includes animation-scoped props.
    pub submodels: Vec<String>,
}

/// A named attachment with all its authored influences.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedAttachment {
    /// Name of the attachment.
    pub name: String,
    /// Whether the attachment ignores rotation.
    pub ignore_rotation: bool,
    /// Influences in authored order.
    pub influences: Vec<AttachmentInfluence>,
}

/// One influence of an attachment, in Source coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentInfluence {
    /// Authored bone name.
    pub bone_name: String,
    /// Index into [ModelRig::bones], or none if the name has no match in this skeleton.
    pub bone: Option<usize>,
    /// Authored influence weight.
    pub weight: f32,
    /// Local offset in inches.
    pub position: [f32; 3],
    /// Local rotation quaternion in XYZW order.
    pub rotation: [f32; 4],
    /// Whether the influence uses the root transform.
    pub root_transform: bool,
}

impl ModelRig {
    /// Reads a VMDL's skeleton, attachments and submodel dependencies, including attachments
    /// from embedded meshes of every level of detail.
    pub fn read(resource: &Resource) -> Result<Self> {
        let mut budget = Budget::named("model rig");
        let data = resource
            .data_in(&mut budget)
            .ok_or_else(|| malformed("model", "no DATA"))??;
        let bones = source_skeleton(&data, &mut budget)?;
        let names = bone_indices(&bones);
        let mut attachments = Vec::new();
        read_attachments(&data, &names, &mut attachments, &mut budget)?;
        if let Some(ctrl) = resource.kv3_in(b"CTRL", &mut budget) {
            for mesh in ctrl?.root.array("embedded_meshes") {
                let block = number(mesh, &["m_nDataBlock", "data_block"])
                    .and_then(|i| usize::try_from(i).ok())
                    .and_then(|i| resource.blocks().get(i))
                    .ok_or_else(|| malformed("model", "embedded mesh has no data block"))?;
                let data = kv3::decode_in(resource.bytes(block)?, &mut budget)?.root;
                read_attachments(&data, &names, &mut attachments, &mut budget)?;
            }
        }
        let submodels = resource
            .external_refs_in(&mut budget)?
            .into_iter()
            .map(|reference| reference.name)
            .filter(|path| path.ends_with(".vmdl") || path.ends_with(".vmdl_c"))
            .collect();
        Ok(Self {
            bones,
            attachments,
            submodels,
        })
    }

    /// Maps a wearable's bones (self) to their indices in the base rig by case-insensitive name.
    /// Unmatched bones remain available for the wearable's own hierarchy.
    pub fn bone_merge_map(&self, base: &Self) -> Vec<Option<usize>> {
        let indices = bone_indices(&base.bones);
        self.bones
            .iter()
            .map(|bone| indices.get(&bone.name.to_lowercase()).copied())
            .collect()
    }
}

pub(crate) fn bone_indices(bones: &[SourceBone]) -> HashMap<String, usize> {
    let mut names = HashMap::with_capacity(bones.len());
    for (index, bone) in bones.iter().enumerate() {
        names.entry(bone.name.to_lowercase()).or_insert(index);
    }
    names
}

/// Model-bearing default cosmetics for a hero's base form, in item registry order.
/// Persona slots target their own skeletons. Pass each model's rig to
/// [ModelRig::bone_merge_map] to resolve it against the base hero.
pub fn default_wearables<'a>(
    items: &'a [HeroItem],
    hero: &'a str,
) -> impl Iterator<Item = &'a HeroItem> + 'a {
    items.iter().filter(move |item| {
        item.is_default()
            && item
                .slot
                .as_deref()
                .is_none_or(|slot| !slot.contains("_persona_") && slot != "hero_base")
            && item.model.as_ref().is_some_and(|path| !path.is_empty())
            && item.heroes.iter().any(|name| name == hero)
    })
}

fn read_attachments(
    data: &Value,
    names: &HashMap<String, usize>,
    out: &mut Vec<WeightedAttachment>,
    budget: &mut Budget,
) -> Result<()> {
    for entry in data.array("m_attachments") {
        let value = entry.get("value").unwrap_or(entry);
        let name = value
            .str_field("m_name")
            .or_else(|| entry.str_field("key"))
            .unwrap_or_default();
        if name.is_empty() || out.iter().any(|attachment| attachment.name == name) {
            continue;
        }
        if out.len() == super::MAX_ATTACHMENTS {
            return Err(malformed("model", "more attachments than any model has"));
        }
        let count = value.usize_field("m_nInfluences").unwrap_or(1);
        if count > 3 {
            return Err(malformed("model", "more than three attachment influences"));
        }
        budget.slots::<WeightedAttachment>(1)?;
        budget.slots::<AttachmentInfluence>(count)?;
        let influences = (0..count)
            .map(|index| influence(value, names, index, budget))
            .collect::<Result<_>>()?;
        out.push(WeightedAttachment {
            name: budget.copy(name)?,
            ignore_rotation: value.get("m_bIgnoreRotation").and_then(Value::as_bool) == Some(true),
            influences,
        });
    }
    Ok(())
}

fn influence(
    value: &Value,
    names: &HashMap<String, usize>,
    index: usize,
    budget: &mut Budget,
) -> Result<AttachmentInfluence> {
    let member = |key| value.array(key).get(index);
    let name = member("m_influenceNames")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(AttachmentInfluence {
        bone_name: budget.copy(name)?,
        bone: Some(name)
            .filter(|n| !n.is_empty())
            .and_then(|n| names.get(&n.to_lowercase()).copied()),
        weight: member("m_influenceWeights")
            .and_then(Value::as_f64)
            .unwrap_or(1.0) as f32,
        position: floats(member("m_vInfluenceOffsets"))?,
        rotation: floats(member("m_vInfluenceRotations"))?,
        root_transform: member("m_bInfluenceRootTransform").and_then(Value::as_bool) == Some(true),
    })
}
