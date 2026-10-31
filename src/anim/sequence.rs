//! What a model's sequences say about themselves: the activities they play for (with the
//! modifiers Dota picks them by), their length, and the events animators put in them (a sound at
//! a footstep, a particle at the moment of a swing). The `ASEQ` block names the sequences and
//! their activities, the `ANIM` block holds the animations they play, with the events.

use super::clip::{block, listing};
use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::model::{array, finite};
use crate::resource::Resource;

/// Most sequences a model has (the largest hero here has about 700).
const MAX_SEQUENCES: usize = 1 << 14;
/// Most events all the sequences of a model carry together.
const MAX_EVENTS: usize = 1 << 16;
/// Most activity entries (activities and their modifiers) one sequence names.
const MAX_ACTIVITY_ENTRIES: usize = 64;
/// Frame rates outside this are not an animation's.
const MIN_FPS: f32 = 1e-3;
const MAX_FPS: f32 = 10_000.0;
/// What an activity's name starts with; the entries after it, up to the next, are its modifiers.
const ACTIVITY_PREFIX: &str = "ACT_";

/// A sequence of a model: one thing it can play.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    /// The sequence's name (`attack01_fast`, `run_anim`).
    pub name: String,
    /// The activities it plays for. Dota picks the sequence of an activity whose modifiers it
    /// has, by weight; a sequence without any is only played by name.
    pub activities: Vec<Activity>,
    /// Whether it loops.
    pub looping: bool,
    /// The frames of its animation (the first one it plays, for a blend).
    pub frames: u32,
    /// Its frame rate, in frames a second.
    pub fps: f32,
    /// The events of its animation, by frame.
    pub events: Vec<AnimEvent>,
}

impl Sequence {
    /// The time of its last frame in seconds, as [`Clip::duration`](super::Clip::duration) counts
    /// it: `(frames - 1) / fps`.
    pub fn duration(&self) -> f32 {
        self.frames.saturating_sub(1) as f32 / self.fps
    }

    /// The seconds at which `frame` plays.
    pub fn time_of(&self, frame: u32) -> f32 {
        frame as f32 / self.fps
    }
}

/// An activity a sequence plays for, and the modifiers it needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    /// Dota's name (`ACT_DOTA_ATTACK`).
    pub name: String,
    /// How likely it is picked among the sequences that fit (higher is likelier).
    pub weight: u32,
    /// The modifiers (`fast`, `injured`, `haste`) the unit must have for it to be picked.
    pub modifiers: Vec<String>,
}

/// An event in an animation.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimEvent {
    /// Source's name of the event (`AE_CL_PLAYSOUND`).
    pub name: String,
    /// The frame it fires on.
    pub frame: u32,
    /// The frame a span event ends on; none for a moment.
    pub end_frame: Option<u32>,
    /// Where in the animation it fires, `0..=1`.
    pub cycle: f32,
    /// What the event says, for the kinds this crate knows.
    pub data: EventData,
}

