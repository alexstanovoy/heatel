//! The clips of a model's embedded animation (`CTRL.embedded_animation`: the `ANIM` block's
//! compressed frames, the `AGRP` block's decode key, the `ASEQ` block's sequences), sampled
//! per frame into glTF keyframes as Source2Viewer 20 writes them.

use std::collections::{HashMap, HashSet};

use super::segment::{Attribute, BonePose, Segment};
use crate::budget::Budget as MemBudget;
use crate::coords::{bone_to_gltf, finite, multiply, rotate};
use crate::error::{Result, malformed};
use crate::kv3::{self, Value};
use crate::model::{SourceBone, bone_indices, floats, source_skeleton};
use crate::resource::Resource;

/// Most bone-frames one call decodes (each becomes up to three keyframes); the largest
/// model we know (a destruction animation) takes 41 k.
const MAX_BONE_FRAMES: usize = 1 << 21;
/// Most frames one clip may claim, even on a skeleton with no bones.
const MAX_FRAMES: usize = 1 << 20;
/// Most element reads, frame-block visits and segment bones one call goes through.
const MAX_WORK: usize = 1 << 28;
/// Most bone entries the decode key's channels may take (four channels of 150 bones here).
const MAX_REMAP: usize = 1 << 20;
/// Frame rates outside this are not a clip's (zero or less plays at one frame a second).
const MAX_FPS: f32 = 10_000.0;
const MIN_FPS: f32 = 1e-3;

/// An animation clip on a model's skeleton in glTF coordinates.
pub struct Clip {
    /// The clip's name in the model (`attack`, `idle`).
    pub name: String,
    /// One per bone, in skeleton order.
    pub tracks: Vec<Track>,
}

/// A bone's keyframes in the coordinate frame of its reader. A run of equal samples
/// keeps both endpoints, including the clip's final frame when a pose holds to the end.
pub struct Track {
    /// Rotations as `[x, y, z, w]` quaternions.
    pub rotation: Keys<4>,
    /// Translations.
    pub translation: Keys<3>,
    /// Scales.
    pub scale: Keys<3>,
}

/// Keyframes of one channel: a time and an `N`-component value each.
#[derive(Default)]
pub struct Keys<const N: usize> {
    /// Seconds, increasing.
    pub times: Vec<f32>,
    /// One value per time.
    pub values: Vec<[f32; N]>,
}

/// Tracks decoded from an ANIM clip without applying a sequence's blend rules.
pub struct SourceTracks {
    /// Source-space tracks in model skeleton order, using inches and XYZW quaternions.
    /// Empty channels have no authored samples.
    pub bones: Vec<Track>,
    /// Authored morph-controller tracks.
    pub morphs: Vec<MorphTrack>,
    /// Authored root-motion segments, separate from the bone tracks.
    pub movements: Vec<RootMovement>,
}

/// Keyframes for a named morph controller.
pub struct MorphTrack {
    /// Controller name from the animation group's decode key.
    pub name: String,
    /// The controller's scalar keyframes.
    pub keys: Keys<1>,
}

impl Clip {
    /// The time of its last keyframe.
    pub fn duration(&self) -> f32 {
        let last = |times: &[f32]| times.last().copied().unwrap_or(0.0);
        self.tracks
            .iter()
            .map(|t| {
                last(&t.rotation.times)
                    .max(last(&t.translation.times))
                    .max(last(&t.scale.times))
            })
            .fold(0.0, f32::max)
    }
}

/// The clips called `names` of the model `resource`, in the order the model lists them (its
/// sequences, then the animations no sequence names). A name the model lacks, and an additive
/// clip (deltas over a pose, which glTF has no way to say), is left out.
pub fn read_clips(resource: &Resource, names: &[&str]) -> Result<Vec<Clip>> {
    read_clips_in_place(resource, names, &[])
}

