//! Authored sequence and clip data for clients that evaluate Source's animation layers.

use std::collections::HashMap;

use super::clip::{Decoder, SourceTracks, block};
use super::sequence::{AnimEvent, MAX_SEQUENCES, frame_rate, frames, read_events};
use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::model::{SourceBone, bone_indices, floats, source_skeleton};
use crate::resource::Resource;

/// The sequences and animation clips embedded in a compiled model.
pub struct AnimationSet {
    /// ASEQ sequence descriptors in file order, including bind poses and blends.
    pub sequences: Vec<LayeredSequence>,
    /// ANIM clip descriptors in file order.
    pub clips: Vec<AnimationInfo>,
    /// ASEQ reference table addressed by blends and automatic layers.
    pub references: Vec<SequenceReference>,
    /// ASEQ pose-parameter table.
    pub pose_parameters: Vec<PoseParameter>,
    /// ASEQ bone-name table resolved against the model skeleton.
    pub bone_references: Vec<BoneReference>,
    /// ASEQ weight lists.
    pub bone_masks: Vec<BoneMask>,
    /// ASEQ named bone scale sets.
    pub bone_scale_sets: Vec<BoneScaleSet>,
    bones: Vec<SourceBone>,
    group: Option<Value>,
    animation: Option<Value>,
}

/// An authored ASEQ sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct LayeredSequence {
    /// Sequence name, used to select it independently of its clips.
    pub name: String,
    /// Flags authored for the sequence.
    pub flags: SequenceFlags,
    /// Activities and modifiers in file order.
    pub activity_entries: Vec<ActivityEntry>,
    /// Authored fade-in duration in seconds.
    pub fade_in: Option<f32>,
    /// Authored fade-out duration in seconds.
    pub fade_out: Option<f32>,
    /// Index into [AnimationSet::bone_masks].
    pub bone_mask: Option<usize>,
    /// The sequence's clip or nested sequence lookup.
    pub blend: BlendGrid,
    /// Ordered sequence layers, including repeated entries.
    pub auto_layers: Vec<AutoLayer>,
    /// Whether the sequence represents the skeleton's bind pose.
    pub bind_pose: bool,
}

/// An ANIM clip's metadata. The clip's flags describe its stored samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationInfo {
    /// ANIM descriptor name.
    pub name: String,
    /// Flags authored for these samples.
    pub flags: AnimationFlags,
    /// Activities and modifiers in file order.
    pub activity_entries: Vec<ActivityEntry>,
    /// Number of stored frames.
    pub frames: u32,
    /// Playback rate in frames per second.
    pub fps: f32,
    /// Events by frame.
    pub events: Vec<AnimEvent>,
}

/// One row of an activity list, including modifiers. A modifier follows the activity it qualifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntry {
    /// Activity or modifier name.
    pub name: String,
    /// Signed weight as stored in the descriptor.
    pub weight: i64,
    /// Numeric activity identifier.
    pub activity: i64,
    /// Raw flags.
    pub flags: i64,
}

impl ActivityEntry {
    /// Whether this row starts a new activity. Other rows are modifier names.
    pub fn is_activity(&self) -> bool {
        self.name.starts_with("ACT_")
    }
}

/// An entry in the local reference table. A name may denote both a clip and a sequence.
/// Blend fetches prefer the clip, and automatic layers select the sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceReference {
    /// Referenced name as stored.
    pub name: String,
    /// Matching ANIM index, if present.
    pub clip: Option<usize>,
    /// Matching ASEQ index, if present.
    pub sequence: Option<usize>,
}

/// A named pose-parameter range. Start and end preserve authored order.
#[derive(Debug, Clone, PartialEq)]
pub struct PoseParameter {
    /// Parameter name.
    pub name: String,
    /// Start of the range.
    pub start: f32,
    /// End of the range.
    pub end: f32,
    /// Wrapping period as authored.
    pub loop_range: f32,
    /// Whether the parameter wraps.
    pub looping: bool,
}

/// The geometry of a sequence's blend space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendDimensions {
    /// A single sample or an empty sequence.
    Single,
    /// A row of samples.
    One,
    /// A two-dimensional grid.
    Two,
    /// A two-dimensional grid using triangular blending.
    Triangular,
}

