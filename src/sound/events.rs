//! Sound event files (`vsndevts_c`): each event a game names (`Hero_Nevermore.Attack`) with the
//! sound files it picks among and how it plays them.

use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::model::finite;
use crate::resource::Resource;

/// Most events one file holds (the largest of the install has a few thousand).
const MAX_EVENTS: usize = 1 << 16;
/// Most sound files one event picks among.
const MAX_FILES: usize = 1 << 12;
/// How many layers an event has besides itself (`soundevent_layer2`, `soundevent_layer3`).
const LAYERS: [(&str, &str); 2] = [
    ("soundevent_layer2", "soundevent_layer2_on"),
    ("soundevent_layer3", "soundevent_layer3_on"),
];

/// A sound event: what the game plays when it names the event.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundEvent {
    /// The event's name (`Hero_Nevermore.Attack`).
    pub name: String,
    /// Its type, which gives the defaults of what it leaves out (`dota_src1_3d`,
    /// `dota_update_default`, `dota_null_start` for a silent one).
    pub kind: String,
    /// The sound files (`sounds/weapons/hero/nevermore/attack01.vsnd`) one play picks among;
    /// none for a silent event.
    pub files: Vec<String>,
    /// Volume, 1 being the file's.
    pub volume: Option<f32>,
    /// Pitch, 1 being the file's.
    pub pitch: Option<f32>,
    /// How far a play's pitch varies from `pitch`: the lowest and highest offset.
    pub pitch_jitter: Option<(f32, f32)>,
    /// The sound level in decibels at one metre, which sets how far it carries.
    pub sound_level: Option<f32>,
    /// The distance beyond which it is not heard.
    pub distance_max: Option<f32>,
    /// The mix group its volume is balanced in (`Weapons`, `Ultimates`).
    pub mix_group: Option<String>,
    /// How wide a stereo sound spreads around its origin.
    pub spread_radius: Option<f32>,
    /// Seconds before it starts.
    pub delay: Option<f32>,
    /// Seconds it fades in over.
    pub fade_in: Option<f32>,
    /// Seconds it fades out over.
    pub fade_out: Option<f32>,
    /// The length of the longest of its files, in seconds.
    pub duration: Option<f32>,
    /// The events it plays along with.
    pub layers: Vec<SoundLayer>,
    /// How many of it may sound at once, and the instances that are repeats of one sound;
    /// what the event leaves unsaid its type gives ([`SoundStack`]).
    pub limits: SoundLimits,
}

/// What bounds the instances of a sound event: Dota's `soundevent_limiter` (at most
/// `limiter_max` at once, the oldest stopped for a new one unless `limiter_stop_oldest` is
/// off) and `soundevent_block` (a repeat within `block_duration` seconds and `block_distance`
/// units of a sounding instance is not played), the event's `stop_at_time` and the numbers a
/// limiter matches on. Every field is the file's own, unread: a field it does not state is none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SoundLimits {
    /// Whether the limiter counts instances at all.
    pub limiter_on: Option<bool>,
    /// Most instances at once; 0 limits nothing.
    pub limiter_max: Option<f32>,
    /// Whether a limit counts the instances of one entity apart.
    pub limiter_match_entity: Option<bool>,
    /// Whether the oldest instance is the one stopped (else the new one is not played).
    pub limiter_stop_oldest: Option<bool>,
    /// The event name a limiter counts, if it names another's.
    pub limiter_event_name: Option<String>,
    /// Whether repeats are blocked at all.
    pub block_multiple_instances: Option<bool>,
    /// Seconds a sounding instance blocks its repeats for.
    pub block_duration: Option<f32>,
    /// Units about a sounding instance within which a repeat is blocked.
    pub block_distance: Option<f32>,
    /// Seconds after which an instance is stopped; negative for never.
    pub stop_at_time: Option<f32>,
}

/// A sound event type (`dota_src1_3d`): what the events of the type leave unsaid, from
/// `soundstacks/*.vsndstck_c`.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundStack {
    /// The type's name (`dota_src1_3d`).
    pub name: String,
    /// The limits its events get where they state none.
    pub limits: SoundLimits,
}

/// An event played along with another.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundLayer {
    /// The event's name.
    pub event: String,
    /// Whether the layer is switched on.
    pub on: bool,
}

/// The events of a `vsndevts_c` resource, in file order. A member that is not an event is left
/// out.
pub fn read_sound_events(resource: &Resource) -> Result<Vec<SoundEvent>> {
    let data = resource
        .data()
        .ok_or_else(|| malformed("vsndevts", "no DATA block"))??;
    let root = data
        .as_object()
        .ok_or_else(|| malformed("vsndevts", "not an object"))?;
    if root.len() > MAX_EVENTS {
        return Err(malformed("vsndevts", "more events than any file has"));
    }
    let mut out = Vec::with_capacity(root.len());
    for (name, value) in root.iter() {
        if value.as_object().is_some() {
            out.push(event(name, value)?);
        }
    }
    Ok(out)
}

