//! Dota's sound mixer definitions (`scripts/soundmixers.txt`): the mix groups a sound belongs to
//! by its file's path, the levels each mix gives them, and the layers that change those levels
//! while another group sounds (the announcer ducking the world).

use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::kv3::{Object, Value, parse_text_in};

/// Most mix groups, mixes, levels in one and layers a file holds.
const MAX_GROUPS: usize = 256;
const MAX_MIXES: usize = 16;
const MAX_LAYERS: usize = 64;
const MAX_TRIGGERS: usize = 16;

/// A mix group: the sounds whose path holds `path_substring` (and whose entity class holds
/// `class_substring`) belong to it. The first group of the file that matches wins: the `Creeps`
/// group (`creep/shared`, priority 40) comes before `Weapons` (`weapons`, 50) in Valve's file
/// and would never be reached by the sounds in `weapons/creep/shared/` if priority decided.
/// `priority` is kept for what the file means it for.
#[derive(Debug, Clone, PartialEq)]
pub struct MixGroup {
    /// The name.
    pub name: String,
    /// The text a sound file path must hold.
    pub path_substring: String,
    /// The text the sound source's class must hold.
    pub class_substring: String,
    /// The lowest sound level (dB) a sound must have to belong, if the group states one.
    pub sound_level_min: Option<f32>,
    /// The group's priority, as the file states it.
    pub priority: i32,
    /// Whether other groups may duck it.
    pub is_ducked: bool,
    /// Whether it ducks others.
    pub causes_ducking: bool,
    /// The percent it is ducked to.
    pub duck_to_percent: f32,
    /// The threshold at which it ducks.
    pub ducker_threshold: f32,
}

/// What a mix or a layer does to one group.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupLevel {
    /// The group it applies to.
    pub group: String,
    /// The volume multiplier.
    pub volume: f32,
    /// The sound level multiplier, which scales the level distance falls off by.
    pub level: f32,
    /// How far it is muted, 0 to 1.
    pub mute: f32,
}

/// A mix: a name and the level of each group.
#[derive(Debug, Clone, PartialEq)]
pub struct Mix {
    /// The name.
    pub name: String,
    /// The level of each group.
    pub levels: Vec<GroupLevel>,
}

/// What switches a layer on: the volume of the `trigger` group passing `threshold`.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerTrigger {
    /// The trigger group.
    pub trigger: String,
    /// The volume the trigger group must pass.
    pub threshold: f32,
    /// The mix amount.
    pub amount: f32,
    /// Seconds to reach full effect, and to let go of it.
    pub attack: f32,
    /// Seconds to let go.
    pub release: f32,
}

/// A layer: the levels it sets while one of its triggers holds.
#[derive(Debug, Clone, PartialEq)]
pub struct MixLayer {
    /// The name.
    pub name: String,
    /// The level of each group.
    pub levels: Vec<GroupLevel>,
    /// What switches it on.
    pub triggers: Vec<LayerTrigger>,
}

/// The whole of a mixer file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SoundMixer {
    /// The mix groups, in file order.
    pub groups: Vec<MixGroup>,
    /// The mixes.
    pub mixes: Vec<Mix>,
    /// The layers.
    pub layers: Vec<MixLayer>,
}

impl SoundMixer {
    /// The group a sound belongs to: the first group of the file whose path substring the
    /// sound's `path` holds, whose class substring `class` holds, and whose lowest level
    /// `sound_level` reaches. A group with an empty substring matches anything.
    pub fn group_of(&self, path: &str, class: &str, sound_level: f32) -> Option<&MixGroup> {
        self.groups.iter().find(|group| {
            path.contains(&group.path_substring)
                && class.contains(&group.class_substring)
                && group.sound_level_min.is_none_or(|min| sound_level >= min)
        })
    }
}

