//! Source 2 soundscapes (`scripts/soundscapes_manifest.txt`, `scripts/soundscapes*.txt`) and
//! `env_soundscape` entity placements.
//!
//! A soundscape script defines named ambient sound sets made of looping sounds (`playlooping`),
//! periodic random picks (`playrandom` / `rndwave`), and nested soundscapes (`playsoundscape`).
//! Every reader is strictly bounded in definition count, nesting depth, entry count, and work.

use std::collections::HashMap;

use crate::keyvalues::{Kv, parse_ordered};
use crate::kv3::Value;

/// Most soundscape definitions read across all scripts.
pub const MAX_SCAPES: usize = 512;
/// Most entries one flattened soundscape holds, nested soundscapes included.
pub const MAX_ENTRIES: usize = 64;
/// Most waves one `playrandom` entry picks from.
pub const MAX_WAVES: usize = 16;
/// Deepest `playsoundscape` nesting followed.
pub const MAX_NESTING: usize = 4;
/// Most keys inspected while flattening one soundscape, preventing multiplicative blowup from
/// nested soundscapes that emit no entries.
pub const MAX_WORK: usize = 1024;
/// Most soundscape script files listed in a manifest or read together.
pub const MAX_SCRIPTS: usize = 16;
/// Most `env_soundscape` entities returned by [`placed_soundscapes`].
pub const MAX_PLACED: usize = 64;
/// Seconds a soundscape fades in for when it specifies no `fadetime`.
pub const DEFAULT_FADE: f64 = 3.0;

/// Longest relative asset path accepted for soundscape scripts and waves.
const MAX_PATH: usize = 200;
/// Longest soundscape name accepted on a placed `env_soundscape` entity.
const MAX_NAME: usize = 64;

/// A single sound played by a flattened [`Soundscape`].
#[derive(Clone, Debug, PartialEq)]
pub enum SoundscapeEntry {
    /// A looping sound (`playlooping`).
    Loop {
        /// Effective volume after multiplying any enclosing `playsoundscape` volume scales.
        volume: f64,
        /// Pitch percentage (default `100.0`).
        pitch: f64,
        /// Optional world origin; `None` plays everywhere.
        origin: Option<[f64; 3]>,
        /// Attenuation level in dB (`SNDLVL_81dB` -> `81`), capped at `200`; `0` if absent.
        sound_level: u32,
        /// Lower-cased `sounds/.../x.vsnd` asset path.
        wave: String,
    },
    /// A periodic random sound (`playrandom` + `rndwave`).
    Random {
        /// Effective `(min, max)` volume range after multiplying enclosing scales.
        volume: (f64, f64),
        /// `(min, max)` pitch percentage range (default `(100.0, 100.0)`).
        pitch: (f64, f64),
        /// `(min, max)` seconds between plays.
        time: (f64, f64),
        /// Optional world origin; `None` plays everywhere.
        origin: Option<[f64; 3]>,
        /// Attenuation level in dB (`SNDLVL_81dB` -> `81`), capped at `200`; `0` if absent.
        sound_level: u32,
        /// Lower-cased `sounds/.../x.vsnd` asset paths in file order (at least one, at most
        /// [`MAX_WAVES`]).
        waves: Vec<String>,
    },
}

impl SoundscapeEntry {
    /// The `.vsnd` wave paths played by this entry.
    pub fn waves(&self) -> &[String] {
        match self {
            Self::Loop { wave, .. } => std::slice::from_ref(wave),
            Self::Random { waves, .. } => waves,
        }
    }

    /// Whether this entry plays only waves whose file stem (without `.vsnd`) is in `omit`.
    pub fn is_omitted(&self, omit: &[String]) -> bool {
        let stem = |wave: &String| {
            let name = wave.rsplit('/').next().unwrap_or(wave);
            let name = name.strip_suffix(".vsnd").unwrap_or(name);
            omit.iter().any(|o| o == name)
        };
        match self {
            Self::Loop { wave, .. } => stem(wave),
            Self::Random { waves, .. } => waves.iter().all(stem),
        }
    }
}