/// The sequence's samples and their coordinates in local pose-parameter space.
#[derive(Debug, Clone, PartialEq)]
pub struct BlendGrid {
    /// Authored blend mode.
    pub dimensions: BlendDimensions,
    /// Grid width and height.
    pub size: [usize; 2],
    /// Pose-parameter index on each axis.
    pub pose_parameters: [Option<usize>; 2],
    /// Sample coordinates per axis, in sample order.
    pub pose_keys: [Vec<f32>; 2],
    /// Indices into [AnimationSet::references] in file order. Empty cells are preserved.
    pub references: Vec<Option<usize>>,
    /// Whether the fetch calculates pose parameters.
    pub calculate_pose_parameters: bool,
    /// Per-axis fixed blend weights, when enabled.
    pub fixed_blend_weights: Option<[f32; 2]>,
    /// Pose parameter used to drive the cycle, if present.
    pub cycle_pose_parameter: Option<usize>,
    /// Whether cycle-pose evaluation is enabled.
    pub cycle_pose: bool,
    /// Whether the blend uses realtime playback.
    pub realtime: bool,
}

/// An authored automatic layer, in its parent sequence's cycle or pose-parameter domain.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoLayer {
    /// Index into [AnimationSet::references], if present.
    pub reference: Option<usize>,
    /// Index into [AnimationSet::pose_parameters], if present.
    pub pose_parameter: Option<usize>,
    /// Start of the layer.
    pub start: f32,
    /// End of fade-in.
    pub peak: f32,
    /// Start of fade-out.
    pub tail: f32,
    /// End of the layer.
    pub end: f32,
    /// Flags controlling layer evaluation.
    pub flags: AutoLayerFlags,
}

/// A resolved entry of ASEQ's local bone-name table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoneReference {
    /// Authored bone name.
    pub name: String,
    /// Model skeleton index, when the name resolves.
    pub bone: Option<usize>,
}

/// Authored bone and morph weights, including fractional feathering and explicit zeroes.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneMask {
    /// Weight-list name.
    pub name: String,
    /// Pairs of local bone-reference index and weight.
    pub bones: Vec<(usize, f32)>,
    /// Named morph-controller weight overrides.
    pub morphs: Vec<(String, f32)>,
    /// Weight for a morph controller absent from the overrides.
    pub default_morph_weight: f32,
}

/// A named set of bone scales.
#[derive(Debug, Clone, PartialEq)]
pub struct BoneScaleSet {
    /// Scale-set name.
    pub name: String,
    /// Pairs of local bone-reference index and scale.
    pub bones: Vec<(usize, f32)>,
    /// Optional Source-space root offset in inches.
    pub root_offset: Option<[f32; 3]>,
}