/// What an animation event says.
#[derive(Debug, Clone, PartialEq)]
pub enum EventData {
    /// `AE_CL_PLAYSOUND`: a sound event (`Hero_Pudge.Footsteps`).
    PlaySound {
        /// The sound event's name.
        sound: String,
        /// Whether changing the sequence stops it.
        stop_on_seq_change: bool,
        /// The tag a `SuppressTag` event can switch it off by.
        tag: Option<String>,
    },
    /// `AE_DOTA_PLAY_SOUND_ATTACK`: the attack sound of the unit's sound set; `sound` names one
    /// instead when it is not empty.
    AttackSound {
        /// The sound event's name, empty for the unit's own.
        sound: String,
    },
    /// `AE_CL_CREATE_PARTICLE_EFFECT_CFG`: a particle system, started the way its configuration
    /// says (`absorigin_follow`, `point_follow`: where it is attached and whether it follows).
    Particle {
        /// The particle system's resource name (`particles/.../nvm_atk_blur.vpcf`).
        particle: String,
        /// The configuration's name.
        config: String,
        /// Whether it stays after its unit is gone.
        detach_from_owner: bool,
        /// Whether it plays even where effects are reduced.
        always_run: bool,
        /// Whether changing the sequence ends it.
        stop_on_seq_change: bool,
        /// Whether changing the sequence ends it without letting it fade.
        stop_immediately: bool,
        /// Whether several of its events share one system.
        aggregate: bool,
        /// The tag a `SuppressTag` event can switch it off by.
        tag: Option<String>,
    },
    /// `AE_CL_SPEECH`: a line of the voice, by response rule concept.
    Speech {
        /// The concept (`TLK_DOTA_CUSTOM`).
        concept: String,
        /// The criteria it is asked with (`special_spawn=pudge_loadout_spawn,...`).
        criteria: String,
    },
    /// `AE_CL_CREATE_ANIM_SCOPE_PROP`: a model that comes with the animation.
    Prop {
        /// The model's resource name.
        model: String,
        /// The attachment it is placed on; none for the unit's origin.
        attachment: Option<String>,
        /// The activity it plays.
        activity: String,
        /// Whether it is parented to the unit.
        parent: bool,
        /// Its skin.
        material_group: String,
    },
    /// `AE_CL_SUPPRESS_EVENTS_WITH_TAG`: switches the events with a tag off or on.
    SuppressTag {
        /// The tag.
        tag: String,
        /// Off (true) or on.
        suppress: bool,
    },
    /// Any other event (cloth, footstep dust, ...): only its name and moment are read.
    Other,
}

/// The sequences of the model `resource`, in the order the model lists them (its sequences,
/// then the animations no sequence names). A sequence without an animation (a bind pose, a blend
/// of nothing) is left out. A model without embedded animation has none.
pub fn read_sequences(resource: &Resource) -> Result<Vec<Sequence>> {
    let Some(ctrl) = resource.kv3(b"CTRL") else {
        return Ok(Vec::new());
    };
    let ctrl = ctrl?.root;
    let Some(embedded) = ctrl.get("embedded_animation") else {
        return Ok(Vec::new());
    };
    let Some(anim) = block(resource, embedded, "anim_data_block")? else {
        return Ok(Vec::new());
    };
    let sequences = block(resource, embedded, "seqgroup_data_block")?;
    let descs = array(anim.get("m_animArray"));
    let mut out = Vec::new();
    let mut events = 0;
    for (name, desc, sequence) in listing(sequences.as_ref(), descs) {
        if out.len() == MAX_SEQUENCES {
            return Err(malformed("animation", "more sequences than any model has"));
        }
        let (flags, activities) = match sequence {
            Some(seq) => (seq.get("m_flags"), seq.get("m_activityArray")),
            None => (desc.get("m_flags"), desc.get("m_activityArray")),
        };
        let own = read_events(desc, &mut events)?;
        out.push(Sequence {
            name: name.to_owned(),
            activities: read_activities(activities)?,
            looping: flags.and_then(|f| f.get("m_bLooping")?.as_bool()) == Some(true),
            frames: frames(desc),
            fps: frame_rate(desc),
            events: own,
        });
    }
    Ok(out)
}

fn frames(desc: &Value) -> u32 {
    desc.get("m_pData")
        .and_then(|d| d.get("m_nFrames")?.as_u64())
        .map_or(0, |f| u32::try_from(f).unwrap_or(u32::MAX))
}

fn frame_rate(desc: &Value) -> f32 {
    let fps = finite(desc.get("fps").and_then(Value::as_f64).unwrap_or(0.0) as f32);
    // A rate of zero or less plays at one frame a second, as the clips do.
    if fps <= 0.0 {
        1.0
    } else {
        fps.clamp(MIN_FPS, MAX_FPS)
    }
}