/// A parsed and flattened soundscape definition.
#[derive(Clone, Debug, PartialEq)]
pub struct Soundscape {
    /// The soundscape's name as defined in its script.
    pub name: String,
    /// Cross-fade duration in seconds (`fadetime`, defaulting to [`DEFAULT_FADE`]).
    pub fade: f64,
    /// Flattened `playlooping` and `playrandom` entries (including nested `playsoundscape`s),
    /// in file order, capped at [`MAX_ENTRIES`].
    pub entries: Vec<SoundscapeEntry>,
}

/// A collection of parsed soundscape definitions indexed case-insensitively by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Soundscapes {
    scapes: Vec<Soundscape>,
    index: HashMap<String, usize>,
}

impl Soundscapes {
    /// Looks up a soundscape by name (ASCII case-insensitive).
    pub fn get(&self, name: &str) -> Option<&Soundscape> {
        self.index
            .get(&name.to_ascii_lowercase())
            .map(|&i| &self.scapes[i])
    }

    /// All parsed soundscapes in definition order.
    pub fn iter(&self) -> &[Soundscape] {
        &self.scapes
    }

    /// Number of soundscape definitions.
    pub fn len(&self) -> usize {
        self.scapes.len()
    }

    /// Whether there are no soundscape definitions.
    pub fn is_empty(&self) -> bool {
        self.scapes.is_empty()
    }
}

/// An `env_soundscape` map entity that starts enabled.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedSoundscape {
    /// The `soundscape` name referenced by the entity.
    pub name: String,
    /// The entity's world position.
    pub origin: [f64; 3],
    /// Activation radius (clamped to `>= 0.0`).
    pub radius: f64,
}

/// Lists the script paths from `scripts/soundscapes_manifest.txt` (`"file" "scripts/x.txt"`),
/// in file order, ignoring commented lines and unsafe paths, capped at [`MAX_SCRIPTS`].
pub fn manifest_files(manifest: &str) -> Vec<String> {
    let Ok(root) = parse_ordered(manifest) else {
        return Vec::new();
    };
    root.block()
        .unwrap_or_default()
        .iter()
        .filter_map(|(_, node)| node.block())
        .flat_map(|items| items.iter())
        .filter_map(|(k, node)| match node {
            Kv::Text(path) if k.eq_ignore_ascii_case("file") => Some(path),
            _ => None,
        })
        .filter(|path| path.starts_with("scripts/") && safe_path(path))
        .take(MAX_SCRIPTS)
        .cloned()
        .collect()
}

enum RawItem {
    Entry(SoundscapeEntry),
    Nested { name: String, volume: f64 },
}

struct RawScape {
    name: String,
    fade: f64,
    items: Vec<RawItem>,
}

/// Parses up to [`MAX_SCRIPTS`] soundscape script texts and flattens their `playsoundscape`
/// references.
///
/// The first definition of a name (ASCII case-insensitive) wins, up to [`MAX_SCAPES`] definitions.
/// Malformed scripts are skipped.
pub fn read_soundscapes(scripts: &[&str]) -> Soundscapes {
    let trees: Vec<Kv> = scripts
        .iter()
        .take(MAX_SCRIPTS)
        .filter_map(|s| parse_ordered(s).ok())
        .collect();
    let mut order: Vec<String> = Vec::new();
    let mut defs: HashMap<String, RawScape> = HashMap::new();
    for tree in &trees {
        for (name, node) in tree.block().unwrap_or_default() {
            if let Some(items) = node.block() {
                let lower = name.to_ascii_lowercase();
                if !defs.contains_key(&lower) && defs.len() < MAX_SCAPES {
                    defs.insert(lower.clone(), parse_raw_scape(name.clone(), items));
                    order.push(lower);
                }
            }
        }
    }
    let mut scapes = Vec::with_capacity(order.len());
    let mut index = HashMap::with_capacity(order.len());
    for lower in order {
        let raw = &defs[&lower];
        let mut flat = Flatten {
            defs: &defs,
            trail: Vec::new(),
            entries: Vec::new(),
            work: 0,
        };
        flat.walk(&lower, 1.0);
        index.insert(lower, scapes.len());
        scapes.push(Soundscape {
            name: raw.name.clone(),
            fade: raw.fade,
            entries: flat.entries,
        });
    }
    Soundscapes { scapes, index }
}