/// Reads clips with selected activities in place. Only the explicit movement trajectory
/// is omitted; the authored bone pose, including root sway and vertical motion, is retained.
pub fn read_clips_in_place(
    resource: &Resource,
    names: &[&str],
    activities: &[&str],
) -> Result<Vec<Clip>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let mut mem_budget = MemBudget::named("animation");
    let ctrl = resource
        .kv3_in(b"CTRL", &mut mem_budget)
        .ok_or_else(|| malformed("animation", "no CTRL"))??
        .root;
    if ctrl.get("embedded_animation").is_none() {
        return Ok(Vec::new());
    }
    let data = resource
        .data_in(&mut mem_budget)
        .ok_or_else(|| malformed("animation", "no DATA"))??;
    read_clips_with_ctrl(resource, &data, &ctrl, names, activities, &mut mem_budget)
}

/// Like [`read_clips_in_place`], reusing already-decoded `DATA` and `CTRL` blocks and charging
/// animation blocks to `mem_budget`.
pub(crate) fn read_clips_with_ctrl(
    resource: &Resource,
    data: &Value,
    ctrl: &Value,
    names: &[&str],
    activities: &[&str],
    mem_budget: &mut MemBudget,
) -> Result<Vec<Clip>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let Some(embedded) = ctrl.get("embedded_animation") else {
        return Ok(Vec::new());
    };
    let bones = source_skeleton(data, mem_budget)?;
    let group = block(resource, embedded, "group_data_block", mem_budget)?
        .ok_or_else(|| malformed("animation", "no group block"))?;
    let anim = block(resource, embedded, "anim_data_block", mem_budget)?
        .ok_or_else(|| malformed("animation", "no animation block"))?;
    let sequences = block(resource, embedded, "seqgroup_data_block", mem_budget)?;
    let mut decoder = Decoder::new(&anim, &group, &bones, false)?;
    let descs = anim.array("m_animArray");
    let mut clips: Vec<Clip> = Vec::new();
    for (name, desc, sequence) in listing(sequences.as_ref(), descs) {
        if !names.contains(&name) || clips.iter().any(|c| c.name == name) {
            continue;
        }
        let flag = |v: Option<&Value>, key| {
            v.and_then(|v| v.get("m_flags")?.get(key)?.as_bool()) == Some(true)
        };
        let additive = flag(Some(desc), "m_bDelta")
            || flag(Some(desc), "m_bAnimGraphAdditive")
            || flag(sequence, "m_bLegacyDelta");
        if !additive {
            let in_place = in_place_for(
                activities,
                sequence.unwrap_or(desc).get("m_activityArray"),
                &mut decoder.budget,
            )?;
            let tracks = decoder
                .decode(
                    desc,
                    Output::Gltf {
                        trajectory: !in_place,
                    },
                    mem_budget,
                )?
                .bones;
            clips.push(Clip {
                name: name.to_owned(),
                tracks,
            });
        }
    }
    Ok(clips)
}

fn in_place_for(activities: &[&str], entries: Option<&Value>, budget: &mut Budget) -> Result<bool> {
    if activities.is_empty() {
        return Ok(false);
    }
    let names = entries.map_or(&[][..], Value::slice);
    Budget::spend(&mut budget.work, names.len(), "activity entries")?;
    if !names
        .iter()
        .filter_map(|entry| entry.get("m_name")?.as_str())
        .any(|name| name.starts_with("ACT_") && activities.contains(&name))
    {
        return Ok(false);
    }
    // Unselected activities have no bearing on this export. Selected ones retain the
    // bounded parser's validation, rather than treating an oversized list as authoritative.
    super::sequence::read_activities(entries)?;
    Ok(true)
}

#[cfg(test)]
mod in_place_tests {
    use super::*;

    fn entries(name: &str) -> Value {
        Value::Array(
            (0..65)
                .map(|_| {
                    let mut object = crate::kv3::Object::with_capacity(1);
                    object.push("m_name".into(), Value::String(name.into()));
                    Value::Object(object)
                })
                .collect(),
        )
    }