macro_rules! flags {
    ($(#[$description:meta])* $type:ident { $($(#[$doc:meta])* $field:ident: $key:literal),+ $(,)? }) => {
        $(#[$description])*
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        pub struct $type {
            $($(#[$doc])* pub $field: bool,)+
        }

        impl $type {
            fn read(value: &Value) -> Self {
                let flags = value.get("m_flags");
                Self { $($field: flags.is_some_and(|value| flag(value, $key)),)+ }
            }
        }
    };
}

flags! {
    /// Sequence flags from ASEQ.
    SequenceFlags {
        /// Plays cyclically.
        looping: "m_bLooping",
        /// Automatically starts at model playback.
        autoplay: "m_bAutoplay",
        /// Hidden sequence.
        hidden: "m_bHidden",
        /// Layers as a delta.
        delta: "m_bLegacyDelta",
        /// Uses legacy world-space blending.
        worldspace: "m_bLegacyWorldspace",
        /// Uses legacy cycle-pose evaluation.
        cycle_pose: "m_bLegacyCyclepose",
        /// Uses legacy realtime playback.
        realtime: "m_bLegacyRealtime",
        /// Authored through ModelDoc.
        model_doc: "m_bModelDoc",
        /// Combines multiple members.
        multi: "m_bMulti",
        /// Evaluates after the base blend.
        post: "m_bPost",
        /// Snaps during transitions.
        snap: "m_bSnap",
    }
}

flags! {
    /// Clip flags from ANIM, independently of its sequence.
    AnimationFlags {
        /// Plays cyclically.
        looping: "m_bLooping",
        /// Hidden clip.
        hidden: "m_bHidden",
        /// Samples encode a legacy delta.
        delta: "m_bDelta",
        /// Samples encode an AnimGraph additive clip.
        additive: "m_bAnimGraphAdditive",
        /// Uses legacy world-space blending.
        worldspace: "m_bLegacyWorldspace",
        /// Clip is flagged as all zeroes.
        all_zeros: "m_bAllZeros",
        /// The implicit sequence ignores the clip's delta flag.
        implicit_sequence_ignores_delta: "m_bImplicitSeqIgnoreDelta",
        /// Authored through ModelDoc.
        model_doc: "m_bModelDoc",
    }
}

flags! {
    /// Flags on an ASEQ automatic layer.
    AutoLayerFlags {
        /// Fetches a frame from the layer.
        fetch_frame: "m_bFetchFrame",
        /// Evaluates the layer locally.
        local: "m_bLocal",
        /// Disables blending.
        no_blend: "m_bNoBlend",
        /// Uses the pose-parameter domain.
        pose: "m_bPose",
        /// Evaluates after the base blend.
        post: "m_bPost",
        /// Uses spline weights.
        spline: "m_bSpline",
        /// Subtracts the layer.
        subtract: "m_bSubtract",
        /// Crossfades the layer.
        crossfade: "m_bXFade",
    }
}

impl AnimationSet {
    /// Reads the embedded ANIM, ASEQ and AGRP blocks of a model. A model with no embedded
    /// animations yields an empty set.
    pub fn read(resource: &Resource) -> Result<Self> {
        let mut budget = Budget::named("animation set");
        let ctrl = resource.kv3_in(b"CTRL", &mut budget).transpose()?;
        let embedded = ctrl
            .as_ref()
            .and_then(|doc| doc.root.get("embedded_animation"));
        let (group, animation, sequence) = match embedded {
            Some(ctrl) => (
                block(resource, ctrl, "group_data_block", &mut budget)?,
                block(resource, ctrl, "anim_data_block", &mut budget)?,
                block(resource, ctrl, "seqgroup_data_block", &mut budget)?,
            ),
            None => (None, None, None),
        };
        let bones = if animation.is_some() || sequence.is_some() {
            let data = resource
                .data_in(&mut budget)
                .ok_or_else(|| malformed("animation", "no model DATA"))??;
            source_skeleton(&data)?
        } else {
            Vec::new()
        };
        let sequence = sequence.as_ref().unwrap_or(&Value::Null);
        let animation_data = animation.as_ref().unwrap_or(&Value::Null);
        let mut events = 0;
        let clips = read_array(
            animation_data,
            "m_animArray",
            &mut budget,
            |clip, budget| animation_info(clip, budget, &mut events),
        )?;
        let sequences = read_array(
            sequence,
            "m_localS1SeqDescArray",
            &mut budget,
            layered_sequence,
        )?;
        let references = references(sequence, &clips, &sequences, &mut budget)?;
        let pose_parameters = read_array(
            sequence,
            "m_localPoseParamArray",
            &mut budget,
            pose_parameter,
        )?;
        Ok(Self {
            clips,
            sequences,
            references,
            pose_parameters,
            bone_references: bone_references(sequence, &bones, &mut budget)?,
            bone_masks: read_array(sequence, "m_localBoneMaskArray", &mut budget, bone_mask)?,
            bone_scale_sets: read_array(
                sequence,
                "m_localScaleSetArray",
                &mut budget,
                bone_scale_set,
            )?,
            bones,
            group,
            animation,
        })
    }

    /// Model skeleton in the same order as the decoded clips' bone tracks.
    pub fn bones(&self) -> &[SourceBone] {
        &self.bones
    }

    /// Decodes one [AnimationSet::clips] index to authored Source-space bone and morph tracks.
    /// Empty channels inherit from the caller's base pose for absolute clips and contribute
    /// no change for layers. An index outside the clip table returns an error.
    pub fn decode_clip(&self, index: usize) -> Result<SourceTracks> {
        let animation = self.animation.as_ref().unwrap_or(&Value::Null);
        let clip = animation
            .array("m_animArray")
            .get(index)
            .ok_or_else(|| malformed("animation", format!("no clip {index}")))?;
        let group = self.group.as_ref().unwrap_or(&Value::Null);
        Decoder::new(animation, group, &self.bones, true)?.source_tracks(clip)
    }
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool) == Some(true)
}

fn name(value: &Value, key: &str, budget: &mut Budget) -> Result<String> {
    budget.copy(value.str_field(key).unwrap_or_default())
}

fn read_array<T>(
    value: &Value,
    key: &str,
    budget: &mut Budget,
    mut read: impl FnMut(&Value, &mut Budget) -> Result<T>,
) -> Result<Vec<T>> {
    let values = value.array(key);
    if values.len() > MAX_SEQUENCES {
        return Err(malformed("animation", format!("too many {key} entries")));
    }
    budget.slots::<T>(values.len())?;
    values.iter().map(|value| read(value, budget)).collect()
}

fn animation_info(value: &Value, budget: &mut Budget, events: &mut usize) -> Result<AnimationInfo> {
    Ok(AnimationInfo {
        name: name(value, "m_name", budget)?,
        flags: AnimationFlags::read(value),
        activity_entries: activity_entries(value, budget)?,
        frames: frames(value),
        fps: frame_rate(value),
        events: read_events(value, events)?,
    })
}

fn activity_entries(value: &Value, budget: &mut Budget) -> Result<Vec<ActivityEntry>> {
    read_array(value, "m_activityArray", budget, |entry, budget| {
        let int = |key| entry.get(key).and_then(Value::as_i64).unwrap_or(0);
        Ok(ActivityEntry {
            name: name(entry, "m_name", budget)?,
            weight: entry.get("m_nWeight").and_then(Value::as_i64).unwrap_or(1),
            activity: int("m_nActivity"),
            flags: int("m_nFlags"),
        })
    })
}

fn layered_sequence(value: &Value, budget: &mut Budget) -> Result<LayeredSequence> {
    let transition = value.get("m_transition").unwrap_or(&Value::Null);
    Ok(LayeredSequence {
        name: name(value, "m_sName", budget)?,
        flags: SequenceFlags::read(value),
        activity_entries: activity_entries(value, budget)?,
        fade_in: transition.f32_field("m_flFadeInTime"),
        fade_out: transition.f32_field("m_flFadeOutTime"),
        bone_mask: value.usize_field("m_nLocalWeightlist"),
        blend: blend_grid(value.get("m_fetch").unwrap_or(&Value::Null), budget)?,
        auto_layers: read_array(value, "m_autoLayerArray", budget, |layer, _| {
            Ok(auto_layer(layer))
        })?,
        bind_pose: value
            .get("m_SequenceKeys")
            .is_some_and(|keys| flag(keys, "bind_pose")),
    })
}

fn blend_grid(value: &Value, budget: &mut Budget) -> Result<BlendGrid> {
    let flags = value.get("m_flags").unwrap_or(&Value::Null);
    let dimensions = if flag(flags, "m_b2D_TRI") {
        BlendDimensions::Triangular
    } else if flag(flags, "m_b2D") {
        BlendDimensions::Two
    } else if flag(flags, "m_b1D") {
        BlendDimensions::One
    } else {
        BlendDimensions::Single
    };
    let index = |key, axis| value.array(key).get(axis).and_then(Value::as_usize);
    let keys = |value: &Value, _: &mut Budget| Ok(value.as_f64().unwrap_or_default() as f32);
    Ok(BlendGrid {
        dimensions,
        size: [0, 1].map(|axis| index("m_nGroupSize", axis).unwrap_or(0)),
        pose_parameters: [0, 1].map(|axis| index("m_nLocalPose", axis)),
        pose_keys: [
            read_array(value, "m_poseKeyArray0", budget, keys)?,
            read_array(value, "m_poseKeyArray1", budget, keys)?,
        ],
        references: read_array(value, "m_localReferenceArray", budget, |entry, _| {
            Ok(entry.as_usize())
        })?,
        calculate_pose_parameters: flag(value, "m_bCalculatePoseParameters"),
        fixed_blend_weights: flag(value, "m_bFixedBlendWeight")
            .then(|| floats(value.get("m_flFixedBlendWeightVals")))
            .transpose()?,
        cycle_pose_parameter: value.usize_field("m_nLocalCyclePoseParameter"),
        cycle_pose: flag(flags, "m_bCylepose") || flag(flags, "m_bCyclepose"),
        realtime: flag(flags, "m_bRealtime"),
    })
}

fn auto_layer(value: &Value) -> AutoLayer {
    let cycle = |key| value.f32_field(key).unwrap_or_default();
    AutoLayer {
        reference: value.usize_field("m_nLocalReference"),
        pose_parameter: value.usize_field("m_nLocalPose"),
        start: cycle("m_start"),
        peak: cycle("m_peak"),
        tail: cycle("m_tail"),
        end: cycle("m_end"),
        flags: AutoLayerFlags::read(value),
    }
}

fn pose_parameter(value: &Value, budget: &mut Budget) -> Result<PoseParameter> {
    Ok(PoseParameter {
        name: name(value, "m_sName", budget)?,
        start: value.f32_field("m_flStart").unwrap_or_default(),
        end: value.f32_field("m_flEnd").unwrap_or_default(),
        loop_range: value.f32_field("m_flLoop").unwrap_or_default(),
        looping: flag(value, "m_bLooping"),
    })
}

fn references(
    value: &Value,
    clips: &[AnimationInfo],
    sequences: &[LayeredSequence],
    budget: &mut Budget,
) -> Result<Vec<SequenceReference>> {
    let clips: HashMap<_, _> = clips
        .iter()
        .enumerate()
        .map(|(i, clip)| (clip.name.as_str(), i))
        .collect();
    let sequences: HashMap<_, _> = sequences
        .iter()
        .enumerate()
        .map(|(i, seq)| (seq.name.as_str(), i))
        .collect();
    read_array(
        value,
        "m_localSequenceNameArray",
        budget,
        |entry, budget| {
            let name = entry.as_str().unwrap_or_default();
            Ok(SequenceReference {
                name: budget.copy(name)?,
                clip: clips.get(name).copied(),
                sequence: sequences.get(name).copied(),
            })
        },
    )
}

fn bone_references(
    value: &Value,
    bones: &[SourceBone],
    budget: &mut Budget,
) -> Result<Vec<BoneReference>> {
    let indices = bone_indices(bones);
    read_array(value, "m_localBoneNameArray", budget, |bone, budget| {
        let name = bone.as_str().unwrap_or_default();
        Ok(BoneReference {
            name: budget.copy(name)?,
            bone: indices.get(&name.to_lowercase()).copied(),
        })
    })
}

fn bone_mask(value: &Value, budget: &mut Budget) -> Result<BoneMask> {
    Ok(BoneMask {
        name: name(value, "m_sName", budget)?,
        bones: bone_values(value, "m_flBoneWeightArray", budget)?,
        morphs: read_array(value, "m_morphCtrlWeightArray", budget, morph_weight)?,
        default_morph_weight: value.f32_field("m_flDefaultMorphCtrlWeight").unwrap_or(1.0),
    })
}

fn morph_weight(value: &Value, budget: &mut Budget) -> Result<(String, f32)> {
    let [name, weight] = value.slice() else {
        return Err(malformed(
            "animation",
            "a morph weight without its name and value",
        ));
    };
    Ok((
        budget.copy(name.as_str().unwrap_or_default())?,
        weight.as_f64().unwrap_or_default() as f32,
    ))
}

fn bone_scale_set(value: &Value, budget: &mut Budget) -> Result<BoneScaleSet> {
    Ok(BoneScaleSet {
        name: name(value, "m_sName", budget)?,
        bones: bone_values(value, "m_flBoneScaleArray", budget)?,
        root_offset: flag(value, "m_bRootOffset")
            .then(|| floats(value.get("m_vRootOffset")))
            .transpose()?,
    })
}

fn bone_values(value: &Value, key: &str, budget: &mut Budget) -> Result<Vec<(usize, f32)>> {
    let weights = value.array(key);
    let bones = value.array("m_nLocalBoneArray");
    if bones.len() != weights.len() {
        return Err(malformed(
            "animation",
            format!("a {key} count unequal to its bones"),
        ));
    }
    budget.slots::<(usize, f32)>(bones.len())?;
    bones
        .iter()
        .zip(weights)
        .map(|(bone, weight)| {
            let index = bone
                .as_usize()
                .ok_or_else(|| malformed("animation", "invalid local bone index"))?;
            Ok((index, weight.as_f64().unwrap_or_default() as f32))
        })
        .collect()
}
