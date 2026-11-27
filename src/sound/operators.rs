use std::borrow::Cow;

use super::events::limiter_event_name;
use super::graph::ResolvedSoundEvent;
use crate::kv3::Value;

/// Playback and envelope settings after event inheritance and stack defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SoundPlayback<'a> {
    /// Base volume.
    pub volume: Option<f32>,
    /// Minimum random volume offset.
    pub volume_rand_min: Option<f32>,
    /// Maximum random volume offset.
    pub volume_rand_max: Option<f32>,
    /// Base pitch multiplier.
    pub pitch: Option<f32>,
    /// Minimum random pitch offset.
    pub pitch_rand_min: Option<f32>,
    /// Maximum random pitch offset.
    pub pitch_rand_max: Option<f32>,
    /// Seconds before playback.
    pub delay: Option<f32>,
    /// Minimum random delay.
    pub delay_rand_min: Option<f32>,
    /// Maximum random delay.
    pub delay_rand_max: Option<f32>,
    /// Seconds to fade in.
    pub fade_in: Option<f32>,
    /// Seconds to fade out.
    pub fade_out: Option<f32>,
    /// Compiled duration in seconds.
    pub duration: Option<f32>,
    /// Mixer routing group.
    pub mix_group: Option<&'a str>,
    /// Authored `soundlevel`.
    pub sound_level: Option<f32>,
    /// Explicit event loop flag. Individual clips can also specify a loop.
    pub looping: Option<bool>,
    /// Authored `max_voices` limit.
    pub max_voices: Option<f32>,
}

/// Attenuation and spatial settings after event inheritance and stack defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SoundSpatial<'a> {
    /// Maximum audible distance.
    pub distance_max: Option<f32>,
    /// The authored curve data, including control points and tangent metadata.
    pub distance_volume_mapping_curve: Option<SoundCurve<'a>>,
    /// Whether to apply the distance-to-volume curve.
    pub use_distance_volume_mapping_curve: Option<bool>,
    /// Distance at which volume attenuation starts.
    pub volume_falloff_min: Option<f32>,
    /// Distance at which volume reaches its attenuation floor.
    pub volume_falloff_max: Option<f32>,
    /// Authored `spread_radius`.
    pub spread_radius: Option<f32>,
    /// Near distance for speaker spread.
    pub spread_min: Option<f32>,
    /// Far distance for speaker spread.
    pub spread_max: Option<f32>,
    /// Explicit position in Source 2 coordinates.
    pub position: Option<[f32; 3]>,
    /// Offset applied to the sound source position.
    pub position_offset: Option<[f32; 3]>,
    /// Whether the event uses the source entity position.
    pub use_entity_position: Option<bool>,
    /// Whether fog of war controls the volume.
    pub use_fog_of_war: Option<bool>,
    /// Search radius for fog-of-war visibility.
    pub fog_of_war_radius: Option<f32>,
    /// Volume multiplier for the local player entity.
    pub volume_on_player_mult: Option<f32>,
}

/// Distance-to-volume control points as stored in the operator stack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundCurve<'a>(&'a Value);

impl<'a> SoundCurve<'a> {
    /// The original value, including any stack-specific metadata.
    pub fn value(&self) -> &'a Value {
        self.0
    }

    /// Numeric points of a given width, in file order. Each point starts with distance and volume.
    pub fn points<const N: usize>(&self) -> impl Iterator<Item = [f32; N]> + 'a {
        self.0.slice().iter().filter_map(vector)
    }
}