    #[test]
    fn morph_decode_buffers_and_names_refuse_a_tight_memory_budget() {
        let group = Value::object([(
            "m_decodeKey",
            Value::object([(
                "m_dataChannelArray",
                Value::from_array([Value::object([
                    ("m_szVariableName", Value::from("data")),
                    ("m_szChannelClass", Value::from("MorphChannel")),
                    ("m_szElementNameArray", Value::from(["smile"])),
                    ("m_nElementIndexArray", Value::from([0])),
                ])]),
            )]),
        )]);
        let animation = Value::Null;
        let desc = Value::object([("m_pData", Value::object([("m_nFrames", 0)]))]);
        let mut decoder = Decoder::new(&animation, &group, &[], true).unwrap();
        assert_eq!(decoder.morph_names, ["smile"]);
        let buffers =
            size_of::<Option<f32>>() + size_of::<KeyWriter<1>>() + size_of::<MorphTrack>();
        let required = buffers + "smile".len() + 16;
        for (limit, charge) in [
            (0, size_of::<Option<f32>>()),
            (size_of::<Option<f32>>(), size_of::<KeyWriter<1>>()),
            (buffers - 1, size_of::<MorphTrack>()),
            (buffers, "smile".len() + 16),
            (required - 1, "smile".len() + 16),
        ] {
            let mut memory = MemBudget::new(limit);
            let error = match decoder.source_tracks(&desc, &mut memory) {
                Err(error) => error,
                Ok(_) => panic!("morph buffers exceeded their memory budget"),
            };
            assert_eq!(
                error.to_string(),
                format!("reading: takes {charge} bytes more than it may")
            );
        }
        let mut memory = MemBudget::new(required);
        let tracks = decoder.source_tracks(&desc, &mut memory).unwrap();
        assert!(tracks.morphs.is_empty());
        assert_eq!(memory.spent(), required);
    }

    #[test]
    fn a_gltf_trajectory_keeps_child_transforms_local() {
        let pose = BonePose {
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: 1.0,
            authored: 0,
        };
        let output = Output::Gltf { trajectory: true };
        let motion = Some(([10.0, 0.0, 0.0], 90.0));
        let mut child = TrackWriter::default();
        child
            .submit(0.0, 0.0, &pose, false, motion, output)
            .unwrap();
        let track = child.finish(0.0);
        assert_eq!(track.translation.values, [pose.position]);
        assert_eq!(track.rotation.values, [pose.rotation]);
    }

    #[test]
    fn unopted_activities_do_not_reject_a_clip_with_many_activity_entries() {
        let mut budget = Budget {
            bone_frames: MAX_BONE_FRAMES,
            work: MAX_WORK,
        };
        assert!(
            !in_place_for(
                &["ACT_DOTA_RUN"],
                Some(&entries("ACT_DOTA_IDLE")),
                &mut budget
            )
            .unwrap()
        );
    }

    #[test]
    fn opted_activities_keep_the_activity_entry_bound() {
        let mut budget = Budget {
            bone_frames: MAX_BONE_FRAMES,
            work: MAX_WORK,
        };
        assert!(
            in_place_for(
                &["ACT_DOTA_RUN"],
                Some(&entries("ACT_DOTA_RUN")),
                &mut budget
            )
            .is_err()
        );
    }
}

/// The KV3 block `CTRL.embedded_animation.<key>` addresses; none for an index of zero or less.
pub(super) fn block(
    resource: &Resource,
    embedded: &Value,
    key: &str,
    budget: &mut MemBudget,
) -> Result<Option<Value>> {
    let Some(block) = embedded
        .usize_field(key)
        .filter(|&i| i > 0)
        .and_then(|i| resource.blocks().get(i))
    else {
        return Ok(None);
    };
    Ok(Some(kv3::decode_in(resource.bytes(block)?, budget)?.root))
}

/// A decode-key channel: what it animates (`None`: nothing a skeleton shows) and, per bone,
/// the bone's element id in it.
struct Channel {
    attribute: Option<Attribute>,
    remap: Vec<Option<i64>>,
}