/// Reads `scripts/soundmixers.txt` within `budget`. Counts are bounded, and a number that is not
/// finite refuses the file. The mixer copies texts of the parsed document, each once at most, so
/// what the parse took is charged again before it is built.
pub fn read_sound_mixer(bytes: &[u8], budget: &mut Budget) -> Result<SoundMixer> {
    let before = budget.spent();
    let document = parse_text_in(bytes, budget)?;
    budget.charge(budget.spent() - before)?;
    let root = document
        .root
        .as_object()
        .ok_or_else(|| malformed("soundmixers", "not an object"))?;
    let groups = list(root, "MixGroups", MAX_GROUPS)?
        .iter()
        .map(group)
        .collect::<Result<Vec<_>>>()?;
    let mixes = match root.get("SoundMixers").and_then(Value::as_object) {
        Some(mixes) => {
            if mixes.len() > MAX_MIXES {
                return Err(malformed("soundmixers", "more mixes than any file has"));
            }
            mixes
                .iter()
                .map(|(name, levels)| {
                    Ok(Mix {
                        name: name.to_owned(),
                        levels: levels_of(levels)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?
        }
        None => Vec::new(),
    };
    let layers = match root.get("MixLayers").and_then(Value::as_object) {
        Some(layers) => {
            if layers.len() > MAX_LAYERS {
                return Err(malformed("soundmixers", "more layers than any file has"));
            }
            layers
                .iter()
                .map(|(name, layer)| layer_of(name, layer))
                .collect::<Result<Vec<_>>>()?
        }
        None => Vec::new(),
    };
    Ok(SoundMixer {
        groups,
        mixes,
        layers,
    })
}

fn list<'a>(object: &'a Object, key: &str, most: usize) -> Result<&'a [Value]> {
    let list = object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("soundmixers", format!("no {key} list")))?;
    if list.len() > most {
        return Err(malformed(
            "soundmixers",
            format!("more {key} than any file has"),
        ));
    }
    Ok(list)
}

fn number(object: &Object, key: &str, default: f32) -> Result<f32> {
    match object.get(key) {
        None => Ok(default),
        Some(value) => match value.as_f64() {
            Some(n) if n.is_finite() && (n as f32).is_finite() => Ok(n as f32),
            Some(n) if n.is_finite() => Err(malformed(
                "soundmixers",
                format!("{key} does not fit a number"),
            )),
            _ => Err(malformed("soundmixers", format!("{key} is not a number"))),
        },
    }
}

fn text(object: &Object, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn group(value: &Value) -> Result<MixGroup> {
    let o = value
        .as_object()
        .ok_or_else(|| malformed("soundmixers", "a mix group is not an object"))?;
    let name = text(o, "name");
    if name.is_empty() {
        return Err(malformed("soundmixers", "a mix group has no name"));
    }
    Ok(MixGroup {
        name,
        path_substring: text(o, "path_substring"),
        class_substring: text(o, "class_substring"),
        sound_level_min: o
            .get("sndlvl_min")
            .map(|_| number(o, "sndlvl_min", 0.0))
            .transpose()?,
        priority: number(o, "priority", 0.0)? as i32,
        is_ducked: number(o, "is_ducked", 0.0)? != 0.0,
        causes_ducking: number(o, "causes_ducking", 0.0)? != 0.0,
        duck_to_percent: number(o, "duck_to_percent", 100.0)?,
        ducker_threshold: number(o, "ducker_threshold", 0.0)?,
    })
}

fn levels_of(value: &Value) -> Result<Vec<GroupLevel>> {
    let list = value
        .as_array()
        .ok_or_else(|| malformed("soundmixers", "levels are not a list"))?;
    if list.len() > MAX_GROUPS {
        return Err(malformed("soundmixers", "more levels than any file has"));
    }
    list.iter()
        .map(|level| {
            let o = level
                .as_object()
                .ok_or_else(|| malformed("soundmixers", "a level is not an object"))?;
            Ok(GroupLevel {
                group: text(o, "mixgroup"),
                volume: number(o, "vol", 1.0)?,
                level: number(o, "lvl", 1.0)?,
                mute: number(o, "mute", 0.0)?,
            })
        })
        .collect()
}

fn layer_of(name: &str, value: &Value) -> Result<MixLayer> {
    let o = value
        .as_object()
        .ok_or_else(|| malformed("soundmixers", "a layer is not an object"))?;
    let triggers = list(o, "Triggers", MAX_TRIGGERS)?
        .iter()
        .map(|trigger| {
            let t = trigger
                .as_object()
                .ok_or_else(|| malformed("soundmixers", "a trigger is not an object"))?;
            Ok(LayerTrigger {
                trigger: text(t, "trigger"),
                threshold: number(t, "threshold", 0.0)?,
                amount: number(t, "mixamount", 1.0)?,
                attack: number(t, "attack", 0.0)?.max(0.0),
                release: number(t, "release", 0.0)?.max(0.0),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(MixLayer {
        name: name.to_owned(),
        levels: o
            .get("Mixers")
            .map(levels_of)
            .transpose()?
            .unwrap_or_default(),
        triggers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"<!-- kv3 -->
{
  MixGroups = [
    { name = "creeps" path_substring = "creep/shared" priority = 40 },
    { name = "weapons" path_substring = "weapons" priority = 50 },
    { name = "attack" path_substring = "attack" priority = 50 },
    { name = "ui" path_substring = "ui/" priority = 60 },
    { name = "all" path_substring = "" sndlvl_min = 1 priority = 30 },
  ]
  SoundMixers = { Default_Mix = [ { mixgroup = "weapons" vol = 0.4 lvl = 1 mute = 0 } ] }
  MixLayers = {
    Announcer = {
      Mixers = [ { mixgroup = "weapons" vol = 0.7 } ]
      Triggers = [ { trigger = "ui" threshold = 0.4 mixamount = 1.0 attack = 0.3 release = 0.5 } ]
    }
  }
}"#;

    #[test]
    fn a_sound_belongs_to_the_first_group_of_the_file_that_matches() {
        let mixer = read_sound_mixer(FILE.as_bytes(), &mut crate::budget::Budget::default())
            .expect("a mixer");
        let of = |path: &str, level| mixer.group_of(path, "", level).map(|g| g.name.as_str());
        assert_eq!(of("sounds/weapons/hero/x.vsnd", 75.0), Some("weapons"));
        assert_eq!(of("sounds/weapons/attack/x.vsnd", 75.0), Some("weapons"));
        assert_eq!(of("sounds/ui/x.vsnd", 75.0), Some("ui"));
        assert_eq!(
            of("sounds/ui/weapons.vsnd", 75.0),
            Some("weapons"),
            "file order, not priority"
        );
        assert_eq!(
            of("sounds/weapons/creep/shared/a.vsnd", 75.0),
            Some("creeps"),
            "a lower priority listed first still wins"
        );
        assert_eq!(of("sounds/other/a.vsnd", 75.0), Some("all"));
        assert_eq!(
            of("sounds/other/a.vsnd", 0.0),
            None,
            "below the catch-all's level"
        );
    }

    #[test]
    fn mixes_and_layers_read_with_their_triggers() {
        let mixer = read_sound_mixer(FILE.as_bytes(), &mut crate::budget::Budget::default())
            .expect("a mixer");
        assert_eq!(mixer.mixes[0].name, "Default_Mix");
        assert_eq!(mixer.mixes[0].levels[0].volume, 0.4);
        let layer = &mixer.layers[0];
        assert_eq!(layer.levels[0].volume, 0.7);
        assert_eq!(layer.triggers[0].attack, 0.3);
        assert_eq!(layer.triggers[0].release, 0.5);
    }

    #[test]
    fn a_volume_that_is_not_a_number_refuses_the_file() {
        let bad = FILE.replace("vol = 0.4", "vol = \"loud\"");
        let err =
            read_sound_mixer(bad.as_bytes(), &mut crate::budget::Budget::default()).unwrap_err();
        assert!(err.to_string().contains("vol is not a number"), "{err}");
    }

    #[test]
    fn a_volume_too_large_for_a_number_refuses_the_file_naming_the_field() {
        let bad = FILE.replace("vol = 0.4", "vol = 1e300");
        let err =
            read_sound_mixer(bad.as_bytes(), &mut crate::budget::Budget::default()).unwrap_err();
        assert!(
            err.to_string().contains("vol does not fit a number"),
            "{err}"
        );
    }

    #[test]
    fn more_groups_than_any_file_has_refuse_the_file() {
        let many = format!(
            "{{ MixGroups = [ {} ] }}",
            "{ name = \"g\" } ".repeat(MAX_GROUPS + 1)
        );
        let err =
            read_sound_mixer(many.as_bytes(), &mut crate::budget::Budget::default()).unwrap_err();
        assert!(err.to_string().contains("more MixGroups"), "{err}");
    }
}