/// Limiter and repeat-blocking settings after event inheritance and stack defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SoundOperatorLimits<'a> {
    /// Whether the limiter is enabled.
    pub limiter_on: Option<bool>,
    /// Maximum concurrent matching events.
    pub limiter_max: Option<f32>,
    /// Event name used for limiter matching.
    pub limiter_event_name: Option<&'a str>,
    /// Whether the limiter matches this event name.
    pub limiter_match_this: Option<bool>,
    /// Whether limiter name matching uses a substring.
    pub limiter_match_substring: Option<bool>,
    /// Whether limiter matching includes the source entity.
    pub limiter_match_entity: Option<bool>,
    /// Event type code to match.
    pub limiter_match_type: Option<f32>,
    /// Minimum duration for limiter matches.
    pub limiter_match_event_duration: Option<f32>,
    /// Whether the oldest matching event is stopped first.
    pub limiter_stop_oldest: Option<bool>,
    /// Whether matching instances block repeats.
    pub block_multiple_instances: Option<bool>,
    /// Whether repeat blocking matches the source entity.
    pub block_match_entity: Option<bool>,
    /// First event name used by timed repeat blocking.
    pub block_name: Option<&'a str>,
    /// Second event name used by timed repeat blocking.
    pub block_name2: Option<&'a str>,
    /// Block time for the first named event.
    pub block_time: Option<f32>,
    /// Block time for the second named event.
    pub block_time2: Option<f32>,
    /// Distance within which a matching repeat is blocked.
    pub block_distance: Option<f32>,
    /// Seconds during which a matching repeat is blocked.
    pub block_duration: Option<f32>,
    /// Time at which playback stops.
    pub stop_at_time: Option<f32>,
}

/// A named event triggered by a sound-event operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SoundEventLink<'a> {
    /// Key that stores the referenced name.
    pub field: &'static str,
    /// Exact name of the referenced event.
    pub event: &'a str,
    /// Effective on/off switch, when supplied by the stack or event.
    pub enabled: Option<bool>,
}

const LAYER_FIELDS: [(&str, &str); 8] = [
    ("soundevent_layer2", "soundevent_layer2_on"),
    ("soundevent_layer3", "soundevent_layer3_on"),
    ("soundevent_01", "use_01"),
    ("soundevent_02", "use_02"),
    ("soundevent_03", "use_03"),
    ("layer_01_soundevent", ""),
    ("layer_02_soundevent", ""),
    ("layer_03_soundevent", ""),
];

impl<'a> ResolvedSoundEvent<'a> {
    /// Reads a finite scalar, including numeric strings in stack defaults.
    pub fn number(&self, key: &str) -> Option<f32> {
        number(self.get(key)?)
    }