/// The decode key's channels; elements find their bone by name, ignoring case.
fn channels<'a>(
    key: Option<&'a Value>,
    bones: &[SourceBone],
    morph_names: Option<&mut Vec<&'a str>>,
) -> Result<Vec<Channel>> {
    let key = key.ok_or_else(|| malformed("animation", "no decode key"))?;
    let raw_channels = key.array("m_dataChannelArray");
    if raw_channels.len().saturating_mul(bones.len()) > MAX_REMAP {
        return Err(malformed(
            "animation",
            format!("{} channels of {} bones", raw_channels.len(), bones.len()),
        ));
    }
    let by_name = bone_indices(bones);
    let mut out = Vec::new();
    let mut morph_names = morph_names;
    let mut morph_budget = MAX_REMAP;
    for channel in raw_channels {
        let attribute = channel
            .str_field("m_szVariableName")
            .and_then(Attribute::from_name);
        let names = channel.array("m_szElementNameArray");
        let ids = channel.array("m_nElementIndexArray");
        let remap = match attribute {
            Some(Attribute::Data)
                if channel.str_field("m_szChannelClass") == Some("MorphChannel") =>
            {
                match &mut morph_names {
                    Some(morph_names) => morph_remap(names, ids, morph_names, &mut morph_budget)?,
                    None => Vec::new(),
                }
            }
            Some(Attribute::Position | Attribute::Angle | Attribute::Scale) => {
                let mut remap = vec![None; bones.len()];
                for (name, id) in names.iter().zip(ids) {
                    if let Some(&bone) = name.as_str().and_then(|n| by_name.get(&n.to_lowercase()))
                    {
                        remap[bone] = id.as_i64();
                    }
                }
                remap
            }
            _ => Vec::new(),
        };
        out.push(Channel { attribute, remap });
    }
    Ok(out)
}

fn morph_remap<'a>(
    names: &'a [Value],
    ids: &[Value],
    morph_names: &mut Vec<&'a str>,
    budget: &mut usize,
) -> Result<Vec<Option<i64>>> {
    let count = names.len().min(ids.len());
    Budget::spend(
        budget,
        morph_names.len().saturating_add(count),
        "morph remaps",
    )?;
    let mut remap = vec![None; morph_names.len()];
    for (name, id) in names.iter().zip(ids) {
        morph_names.push(name.as_str().unwrap_or_default());
        remap.push(id.as_i64());
    }
    Ok(remap)
}

/// The model's clips as Source2Viewer lists them: per sequence (`ASEQ`), its name and the
/// animation its first fetch reference names; then every animation no sequence took the name
/// of. Without sequences, the animations themselves.
pub(super) fn listing<'a>(
    sequences: Option<&'a Value>,
    descs: &'a [Value],
) -> Vec<(&'a str, &'a Value, Option<&'a Value>)> {
    let mut out = Vec::new();
    let mut taken = HashSet::new();
    if let Some(sequences) = sequences {
        let by_name: HashMap<&str, &Value> = descs
            .iter()
            .map(|d| (d.str_field("m_name").unwrap_or_default(), d))
            .collect();
        let referenced = sequences.array("m_localSequenceNameArray");
        for sequence in sequences.array("m_localS1SeqDescArray") {
            let first = sequence
                .get("m_fetch")
                .and_then(|f| f.array("m_localReferenceArray").first())
                .and_then(Value::as_usize)
                .and_then(|i| referenced.get(i))
                .and_then(Value::as_str);
            if let Some(&desc) = first.and_then(|f| by_name.get(f)) {
                let own = sequence.str_field("m_sName").unwrap_or_default();
                taken.insert(own);
                out.push((own, desc, Some(sequence)));
            }
        }
    }
    for desc in descs {
        let name = desc.str_field("m_name").unwrap_or_default();
        if !taken.contains(name) {
            out.push((name, desc, None));
        }
    }
    out
}

/// What decoding may still spend.
struct Budget {
    bone_frames: usize,
    work: usize,
}

impl Budget {
    fn spend(left: &mut usize, cost: usize, what: &str) -> Result<()> {
        *left = left
            .checked_sub(cost)
            .ok_or_else(|| malformed("animation", format!("more {what} than a model has")))?;
        Ok(())
    }
}