/// Extracts enabled `env_soundscape` entities from an entity lump's entity list, in order,
/// capped at [`MAX_PLACED`].
pub fn placed_soundscapes(entities: &[Value]) -> Vec<PlacedSoundscape> {
    entities
        .iter()
        .filter_map(placed_scape)
        .take(MAX_PLACED)
        .collect()
}

fn placed_scape(entity: &Value) -> Option<PlacedSoundscape> {
    if entity.get("classname").and_then(Value::as_str) != Some("env_soundscape") {
        return None;
    }
    if entity.get("StartDisabled").is_some_and(is_disabled) {
        return None;
    }
    let name = entity.get("soundscape").and_then(Value::as_str)?;
    if !plain_name(name) {
        return None;
    }
    let radius = entity_number(entity.get("radius")?)?;
    let origin = entity_vector(entity.get("origin")?)?;
    Some(PlacedSoundscape {
        name: name.to_owned(),
        origin,
        radius: radius.max(0.0),
    })
}

fn is_disabled(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::String(s) => {
            let s = s.trim();
            s == "1" || s.eq_ignore_ascii_case("true")
        }
        other => other.as_i64().is_some_and(|n| n != 0),
    }
}

fn entity_number(value: &Value) -> Option<f64> {
    match value {
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        other => other.as_f64().filter(|v| v.is_finite()),
    }
}

fn entity_vector(value: &Value) -> Option<[f64; 3]> {
    if let Some(text) = value.as_str() {
        return parse_vec3_str(text);
    }
    let [x, y, z] = value.as_array()? else {
        return None;
    };
    let number = |v: &Value| v.as_f64().filter(|f| f.is_finite()).unwrap_or(0.0);
    Some([number(x), number(y), number(z)])
}

fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
        })
}

fn wave_path(wave: &str) -> Option<String> {
    let path = wave.trim().to_ascii_lowercase();
    let plain = path.starts_with("sounds/") && path.ends_with(".vsnd");
    (plain && safe_path(&path)).then_some(path)
}