    /// Reads a Boolean, including numeric flags and stack strings.
    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Value::Bool(flag) => Some(*flag),
            Value::String(text) if text.trim().eq_ignore_ascii_case("true") => Some(true),
            Value::String(text) if text.trim().eq_ignore_ascii_case("false") => Some(false),
            value => number(value).map(|n| n != 0.0),
        }
    }

    /// Effective playback settings.
    pub fn playback(&self) -> SoundPlayback<'a> {
        SoundPlayback {
            volume: self.number("volume"),
            volume_rand_min: self.number("volume_rand_min"),
            volume_rand_max: self.number("volume_rand_max"),
            pitch: self.number("pitch"),
            pitch_rand_min: self.number("pitch_rand_min"),
            pitch_rand_max: self.number("pitch_rand_max"),
            delay: self.number("delay"),
            delay_rand_min: self.number("rand_delay_min"),
            delay_rand_max: self.number("rand_delay_max"),
            fade_in: self
                .number("volume_fade_in")
                .or_else(|| self.number("fade_in")),
            fade_out: self
                .number("volume_fade_out")
                .or_else(|| self.number("fade_out")),
            duration: self.number("vsnd_duration"),
            mix_group: self.get("mixgroup").and_then(Value::as_str),
            sound_level: self.number("soundlevel"),
            looping: self.flag("looping").or_else(|| self.flag("loop")),
            max_voices: self.number("max_voices"),
        }
    }

    /// Effective attenuation and positioning settings.
    pub fn spatial(&self) -> SoundSpatial<'a> {
        SoundSpatial {
            distance_max: self.number("distance_max"),
            distance_volume_mapping_curve: self
                .get("distance_volume_mapping_curve")
                .map(SoundCurve),
            use_distance_volume_mapping_curve: self.flag("use_distance_volume_mapping_curve"),
            volume_falloff_min: self.number("volume_falloff_min"),
            volume_falloff_max: self.number("volume_falloff_max"),
            spread_radius: self.number("spread_radius"),
            spread_min: self.number("spread_min"),
            spread_max: self.number("spread_max"),
            position: self.get("position").and_then(vector),
            position_offset: self.get("position_offset").and_then(vector),
            use_entity_position: self.flag("use_entity_position"),
            use_fog_of_war: self.flag("volume_use_FOW"),
            fog_of_war_radius: self.number("volume_FOW_radius"),
            volume_on_player_mult: self.number("volume_on_player_mult"),
        }
    }

    /// Effective limiter and repeat-blocking settings.
    pub fn limits(&self) -> SoundOperatorLimits<'a> {
        SoundOperatorLimits {
            limiter_on: self.flag("limiter_on"),
            limiter_max: self.number("limiter_max"),
            limiter_event_name: limiter_event_name(|key| self.get(key)),
            limiter_match_this: self.flag("limiter_match_this"),
            limiter_match_substring: self.flag("limiter_match_substring"),
            limiter_match_entity: self.flag("limiter_match_entity"),
            limiter_match_type: self.number("limiter_match_type"),
            limiter_match_event_duration: self.number("limiter_match_event_duration"),
            limiter_stop_oldest: self.flag("limiter_stop_oldest"),
            block_multiple_instances: self.flag("block_multiple_instances"),
            block_match_entity: self.flag("block_match_entity"),
            block_name: self.get("block_name").and_then(Value::as_str),
            block_name2: self.get("block_name2").and_then(Value::as_str),
            block_time: self.number("block_time"),
            block_time2: self.number("block_time2"),
            block_distance: self.number("block_distance"),
            block_duration: self.number("block_duration"),
            stop_at_time: self.number("stop_at_time"),
        }
    }

    /// Layer and multi-start references, including disabled layers.
    pub fn layers(&self) -> impl Iterator<Item = SoundEventLink<'a>> + '_ {
        LAYER_FIELDS.into_iter().filter_map(|(field, on)| {
            let event = self.get(field)?.as_str()?;
            (!event.is_empty() && event != "None").then(|| SoundEventLink {
                field,
                event,
                enabled: if on.is_empty() { None } else { self.flag(on) },
            })
        })
    }

    /// Gets a track with its effective selector and weights, including explicitly empty tracks.
    pub fn track(&self, track: SoundTrack) -> Option<SoundFileTrack<'a>> {
        let (files, weights, mode, index) = match track {
            SoundTrack::Main => (
                "vsnd_files",
                "weights",
                "vsnd_selection_type",
                "vsnd_selection_index",
            ),
            SoundTrack::Track01 => (
                "vsnd_files_track_01",
                "weights_track_01",
                "vsnd_selection_type_track_01",
                "vsnd_selection_index_track_01",
            ),
            SoundTrack::Track02 => (
                "vsnd_files_track_02",
                "weights_track_02",
                "vsnd_selection_type_track_02",
                "vsnd_selection_index_track_02",
            ),
        };
        Some(SoundFileTrack {
            track,
            files: self.get(files)?,
            weights: self.get(weights),
            selection: self.get(mode),
            selection_index: self.number(index),
        })
    }

    /// Tracks in primary, 01, 02 order.
    pub fn tracks(&self) -> impl Iterator<Item = SoundFileTrack<'a>> + '_ {
        [SoundTrack::Main, SoundTrack::Track01, SoundTrack::Track02]
            .into_iter()
            .filter_map(|track| self.track(track))
    }
}

fn number(value: &Value) -> Option<f32> {
    match value {
        Value::String(text) => finite_number(text),
        value => value.finite_f32(),
    }
}