/// The animation block's segments, parsed when a clip first uses them.
struct Segments<'a> {
    list: &'a [Value],
    codecs: Vec<&'a str>,
    channels: Vec<Channel>,
    built: HashMap<usize, Option<Segment<'a>>>,
}

impl<'a> Segments<'a> {
    fn get(&mut self, i: usize, budget: &mut Budget) -> Result<Option<&Segment<'a>>> {
        if !self.built.contains_key(&i) {
            let desc = self
                .list
                .get(i)
                .ok_or_else(|| malformed("animation", format!("no segment {i}")))?;
            let container = desc
                .get("m_container")
                .and_then(Value::as_blob)
                .ok_or_else(|| malformed("animation", format!("segment {i} has no data")))?;
            let channel = desc
                .get("m_nLocalChannel")
                .and_then(Value::as_u64)
                .and_then(|c| self.channels.get(usize::try_from(c).ok()?))
                .ok_or_else(|| malformed("animation", format!("segment {i} has no channel")))?;
            // Parsing looks at every element and every bone.
            let cost = container.len() / 2 + channel.remap.len();
            Budget::spend(&mut budget.work, cost, "segment bones")?;
            let segment =
                Segment::parse(container, &self.codecs, channel.attribute, &channel.remap)?;
            self.built.insert(i, segment);
        }
        Ok(self.built.get(&i).and_then(Option::as_ref))
    }
}

/// A run of frames and the segments that hold them (`m_frameblockArray`), ends included.
struct FrameBlock {
    start: usize,
    end: usize,
    segments: Vec<usize>,
}

/// Root motion up to a frame, in Source coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct RootMovement {
    /// Last frame of the movement segment.
    pub end_frame: i64,
    /// Cumulative yaw in degrees.
    pub angle: f32,
    /// Cumulative position in inches.
    pub position: [f32; 3],
    /// Authored motion flags.
    pub flags: i64,
    /// Direction vector.
    pub direction: Option<[f32; 3]>,
    /// Authored starting velocity.
    pub initial_velocity: Option<f32>,
    /// Authored ending velocity.
    pub final_velocity: Option<f32>,
}

#[derive(Clone, Copy)]
enum Output {
    Gltf { trajectory: bool },
    Source,
}

pub(super) struct Decoder<'a> {
    bones: &'a [SourceBone],
    segments: Segments<'a>,
    morph_names: Vec<&'a str>,
    budget: Budget,
}

impl<'a> Decoder<'a> {
    pub(super) fn new(
        animation: &'a Value,
        group: &'a Value,
        bones: &'a [SourceBone],
        morphs: bool,
    ) -> Result<Self> {
        let mut morph_names = Vec::new();
        let channels = channels(
            group.get("m_decodeKey"),
            bones,
            morphs.then_some(&mut morph_names),
        )?;
        Ok(Self {
            bones,
            segments: Segments {
                list: animation.array("m_segmentArray"),
                codecs: animation
                    .array("m_decoderArray")
                    .iter()
                    .map(|decoder| decoder.str_field("m_szName").unwrap_or_default())
                    .collect(),
                channels,
                built: HashMap::new(),
            },
            morph_names,
            budget: Budget {
                bone_frames: MAX_BONE_FRAMES,
                work: MAX_WORK,
            },
        })
    }

    pub(super) fn source_tracks(
        &mut self,
        desc: &Value,
        memory: &mut MemBudget,
    ) -> Result<SourceTracks> {
        self.decode(desc, Output::Source, memory)
    }