/// The activities of an `m_activityArray`: each `ACT_` entry, with the plain entries after it as
/// its modifiers. Plain entries before any activity belong to none and are left out.
fn read_activities(entries: Option<&Value>) -> Result<Vec<Activity>> {
    let entries = array(entries);
    if entries.len() > MAX_ACTIVITY_ENTRIES {
        return Err(malformed(
            "animation",
            "more activities than any sequence has",
        ));
    }
    let mut out: Vec<Activity> = Vec::new();
    for entry in entries {
        let name = entry
            .get("m_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.starts_with(ACTIVITY_PREFIX) {
            let weight = entry.get("m_nWeight").and_then(Value::as_u64);
            out.push(Activity {
                name: name.to_owned(),
                weight: weight.map_or(1, |w| u32::try_from(w).unwrap_or(u32::MAX)),
                modifiers: Vec::new(),
            });
        } else if let (false, Some(last)) = (name.is_empty(), out.last_mut()) {
            last.modifiers.push(name.to_owned());
        }
    }
    Ok(out)
}

/// The events of an animation, charged to `total`, sorted by frame (stably).
fn read_events(desc: &Value, total: &mut usize) -> Result<Vec<AnimEvent>> {
    let list = array(desc.get("m_eventArray"));
    *total += list.len();
    if *total > MAX_EVENTS {
        return Err(malformed("animation", "more events than any model has"));
    }
    let mut out: Vec<AnimEvent> = list.iter().map(event).collect();
    out.sort_by_key(|e| e.frame);
    Ok(out)
}

fn event(value: &Value) -> AnimEvent {
    let name = value
        .get("m_sEventName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let frame = |key| {
        value
            .get(key)
            .and_then(Value::as_i64)
            .and_then(|f| u32::try_from(f).ok())
    };
    let cycle = value.get("m_flCycle").and_then(Value::as_f64);
    AnimEvent {
        name: name.to_owned(),
        frame: frame("m_nFrame").unwrap_or(0),
        end_frame: frame("m_nEndFrame"),
        cycle: finite(cycle.unwrap_or(0.0) as f32),
        data: value
            .get("m_EventData")
            .map_or(EventData::Other, |d| event_data(name, d)),
    }
}

fn event_data(name: &str, d: &Value) -> EventData {
    let text = |key: &str| {
        d.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let flag = |key: &str| d.get(key).and_then(Value::as_bool) == Some(true);
    let some = |key: &str| Some(text(key)).filter(|t| !t.is_empty());
    match name {
        "AE_CL_PLAYSOUND" => EventData::PlaySound {
            sound: text("name"),
            stop_on_seq_change: flag("stop_on_seq_change"),
            tag: some("tags"),
        },
        "AE_DOTA_PLAY_SOUND_ATTACK" => EventData::AttackSound {
            sound: text("name"),
        },
        "AE_CL_CREATE_PARTICLE_EFFECT_CFG" => EventData::Particle {
            particle: text("name"),
            config: text("config"),
            detach_from_owner: flag("detach_from_owner"),
            always_run: flag("always_run"),
            stop_on_seq_change: flag("stop_on_seq_change"),
            stop_immediately: flag("stop_immediately"),
            aggregate: flag("aggregate"),
            tag: some("tags"),
        },
        "AE_CL_SPEECH" => EventData::Speech {
            concept: text("concept"),
            criteria: text("criteria"),
        },
        "AE_CL_CREATE_ANIM_SCOPE_PROP" => EventData::Prop {
            model: text("name"),
            attachment: some("attachment"),
            activity: text("activity"),
            parent: flag("parent"),
            material_group: text("materialgroup"),
        },
        "AE_CL_SUPPRESS_EVENTS_WITH_TAG" => EventData::SuppressTag {
            tag: text("tag"),
            suppress: flag("suppress"),
        },
        _ => EventData::Other,
    }
}