fn finite_number(text: &str) -> Option<f32> {
    let text = text.trim();
    text.strip_suffix('f')
        .unwrap_or(text)
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
}

fn vector<const N: usize>(value: &Value) -> Option<[f32; N]> {
    if let Some(text) = value.as_str() {
        let text = text.trim().strip_prefix('[')?.strip_suffix(']')?;
        let mut fields = text.trim().trim_end_matches(',').split(',');
        let mut vector = [0.0; N];
        for component in &mut vector {
            *component = finite_number(fields.next()?)?;
        }
        return fields.next().is_none().then_some(vector);
    }
    value.f32_array()
}

/// The source slot of a track in a sound event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundTrack {
    /// Primary `vsnd_files` slot.
    Main,
    /// Secondary `vsnd_files_track_01` slot.
    Track01,
    /// Secondary `vsnd_files_track_02` slot.
    Track02,
}

/// The track's stored selector. Sound stacks may define their own selector names or codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SoundSelectionMode<'a> {
    /// Use the separate selection index.
    Index,
    /// Select in increasing index order.
    Forward,
    /// Select in decreasing index order.
    Backward,
    /// Select independently on each play.
    Random,
    /// Select randomly without repetition within a cycle.
    RandomExclusive,
    /// Select randomly using the candidate weights.
    RandomWeighted,
    /// A stack-defined selector name.
    Other(&'a str),
    /// A stack-defined numeric selector code.
    Code(i64),
}

/// A track and its selection metadata.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundFileTrack<'a> {
    /// Track slot in the event.
    pub track: SoundTrack,
    /// The authored index when the stack selects a file by index.
    pub selection_index: Option<f32>,
    files: &'a Value,
    weights: Option<&'a Value>,
    selection: Option<&'a Value>,
}

impl<'a> SoundFileTrack<'a> {
    /// Authored files in selection order, with weights matched by file index.
    pub fn files(&self) -> impl Iterator<Item = SoundFile<'a>> + '_ {
        let weights = self.weights.into_iter().flat_map(file_values).map(number);
        file_values(self.files)
            .zip(weights.chain(std::iter::repeat(None)))
            .filter_map(|(file, weight)| {
                Some(SoundFile {
                    path: file.as_str()?,
                    weight,
                })
            })
    }

    /// The selector before any interpretation by the operator stack.
    pub fn selection_mode(&self) -> Option<SoundSelectionMode<'a>> {
        let mode = self.selection?;
        use SoundSelectionMode::*;
        Some(match mode.as_str() {
            Some("index") => Index,
            Some("forward") => Forward,
            Some("backward") => Backward,
            Some("random") => Random,
            Some("random_exclusive") => RandomExclusive,
            Some("random_weighted") => RandomWeighted,
            Some(other) => Other(other),
            None => Code(mode.as_i64()?),
        })
    }
}

fn file_values(value: &Value) -> impl Iterator<Item = &Value> {
    let scalar = match value {
        Value::Object(_) | Value::Array(_) | Value::Null => None,
        _ => Some(value),
    };
    let keyed = value.as_object().into_iter().flat_map(|object| {
        object.iter().filter_map(|(key, value)| {
            let suffix = key.strip_prefix("value")?;
            suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit())
                .then_some(value)
        })
    });
    scalar.into_iter().chain(value.slice()).chain(keyed)
}

/// One candidate sound and its authored selection weight.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SoundFile<'a> {
    /// Original sound resource path.
    pub path: &'a str,
    /// Authored selection weight for this candidate.
    pub weight: Option<f32>,
}

impl<'a> SoundFile<'a> {
    /// The compiled VPK entry path for this sound reference.
    pub fn compiled_path(&self) -> Option<Cow<'a, str>> {
        if self.path.ends_with(".vsnd_c") {
            Some(Cow::Borrowed(self.path))
        } else if self.path.ends_with(".vsnd") {
            Some(Cow::Owned(format!("{}_c", self.path)))
        } else {
            None
        }
    }
}