    fn decode(
        &mut self,
        desc: &Value,
        output: Output,
        memory: &mut MemBudget,
    ) -> Result<SourceTracks> {
        let data = desc.get("m_pData");
        let frames = frame_count(data)?;
        self.spend_frames(frames)?;
        let fps = frame_rate(desc)?;
        let blocks = frame_blocks(data, &mut self.budget)?;
        let movements = if matches!(output, Output::Gltf { trajectory: false }) {
            Vec::new()
        } else {
            movements(desc, &mut self.budget, matches!(output, Output::Source))?
        };
        self.charge_buffers(memory)?;
        let mut pose: Vec<_> = self.bones.iter().map(rest_pose).collect();
        let mut morphs = vec![None; self.morph_names.len()];
        let mut writers: Vec<TrackWriter> =
            self.bones.iter().map(|_| TrackWriter::default()).collect();
        let mut morph_writers: Vec<_> = self
            .morph_names
            .iter()
            .map(|_| KeyWriter::default())
            .collect();
        for frame in 0..frames {
            self.read_frame(&blocks, frame, &mut pose, &mut morphs)?;
            let time = frame as f32 / fps;
            let previous = (frame as f32 - 1.0) / fps;
            let motion =
                if matches!(output, Output::Gltf { trajectory: true }) && !movements.is_empty() {
                    Budget::spend(&mut self.budget.work, movements.len(), "movements")?;
                    Some(root_motion(&movements, frame))
                } else {
                    None
                };
            for ((bone, now), writer) in self.bones.iter().zip(&pose).zip(&mut writers) {
                writer.submit(time, previous, now, bone.parent.is_none(), motion, output)?;
            }
            for (writer, value) in morph_writers.iter_mut().zip(&morphs) {
                if let Some(value) = value {
                    writer.submit(time, previous, [finite(*value)])?;
                }
            }
        }
        if frames == 0 && matches!(output, Output::Gltf { .. }) {
            for ((bone, now), writer) in self.bones.iter().zip(&pose).zip(&mut writers) {
                writer.submit(0.0, 0.0, now, bone.parent.is_none(), None, output)?;
            }
        }
        let end = frames.saturating_sub(1) as f32 / fps;
        Ok(finish_tracks(
            writers,
            &self.morph_names,
            morph_writers,
            movements,
            end,
        ))
    }

    fn spend_frames(&mut self, frames: usize) -> Result<()> {
        let channels = self
            .bones
            .len()
            .saturating_add(self.morph_names.len())
            .max(1);
        Budget::spend(
            &mut self.budget.bone_frames,
            frames.max(1).saturating_mul(channels),
            "frames",
        )
    }

    fn charge_buffers(&self, memory: &mut MemBudget) -> Result<()> {
        memory.slots::<BonePose>(self.bones.len())?;
        memory.slots::<TrackWriter>(self.bones.len())?;
        memory.slots::<Track>(self.bones.len())?;
        memory.slots::<Option<f32>>(self.morph_names.len())?;
        memory.slots::<KeyWriter<1>>(self.morph_names.len())?;
        memory.slots::<MorphTrack>(self.morph_names.len())?;
        for name in &self.morph_names {
            memory.string_of(name.len())?;
        }
        Ok(())
    }

    fn read_frame(
        &mut self,
        blocks: &[FrameBlock],
        frame: usize,
        pose: &mut [BonePose],
        morphs: &mut [Option<f32>],
    ) -> Result<()> {
        Budget::spend(&mut self.budget.work, blocks.len().max(1), "frame blocks")?;
        for block in blocks
            .iter()
            .filter(|block| (block.start..=block.end).contains(&frame))
        {
            Budget::spend(
                &mut self.budget.work,
                block.segments.len(),
                "segment visits",
            )?;
            for &index in &block.segments {
                if let Some(segment) = self.segments.get(index, &mut self.budget)? {
                    Budget::spend(&mut self.budget.work, segment.work(), "segment reads")?;
                    segment.read(frame - block.start, pose, morphs)?;
                }
            }
        }
        Ok(())
    }
}

fn rest_pose(bone: &SourceBone) -> BonePose {
    BonePose {
        position: bone.position,
        rotation: bone.rotation,
        scale: bone.scale,
        authored: 0,
    }
}

