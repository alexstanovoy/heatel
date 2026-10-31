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
