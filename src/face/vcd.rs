//! Facial choreography scenes (`.vcd_c`, KV3 `vcd3` in their `DATA`), such as Dota's heroes'
//! idle expressions (`IdleExpression` of `scripts/npc/heroes/npc_dota_hero_<name>.txt`). Read
//! here are what moves a face: the scene's loop and its `flexanimation` events, each with its
//! stretch of the scene, its ramp (the weight it is laid over the face with, a Hermite curve
//! with explicit slopes) and its flex controller tracks (values normalized to each track's
//! range, at seconds into the event).

use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::model::array;

const WHAT: &str = "vcd";
/// Most events, tracks of an event, keys of a curve; a hero's idle scene has 3 events of up to
/// 16 tracks with up to a few hundred samples each.
const MAX_EVENTS: usize = 256;
const MAX_TRACKS: usize = 256;
const MAX_KEYS: usize = 1 << 14;
/// Largest time or value read; a scene lasts seconds.
const MAX_NUMBER: f32 = 360_000.0;

/// The flex animation of a choreography scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// The scene's loop, when it loops forever: on reaching the second time it goes back to the
    /// first.
    pub looping: Option<(f32, f32)>,
    /// Its flex animation events, of every actor and channel, in file order.
    pub events: Vec<FlexEvent>,
}

/// A `flexanimation` event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlexEvent {
    /// Seconds into the scene it starts and ends at.
    pub start: f32,
    /// See [`start`](Self::start).
    pub end: f32,
    /// Its weight over its stretch (seconds into it); empty for a weight of 1 throughout.
    pub ramp: Vec<RampKey>,
    /// Its controller tracks; disabled tracks are left out.
    pub tracks: Vec<FlexTrack>,
}

/// A key of an event's ramp: a cubic Hermite curve through the keys, with each key's slopes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RampKey {
    /// Seconds into the event.
    pub x: f32,
    /// The weight there.
    pub y: f32,
    /// The slope arriving at the key and the one leaving it.
    pub slope_in: f32,
    /// See [`slope_in`](Self::slope_in).
    pub slope_out: f32,
}

/// A flex controller's track of an event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlexTrack {
    /// The controller's name.
    pub name: String,
    /// The controller's range the values are normalized to (0 is `min`, 1 is `max`).
    pub min: f32,
    /// See [`min`](Self::min).
    pub max: f32,
    /// Seconds into the event and the normalized value there, in time order.
    pub samples: Vec<(f32, f32)>,
}

fn float(v: &Value, key: &str) -> Option<f32> {
    v.get(key)
        .and_then(Value::as_f64)
        .map(|f| f as f32)
        .filter(|f| f.is_finite() && f.abs() <= MAX_NUMBER)
}

fn bounded<'a>(list: &'a [Value], max: usize, what: &str) -> Result<&'a [Value]> {
    if list.len() > max {
        return Err(malformed(WHAT, format!("more than {max} {what}")));
    }
    Ok(list)
}

impl Scene {
    /// The flex animation of a scene's `DATA` (decoded KV3).
    ///
    /// # Errors
    ///
    /// An event without its times, timed in fractions of itself (an old scene), a ramp key, track
    /// or sample without its numbers, samples out of time order, and more than 256 events or
    /// tracks of an event, or 16384 keys of a curve.
    pub fn read(data: &Value) -> Result<Self> {
        let mut scene = Self::default();
        for event in bounded(array(data.get("events")), MAX_EVENTS, "scene events")? {
            let looping = event.get("type").and_then(Value::as_str) == Some("loop")
                && event.get("loopcount").and_then(Value::as_i64) == Some(-1);
            if let (true, Some(from), Some(to)) = (
                looping,
                float(event, "start_time"),
                float(event, "end_time"),
            ) && from < to
            {
                scene.looping = Some((from, to));
                break;
            }
        }
        for actor in bounded(array(data.get("actors")), MAX_EVENTS, "actors")? {
            for channel in bounded(array(actor.get("channels")), MAX_EVENTS, "channels")? {
                for event in array(channel.get("events")) {
                    if event.get("type").and_then(Value::as_str) != Some("flexanimation") {
                        continue;
                    }
                    if scene.events.len() >= MAX_EVENTS {
                        return Err(malformed(WHAT, "too many events"));
                    }
                    scene.events.push(flex_event(event)?);
                }
            }
        }
        Ok(scene)
    }
}

fn flex_event(event: &Value) -> Result<FlexEvent> {
    let (Some(start), Some(end)) = (float(event, "start_time"), float(event, "end_time")) else {
        return Err(malformed(WHAT, "an event without its times"));
    };
    if event.get("samples_use_time").and_then(Value::as_bool) == Some(false) {
        return Err(malformed(WHAT, "an event timed in fractions"));
    }
    let keys = event
        .get("event_ramp_curve")
        .map(|r| array(r.get("m_spline")));
    let ramp = bounded(keys.unwrap_or_default(), MAX_KEYS, "ramp keys")?
        .iter()
        .map(|key| {
            let n = ["x", "y", "m_flSlopeIncoming", "m_flSlopeOutgoing"].map(|k| float(key, k));
            let [Some(x), Some(y), Some(slope_in), Some(slope_out)] = n else {
                return Err(malformed(WHAT, "a ramp key"));
            };
            Ok(RampKey {
                x,
                y,
                slope_in,
                slope_out,
            })
        })
        .collect::<Result<_>>()?;
    let mut tracks = Vec::new();
    for track in bounded(array(event.get("flexanimations")), MAX_TRACKS, "tracks")? {
        if track.get("disabled").and_then(Value::as_bool) != Some(true) {
            tracks.push(flex_track(track)?);
        }
    }
    Ok(FlexEvent {
        start,
        end,
        ramp,
        tracks,
    })
}

fn flex_track(track: &Value) -> Result<FlexTrack> {
    let name = track
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (min, max) = (
        float(track, "min").unwrap_or(0.0),
        float(track, "max").unwrap_or(1.0),
    );
    if name.is_empty() || min > max {
        return Err(malformed(WHAT, "a track's controller or range"));
    }
    let mut samples = Vec::new();
    for sample in bounded(array(track.get("samples")), MAX_KEYS, "samples")? {
        let (Some(t), Some(v)) = (float(sample, "time"), float(sample, "value")) else {
            return Err(malformed(WHAT, "a sample"));
        };
        if samples.last().is_some_and(|&(last, _)| t < last) {
            return Err(malformed(WHAT, "samples out of order"));
        }
        samples.push((t, v));
    }
    Ok(FlexTrack {
        name: name.to_owned(),
        min,
        max,
        samples,
    })
}