fn finish_tracks(
    writers: Vec<TrackWriter>,
    morph_names: &[&str],
    morph_writers: Vec<KeyWriter<1>>,
    movements: Vec<RootMovement>,
    end: f32,
) -> SourceTracks {
    let bones = writers
        .into_iter()
        .map(|writer| writer.finish(end))
        .collect();
    let morphs = morph_names
        .iter()
        .zip(morph_writers)
        .filter_map(|(name, writer)| {
            writer.last.map(|_| MorphTrack {
                name: (*name).to_owned(),
                keys: writer.finish(end),
            })
        })
        .collect();
    SourceTracks {
        bones,
        morphs,
        movements,
    }
}

fn frame_count(data: Option<&Value>) -> Result<usize> {
    let frames = data
        .and_then(|data| data.usize_field("m_nFrames"))
        .ok_or_else(|| malformed("animation", "no frame count"))?;
    if frames > MAX_FRAMES {
        return Err(malformed(
            "animation",
            format!("{frames} frames, more than {MAX_FRAMES}"),
        ));
    }
    Ok(frames)
}

/// A bone's local transform in glTF's frame: the decoded pose, moved by the root motion on a
/// root bone. Non-finite values (only a damaged file has them) become zero.
fn local(pose: &BonePose, motion: Option<([f32; 3], f32)>, root: bool) -> Keyframe {
    let (mut position, mut rotation) = (pose.position, pose.rotation);
    if let Some((offset, degrees)) = motion {
        let half = degrees.to_radians() * 0.5;
        let yaw = [0.0, 0.0, half.sin(), half.cos()];
        let moved = rotate(yaw, position);
        position = [0, 1, 2].map(|i| moved[i] + offset[i]);
        rotation = multiply(yaw, rotation);
    }
    let (translation, rotation) = bone_to_gltf(position, rotation, root);
    (translation, rotation, [finite(pose.scale); 3])
}

type Keyframe = ([f32; 3], [f32; 4], [f32; 3]);

/// Frames a second; zero or less plays at one, as Source2Viewer does.
fn frame_rate(desc: &Value) -> Result<f32> {
    let fps = desc.get("fps").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    if fps <= 0.0 {
        return Ok(1.0);
    }
    if !(MIN_FPS..=MAX_FPS).contains(&fps) {
        return Err(malformed("animation", format!("{fps} frames a second")));
    }
    Ok(fps)
}

fn frame_blocks(data: Option<&Value>, budget: &mut Budget) -> Result<Vec<FrameBlock>> {
    let list = data.map_or(&[][..], |d| d.array("m_frameblockArray"));
    Budget::spend(&mut budget.work, list.len(), "frame blocks")?;
    list.iter()
        .map(|b| {
            let raw_segments = b.array("m_segmentIndexArray");
            Budget::spend(&mut budget.work, raw_segments.len(), "frame block segments")?;
            let segments = raw_segments
                .iter()
                .map(Value::as_usize)
                .collect::<Option<Vec<_>>>();
            match (
                b.usize_field("m_nStartFrame"),
                b.usize_field("m_nEndFrame"),
                segments,
            ) {
                (Some(start), Some(end), Some(segments)) => Ok(FrameBlock {
                    start,
                    end,
                    segments,
                }),
                _ => Err(malformed("animation", "a frame block without its range")),
            }
        })
        .collect()
}

fn movements(desc: &Value, budget: &mut Budget, source: bool) -> Result<Vec<RootMovement>> {
    let list = desc.array("m_movementArray");
    Budget::spend(&mut budget.work, list.len(), "movements")?;
    list.iter()
        .map(|m| {
            let end = m.get("endframe").and_then(Value::as_i64);
            let angle = m.get("angle").and_then(Value::as_f64);
            match (end, angle, floats::<3>(m.get("position"))) {
                (Some(end_frame), Some(angle), Ok(position)) => Ok(RootMovement {
                    end_frame,
                    angle: angle as f32,
                    position,
                    flags: m
                        .get("motionflags")
                        .and_then(Value::as_i64)
                        .unwrap_or_default(),
                    direction: m
                        .get("vector")
                        .filter(|_| source)
                        .map(|v| floats(Some(v)))
                        .transpose()?,
                    initial_velocity: m.f32_field("v0"),
                    final_velocity: m.f32_field("v1"),
                }),
                _ => Err(malformed(
                    "animation",
                    "a movement without its end or offset",
                )),
            }
        })
        .collect()
}