fn range(text: &str) -> Option<(f64, f64)> {
    let mut parts = text.split(',').map(|p| {
        p.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
    });
    let first = parts.next()??;
    let second = match parts.next() {
        Some(second) => second?,
        None => first,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((first.min(second), first.max(second)))
}

fn parse_three<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<[f64; 3]> {
    let mut next = || {
        parts
            .next()?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    };
    let coords = [next()?, next()?, next()?];
    parts.next().is_none().then_some(coords)
}

fn parse_vec3_str(text: &str) -> Option<[f64; 3]> {
    let text = text.trim().trim_end_matches(';');
    if text.contains(',') {
        parse_three(text.split(','))
    } else {
        parse_three(text.split_whitespace())
    }
}

fn parse_origin(text: &str) -> Option<[f64; 3]> {
    parse_three(text.trim().trim_end_matches(';').split(','))
}

fn parse_sound_level(level: &str) -> u32 {
    let digits = level.trim().to_ascii_uppercase();
    let digits = digits.strip_prefix("SNDLVL_").unwrap_or(&digits);
    digits
        .trim_end_matches("DB")
        .parse::<u32>()
        .map_or(0, |db| db.min(200))
}

fn charge(work: &mut usize) -> bool {
    *work = work.saturating_add(1);
    *work <= MAX_WORK
}

fn parse_raw_scape(name: String, items: &[(String, Kv)]) -> RawScape {
    let mut fade = None;
    let mut parsed = Vec::new();
    let mut work = 0usize;
    for (kind, node) in items {
        if !charge(&mut work) {
            break;
        }
        match node {
            Kv::Text(t) if kind.eq_ignore_ascii_case("fadetime") => {
                if fade.is_none() {
                    fade = Some(range(t));
                }
            }
            Kv::Block(b) => {
                let body = b.items();
                if kind.eq_ignore_ascii_case("playlooping") {
                    if let Some(entry) = parse_looping(body, &mut work) {
                        parsed.push(RawItem::Entry(entry));
                    }
                } else if kind.eq_ignore_ascii_case("playrandom") {
                    if let Some(entry) = parse_random(body, &mut work) {
                        parsed.push(RawItem::Entry(entry));
                    }
                } else if kind.eq_ignore_ascii_case("playsoundscape")
                    && let Some(nested) = parse_nested(body, &mut work)
                {
                    parsed.push(nested);
                }
            }
            _ => {}
        }
    }
    RawScape {
        name,
        fade: fade.flatten().map_or(DEFAULT_FADE, |r| r.0),
        items: parsed,
    }
}

fn parse_looping(body: &[(String, Kv)], work: &mut usize) -> Option<SoundscapeEntry> {
    let (mut volume, mut pitch, mut origin, mut soundlevel, mut wave) =
        (None, None, None, None, None);
    for (k, node) in body {
        if !charge(work) {
            break;
        }
        let Kv::Text(t) = node else { continue };
        if volume.is_none() && k.eq_ignore_ascii_case("volume") {
            volume = Some(t.as_str());
        } else if pitch.is_none() && k.eq_ignore_ascii_case("pitch") {
            pitch = Some(t.as_str());
        } else if origin.is_none() && k.eq_ignore_ascii_case("origin") {
            origin = Some(t.as_str());
        } else if soundlevel.is_none() && k.eq_ignore_ascii_case("soundlevel") {
            soundlevel = Some(t.as_str());
        } else if wave.is_none() && k.eq_ignore_ascii_case("wave") {
            wave = Some(t.as_str());
        }
    }
    Some(SoundscapeEntry::Loop {
        volume: volume.map_or(Some((1.0, 1.0)), range)?.0,
        pitch: pitch.map_or(Some((100.0, 100.0)), range)?.0,
        origin: origin.and_then(parse_origin),
        sound_level: soundlevel.map_or(0, parse_sound_level),
        wave: wave_path(wave?)?,
    })
}

fn parse_random(body: &[(String, Kv)], work: &mut usize) -> Option<SoundscapeEntry> {
    let (mut volume, mut pitch, mut time, mut origin, mut soundlevel) =
        (None, None, None, None, None);
    let mut waves = Vec::new();
    for (k, node) in body {
        if !charge(work) {
            break;
        }
        match node {
            Kv::Text(t) => {
                if volume.is_none() && k.eq_ignore_ascii_case("volume") {
                    volume = Some(t.as_str());
                } else if pitch.is_none() && k.eq_ignore_ascii_case("pitch") {
                    pitch = Some(t.as_str());
                } else if time.is_none() && k.eq_ignore_ascii_case("time") {
                    time = Some(t.as_str());
                } else if origin.is_none() && k.eq_ignore_ascii_case("origin") {
                    origin = Some(t.as_str());
                } else if soundlevel.is_none() && k.eq_ignore_ascii_case("soundlevel") {
                    soundlevel = Some(t.as_str());
                }
            }
            Kv::Block(b) if k.eq_ignore_ascii_case("rndwave") && waves.len() < MAX_WAVES => {
                for (wk, wnode) in b.items() {
                    if !charge(work) {
                        break;
                    }
                    if let Kv::Text(w) = wnode
                        && wk.eq_ignore_ascii_case("wave")
                        && let Some(path) = wave_path(w)
                    {
                        waves.push(path);
                        if waves.len() >= MAX_WAVES {
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if waves.is_empty() {
        return None;
    }
    Some(SoundscapeEntry::Random {
        volume: volume.map_or(Some((1.0, 1.0)), range)?,
        pitch: pitch.map_or(Some((100.0, 100.0)), range)?,
        time: range(time?)?,
        origin: origin.and_then(parse_origin),
        sound_level: soundlevel.map_or(0, parse_sound_level),
        waves,
    })
}

fn parse_nested(body: &[(String, Kv)], work: &mut usize) -> Option<RawItem> {
    let (mut name, mut volume) = (None, None);
    for (k, node) in body {
        if !charge(work) {
            break;
        }
        let Kv::Text(t) = node else { continue };
        if name.is_none() && k.eq_ignore_ascii_case("name") {
            name = Some(t.as_str());
        } else if volume.is_none() && k.eq_ignore_ascii_case("volume") {
            volume = Some(t.as_str());
        }
    }
    let name = name?.to_ascii_lowercase();
    let volume = volume.and_then(range).map_or(1.0, |r| r.0);
    Some(RawItem::Nested { name, volume })
}

fn scale_entry(entry: &SoundscapeEntry, mult: f64) -> Option<SoundscapeEntry> {
    match entry {
        SoundscapeEntry::Loop {
            volume,
            pitch,
            origin,
            sound_level,
            wave,
        } => {
            let scaled = *volume * mult;
            scaled.is_finite().then(|| SoundscapeEntry::Loop {
                volume: scaled,
                pitch: *pitch,
                origin: *origin,
                sound_level: *sound_level,
                wave: wave.clone(),
            })
        }
        SoundscapeEntry::Random {
            volume: (low, high),
            pitch,
            time,
            origin,
            sound_level,
            waves,
        } => {
            let (lo, hi) = (*low * mult, *high * mult);
            (lo.is_finite() && hi.is_finite()).then(|| SoundscapeEntry::Random {
                volume: (lo, hi),
                pitch: *pitch,
                time: *time,
                origin: *origin,
                sound_level: *sound_level,
                waves: waves.clone(),
            })
        }
    }
}

struct Flatten<'a> {
    defs: &'a HashMap<String, RawScape>,
    trail: Vec<String>,
    entries: Vec<SoundscapeEntry>,
    work: usize,
}

impl Flatten<'_> {
    fn walk(&mut self, name: &str, volume: f64) {
        let Some(raw) = self.defs.get(name) else {
            return;
        };
        if self.trail.len() > MAX_NESTING || self.trail.iter().any(|t| t == name) {
            return;
        }
        self.trail.push(name.to_owned());
        for item in &raw.items {
            if !charge(&mut self.work) || self.entries.len() >= MAX_ENTRIES {
                break;
            }
            match item {
                RawItem::Entry(entry) => {
                    if let Some(scaled) = scale_entry(entry, volume) {
                        self.entries.push(scaled);
                    }
                }
                RawItem::Nested {
                    name: inner,
                    volume: scale,
                } => {
                    let next = volume * *scale;
                    if next.is_finite() {
                        self.walk(inner, next);
                    }
                }
            }
        }
        self.trail.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCAPES: &str = r#"
// the day
"default_day"
{
    "fadetime" "2"
    "playlooping"
    {
        "volume" ".2"  "pitch" "100"  "soundlevel" "SNDLVL_81dB"
        "wave" "sounds/ambient/soundscapes/Spawn_L.vsnd"
        "origin" "-7350, -6825, 389;"
    }
    //"playlooping"
    //{
    //    "volume" "1"
    //    "wave" "sounds/ambient/soundscapes/commented.vsnd"
    //}
    "playsoundscape" { "name" "weather" "volume" "0.5" }
}
"weather"
{
    "playlooping" { "volume" "0.6" "pitch" "100" "wave" "sounds/ambient/soundscapes/day_lp.vsnd" }
    "playrandom"
    {
        "volume" "0.7, 1.0"  "pitch" "95, 105"  "time" "6, 9"  "position" "random"
        "rndwave"
        {
            "wave" "sounds/ambient/soundscapes/b1.vsnd"
            "wave" "sounds/ambient/soundscapes/b2.vsnd"
            "wave" "sounds/ambient/soundscapes/b3.vsnd"
        }
    }
    "playsoundscape" { "name" "default_day" "volume" "1" }
}"#;

    #[test]
    fn manifest_files_reads_unquoted_root_and_skips_comments_and_unsafe_paths() {
        let manifest = "soundscapes_manifest\n{\n\
            \"file\" \"scripts/a.txt\"\n\
            //\"file\" \"scripts/b.txt\"\n\
            \"file\" \"scripts/../escape.txt\"\n\
            \"file\" \"other/c.txt\"\n\
            \"file\" \"scripts/d.txt\"\n\
            }";
        assert_eq!(manifest_files(manifest), ["scripts/a.txt", "scripts/d.txt"]);
    }

    #[test]
    fn read_soundscapes_flattens_nested_scapes_scales_volume_and_stops_cycles() {
        let scapes = read_soundscapes(&[SCAPES]);
        assert_eq!(scapes.len(), 2);
        let day = scapes.get("DEFAULT_DAY").expect("case-insensitive lookup");
        assert_eq!(day.name, "default_day");
        assert_eq!(day.fade, 2.0);
        assert_eq!(day.entries.len(), 3);
        assert_eq!(
            day.entries[0],
            SoundscapeEntry::Loop {
                volume: 0.2,
                pitch: 100.0,
                origin: Some([-7350.0, -6825.0, 389.0]),
                sound_level: 81,
                wave: "sounds/ambient/soundscapes/spawn_l.vsnd".to_owned(),
            }
        );
        assert_eq!(
            day.entries[1],
            SoundscapeEntry::Loop {
                volume: 0.3,
                pitch: 100.0,
                origin: None,
                sound_level: 0,
                wave: "sounds/ambient/soundscapes/day_lp.vsnd".to_owned(),
            }
        );
        assert_eq!(
            day.entries[2],
            SoundscapeEntry::Random {
                volume: (0.35, 0.5),
                pitch: (95.0, 105.0),
                time: (6.0, 9.0),
                origin: None,
                sound_level: 0,
                waves: vec![
                    "sounds/ambient/soundscapes/b1.vsnd".to_owned(),
                    "sounds/ambient/soundscapes/b2.vsnd".to_owned(),
                    "sounds/ambient/soundscapes/b3.vsnd".to_owned(),
                ],
            }
        );
        assert!(day.entries[0].is_omitted(&["spawn_l".to_owned()]));
        assert!(!day.entries[2].is_omitted(&["b1".to_owned()]));
    }

    #[test]
    fn placed_soundscapes_filters_disabled_and_invalid_entities() {
        let active = Value::object([
            ("classname", Value::from("env_soundscape")),
            ("soundscape", Value::from("default_day")),
            ("origin", Value::from_array([280.0, -144.0, 649.0])),
            ("radius", Value::from(16000)),
            ("StartDisabled", Value::from(false)),
        ]);
        let disabled = Value::object([
            ("classname", Value::from("env_soundscape")),
            ("soundscape", Value::from("default_night")),
            ("origin", Value::from_array([0.0, 0.0, 0.0])),
            ("radius", Value::from(1000)),
            ("StartDisabled", Value::from(true)),
        ]);
        let bad_name = Value::object([
            ("classname", Value::from("env_soundscape")),
            ("soundscape", Value::from("bad name!")),
            ("origin", Value::from_array([0.0, 0.0, 0.0])),
            ("radius", Value::from(1000)),
        ]);
        let placed = placed_soundscapes(&[active, disabled, bad_name]);
        assert_eq!(
            placed,
            [PlacedSoundscape {
                name: "default_day".to_owned(),
                origin: [280.0, -144.0, 649.0],
                radius: 16000.0,
            }]
        );
    }

    #[test]
    fn placed_soundscapes_supports_string_encoded_fields() {
        let str_space = Value::object([
            ("classname", Value::from("env_soundscape")),
            ("soundscape", Value::from("space_origin")),
            ("origin", Value::from("10.5 -20 30.25")),
            ("radius", Value::from("512.5")),
            ("StartDisabled", Value::from("0")),
        ]);
        let str_comma = Value::object([
            ("classname", Value::from("env_soundscape")),
            ("soundscape", Value::from("comma_origin")),
            ("origin", Value::from("1, 2, 3;")),
            ("radius", Value::from("128")),
            ("StartDisabled", Value::from("false")),
        ]);
        let disabled_one = Value::object([
            ("classname", Value::from("env_soundscape")),
            ("soundscape", Value::from("off_one")),
            ("origin", Value::from("0 0 0")),
            ("radius", Value::from("100")),
            ("StartDisabled", Value::from("1")),
        ]);
        let disabled_true = Value::object([
            ("classname", Value::from("env_soundscape")),
            ("soundscape", Value::from("off_true")),
            ("origin", Value::from("0 0 0")),
            ("radius", Value::from("100")),
            ("StartDisabled", Value::from("True")),
        ]);
        let placed = placed_soundscapes(&[str_space, str_comma, disabled_one, disabled_true]);
        assert_eq!(
            placed,
            [
                PlacedSoundscape {
                    name: "space_origin".to_owned(),
                    origin: [10.5, -20.0, 30.25],
                    radius: 512.5,
                },
                PlacedSoundscape {
                    name: "comma_origin".to_owned(),
                    origin: [1.0, 2.0, 3.0],
                    radius: 128.0,
                },
            ]
        );
    }

    #[test]
    fn read_soundscapes_bounds_empty_rndwave_amplification_metadata_and_volume_overflow() {
        // Nested soundscapes referencing a leaf with thousands of empty rndwave blocks
        // must finish linearly because each scape is pre-parsed once under MAX_WORK.
        let mut script = String::from("\"leaf\"\n{\n\"playrandom\"\n{\n\"time\" \"1, 2\"\n");
        for _ in 0..2_000 {
            script.push_str("\"rndwave\" {}\n");
        }
        script.push_str("\"rndwave\" { \"wave\" \"sounds/a.vsnd\" }\n}\n}\n");
        for i in 0..32 {
            script.push_str(&format!(
                "\"ref_{i}\" {{ \"playsoundscape\" {{ \"name\" \"leaf\" \"volume\" \"1\" }} }}\n"
            ));
        }

        // Metadata keys like fadetime/dsp must not count against MAX_ENTRIES.
        script.push_str(
            "\"meta_heavy\"\n{\n\"fadetime\" \"1.5\"\n\"dsp\" \"1\"\n\"dsp_spatial\" \"40\"\n",
        );
        for i in 0..MAX_ENTRIES {
            script.push_str(&format!(
                "\"playlooping\" {{ \"volume\" \"0.5\" \"wave\" \"sounds/s{i}.vsnd\" }}\n"
            ));
        }
        script.push_str("}\n");

        // Non-finite volume * scale overflow is dropped instead of producing inf.
        script.push_str(
            "\"overflow_inner\" { \"playlooping\" { \"volume\" \"1e308\" \"wave\" \"sounds/x.vsnd\" } }\n\
             \"overflow_outer\" { \"playsoundscape\" { \"name\" \"overflow_inner\" \"volume\" \"1e10\" } }\n",
        );

        let scapes = read_soundscapes(&[&script]);
        assert!(scapes.get("leaf").unwrap().entries.is_empty());
        assert_eq!(scapes.get("meta_heavy").unwrap().entries.len(), MAX_ENTRIES);
        assert!(scapes.get("overflow_outer").unwrap().entries.is_empty());
    }
}