fn event(name: &str, v: &Value) -> Result<SoundEvent> {
    let number = |key: &str| v.get(key).and_then(Value::as_f64).map(|n| finite(n as f32));
    let text = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    let jitter = number("pitch_rand_min").zip(number("pitch_rand_max"));
    Ok(SoundEvent {
        name: name.to_owned(),
        kind: text("type").unwrap_or_default(),
        files: files(v.get("vsnd_files"))?,
        volume: number("volume"),
        pitch: number("pitch"),
        pitch_jitter: jitter,
        sound_level: number("soundlevel"),
        distance_max: number("distance_max"),
        mix_group: text("mixgroup"),
        spread_radius: number("spread_radius"),
        delay: number("delay"),
        fade_in: number("volume_fade_in"),
        fade_out: number("volume_fade_out"),
        duration: number("vsnd_duration"),
        limits: limits(|key| v.get(key)),
        layers: LAYERS
            .iter()
            .filter_map(|(key, on)| {
                Some(SoundLayer {
                    event: text(key).filter(|e| !e.is_empty())?,
                    on: number(on).is_none_or(|n| n != 0.0),
                })
            })
            .collect(),
    })
}

/// The files of `vsnd_files`: one string, or a list of them.
fn files(value: Option<&Value>) -> Result<Vec<String>> {
    let list = match value {
        Some(Value::String(one)) => return Ok(vec![one.clone()]),
        Some(Value::Array(list)) => list,
        _ => return Ok(Vec::new()),
    };
    if list.len() > MAX_FILES {
        return Err(malformed("vsndevts", "more files than any event has"));
    }
    Ok(list
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect())
}

/// The limits a getter finds under their keys; a stack states its numbers as strings.
fn limits<'a>(get: impl Fn(&str) -> Option<&'a Value>) -> SoundLimits {
    let number = |key: &str| {
        get(key).and_then(|v| match v {
            Value::String(text) => text.trim().parse::<f64>().ok(),
            other => other.as_f64(),
        })
    };
    let float = |key: &str| number(key).map(|n| finite(n as f32));
    let flag = |key: &str| number(key).map(|n| n != 0.0);
    SoundLimits {
        limiter_on: flag("limiter_on"),
        limiter_max: float("limiter_max"),
        limiter_match_entity: flag("limiter_match_entity"),
        limiter_stop_oldest: get("limiter_stop_oldest").and_then(|v| match v {
            Value::String(text) => Some(text != "false" && text != "0"),
            other => other.as_f64().map(|n| n != 0.0),
        }),
        limiter_event_name: get("limiter_event_name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty() && *name != "0")
            .map(str::to_owned),
        block_multiple_instances: flag("block_multiple_instances"),
        block_duration: float("block_duration"),
        block_distance: float("block_distance"),
        stop_at_time: float("stop_at_time"),
    }
}

/// The event types of a `vsndstck_c` resource, in file order, with the defaults their public
/// variables (`operator_variables` of the operator named `public`) give. A member without
/// them is left out.
pub fn read_sound_stacks(resource: &Resource) -> Result<Vec<SoundStack>> {
    let data = resource
        .data()
        .ok_or_else(|| malformed("vsndstck", "no DATA block"))??;
    let root = data
        .as_object()
        .ok_or_else(|| malformed("vsndstck", "not an object"))?;
    if root.len() > MAX_EVENTS {
        return Err(malformed("vsndstck", "more types than any file has"));
    }
    let mut out = Vec::new();
    for (name, stack) in root.iter() {
        let public = stack
            .get("operators")
            .and_then(Value::as_array)
            .and_then(|ops| {
                ops.iter()
                    .find(|op| op.get("name").and_then(Value::as_str) == Some("public"))
            })
            .and_then(|op| op.get("operator_variables"));
        let Some(variables) = public else {
            continue;
        };
        // Each variable is `{data_type, value}`.
        let limits = limits(|key| variables.get(key).and_then(|v| v.get("value")));
        out.push(SoundStack {
            name: name.to_owned(),
            limits,
        });
    }
    Ok(out)
}

impl SoundLimits {
    /// These limits with the gaps filled from `defaults`.
    #[must_use]
    pub fn or(&self, defaults: &SoundLimits) -> SoundLimits {
        SoundLimits {
            limiter_on: self.limiter_on.or(defaults.limiter_on),
            limiter_max: self.limiter_max.or(defaults.limiter_max),
            limiter_match_entity: self.limiter_match_entity.or(defaults.limiter_match_entity),
            limiter_stop_oldest: self.limiter_stop_oldest.or(defaults.limiter_stop_oldest),
            limiter_event_name: self
                .limiter_event_name
                .clone()
                .or_else(|| defaults.limiter_event_name.clone()),
            block_multiple_instances: self
                .block_multiple_instances
                .or(defaults.block_multiple_instances),
            block_duration: self.block_duration.or(defaults.block_duration),
            block_distance: self.block_distance.or(defaults.block_distance),
            stop_at_time: self.stop_at_time.or(defaults.stop_at_time),
        }
    }
}