/// The root's offset (planar) and yaw in degrees at `frame`: interpolated from the previous
/// movement's towards the one the frame is in, by the frame over that movement's end frame,
/// as Source2Viewer 20 does.
fn root_motion(movements: &[RootMovement], frame: usize) -> ([f32; 3], f32) {
    let frame = frame as i64;
    let index = movements
        .iter()
        .position(|m| m.end_frame > frame)
        .unwrap_or(movements.len() - 1);
    let next = &movements[index];
    let (from, from_angle) = match index.checked_sub(1) {
        Some(i) => (movements[i].position, movements[i].angle),
        None => ([0.0; 3], 0.0),
    };
    let t = if next.end_frame > 0 {
        frame as f32 / next.end_frame as f32
    } else {
        1.0
    };
    let lerp = |a: f32, b: f32| a * (1.0 - t) + b * t;
    let position = [
        lerp(from[0], next.position[0]),
        lerp(from[1], next.position[1]),
        0.0,
    ];
    (position, lerp(from_angle, next.angle))
}

#[derive(Default)]
struct TrackWriter {
    rotation: KeyWriter<4>,
    translation: KeyWriter<3>,
    scale: KeyWriter<3>,
}

impl TrackWriter {
    fn submit(
        &mut self,
        time: f32,
        previous: f32,
        pose: &BonePose,
        root: bool,
        motion: Option<([f32; 3], f32)>,
        output: Output,
    ) -> Result<()> {
        match output {
            Output::Gltf { .. } => {
                let (translation, rotation, scale) = local(pose, motion.filter(|_| root), root);
                self.rotation.submit(time, previous, rotation)?;
                self.translation.submit(time, previous, translation)?;
                self.scale.submit(time, previous, scale)?;
            }
            Output::Source => {
                if pose.authored & Attribute::Angle.bit() != 0 {
                    self.rotation
                        .submit(time, previous, pose.rotation.map(finite))?;
                }
                if pose.authored & Attribute::Position.bit() != 0 {
                    self.translation
                        .submit(time, previous, pose.position.map(finite))?;
                }
                if pose.authored & Attribute::Scale.bit() != 0 {
                    self.scale.submit(time, previous, [finite(pose.scale); 3])?;
                }
            }
        }
        Ok(())
    }

    fn finish(self, end: f32) -> Track {
        Track {
            rotation: self.rotation.finish(end),
            translation: self.translation.finish(end),
            scale: self.scale.finish(end),
        }
    }
}

impl<const N: usize> Keys<N> {
    fn push(&mut self, time: f32, value: [f32; N]) {
        self.times.push(time);
        self.values.push(value);
    }
}

/// Writes a channel's keyframes, leaving out the inside of every run of equal values.
#[derive(Default)]
struct KeyWriter<const N: usize> {
    keys: Keys<N>,
    last: Option<[f32; N]>,
    skipped: bool,
}

impl<const N: usize> KeyWriter<N> {
    fn submit(&mut self, time: f32, previous: f32, value: [f32; N]) -> Result<()> {
        if self.last == Some(value) {
            self.skipped = true;
            return Ok(());
        }
        if let Some(last) = self.last.filter(|_| self.skipped) {
            // The run's end, so the interpolation starts from it and not from the run's start.
            self.skipped = false;
            self.keys.push(previous, last);
        }
        if self.keys.times.last().is_some_and(|&t| t >= time) {
            return Err(malformed("animation", "keyframe times do not increase"));
        }
        self.keys.push(time, value);
        self.last = Some(value);
        Ok(())
    }

    /// The keys, with the end of a run that lasted to the last frame, at `end`.
    fn finish(mut self, end: f32) -> Keys<N> {
        if let Some(last) = self.last.filter(|_| self.skipped) {
            self.keys.push(end, last);
        }
        self.keys
    }
}
