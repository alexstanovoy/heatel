//! The portrait framings of Dota's `scripts/npc/portraits.txt`: where the HUD portrait's camera
//! and light stand for a model, and the background behind it.

use crate::error::{Result, malformed};

/// Deepest nesting read; the file has four levels.
const DEPTH_LIMIT: usize = 8;
/// Largest file read; Dota's is ~1.4 MB.
const MAX_FILE: usize = 16 << 20;
/// Bounds of what a framing may say, past which the entry is refused: colours are 0 to 255 a
/// channel, scales a multiple of a light, places and the far plane within a model's world,
/// angles a few turns, a field of view inside a half turn.
const MAX_COLOR: f32 = 255.0;
const MAX_SCALE: f32 = 64.0;
const MAX_PLACE: f32 = 100_000.0;
const MAX_ANGLE: f32 = 3_600.0;
/// The narrowest field of view taken, degrees (Dota's narrowest is 10).
const MIN_FOV: f32 = 1.0;
/// The shortest way of the ambient light taken; Dota's are some 100 long.
const MIN_WAY: f32 = 1e-3;
/// Most a background's corner colour multiplies its image by, a channel (Dota's reach 2).
pub const MAX_TINT: f32 = 4.0;
/// A far plane must lie past this many inches: nearer, no portrait camera sees past its own near
/// plane (dbota's is at 4, twice it the floor both refuse at).
pub const MIN_FAR: f32 = 8.0;

/// A Valve KV1 value: a string, or a block of keyed values.
enum Kv {
    Text(String),
    Block(Vec<(String, Kv)>),
}

impl Kv {
    fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Self::Block(items) => items
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Self::Text(_) => None,
        }
    }

    fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Self::Text(t) => Some(t),
            Self::Block(_) => None,
        }
    }
}

struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    fn next(&mut self) -> Result<Option<(bool, &'a str)>> {
        loop {
            self.rest = self.rest.trim_start();
            if let Some(after) = self.rest.strip_prefix("//") {
                self.rest = after.split_once('\n').map_or("", |(_, tail)| tail);
            } else {
                break;
            }
        }
        let mut chars = self.rest.chars();
        match chars.next() {
            None => Ok(None),
            Some(c @ ('{' | '}')) => {
                let (head, tail) = self.rest.split_at(c.len_utf8());
                self.rest = tail;
                Ok(Some((false, head)))
            }
            Some('"') => {
                let body = &self.rest[1..];
                let end = body
                    .find('"')
                    .ok_or_else(|| malformed("portraits", "an unterminated string"))?;
                self.rest = &body[end + 1..];
                Ok(Some((true, &body[..end])))
            }
            Some(_) => Err(malformed("portraits", "an unquoted token")),
        }
    }
}

fn block(tokens: &mut Tokens<'_>, depth: usize) -> Result<Vec<(String, Kv)>> {
    if depth > DEPTH_LIMIT {
        return Err(malformed("portraits", "blocks nested too deep"));
    }
    let mut items = Vec::new();
    loop {
        let key = match tokens.next()? {
            None if depth == 0 => return Ok(items),
            Some((false, "}")) if depth > 0 => return Ok(items),
            Some((true, key)) => key.to_owned(),
            _ => return Err(malformed("portraits", "a key was expected")),
        };
        let value = match tokens.next()? {
            Some((true, text)) => Kv::Text(text.to_owned()),
            Some((false, "{")) => Kv::Block(block(tokens, depth + 1)?),
            _ => return Err(malformed("portraits", "a value was expected")),
        };
        items.push((key, value));
    }
}

/// Most entries of the `Portraits` block read (Dota's has some thousand).
const MAX_ENTRIES: usize = 1 << 16;
/// Most characters of the file's own text an error repeats.
const SAID: usize = 64;

/// At most [`SAID`] characters of `text`, for an error to repeat.
fn clip(text: &str) -> String {
    match text.char_indices().nth(SAID) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text.to_owned(),
    }
}

/// The entries of the file's `Portraits` block, by their key lower-cased (the first of a key
/// kept): each the text inside its braces. The file is scanned by its braces and quotes alone, in
/// one pass, so an entry that is no KeyValues inside is found (and refused when read) and the
/// next one is found after it: a string not closed on its line ends there, a stray word is
/// passed over. Its own text is not read here.
///
/// # Errors
///
/// No `Portraits` block, more than [`MAX_ENTRIES`] entries in it, or braces that do not pair:
/// the block closing early (a brace too many, with text after it) or never (one too few). Each
/// names the last entry opened, near which the brace is; the index is never cut short silently.
fn entries(text: &str) -> Result<std::collections::HashMap<String, &str>> {
    let bytes = text.as_bytes();
    let (mut i, mut depth) = (0, 0usize);
    let mut last_quoted: Option<&str> = None;
    let mut last_entry: Option<&str> = None;
    let mut portraits = false;
    let mut open: Option<(String, usize)> = None;
    let mut out = std::collections::HashMap::new();
    let mut seen = 0usize;
    let near = |entry: Option<&str>| entry.map_or_else(|| "the block's start".to_owned(), clip);
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
                continue;
            }
            b'"' => {
                // To the closing quote, or the line's end for one not closed on its line: the
                // search stops at the first of them, so every byte is looked at once.
                let end = text[i + 1..]
                    .find(['"', '\n'])
                    .map_or(bytes.len(), |n| i + 1 + n);
                last_quoted = Some(&text[i + 1..end]);
                i = end + 1;
                continue;
            }
            b'{' => {
                depth += 1;
                match (depth, portraits) {
                    (1, false) => {
                        portraits = last_quoted.is_some_and(|k| k.eq_ignore_ascii_case("Portraits"))
                    }
                    (2, true) => {
                        seen += 1;
                        if seen > MAX_ENTRIES {
                            return Err(malformed("portraits", "more entries than any file has"));
                        }
                        last_entry = last_quoted;
                        open = last_quoted.map(|k| (k.to_lowercase(), i + 1));
                    }
                    _ => {}
                }
                last_quoted = None;
            }
            b'}' => {
                if depth == 2
                    && let Some((key, start)) = open.take()
                {
                    out.entry(key).or_insert(&text[start..i]);
                }
                if depth == 1 && portraits {
                    if !only_comments(&text[i + 1..]) {
                        return Err(malformed(
                            "portraits",
                            format!(
                                "the Portraits block closes early, with text after it: a brace \
                                 too many in or after the entry {}",
                                near(last_entry)
                            ),
                        ));
                    }
                    return Ok(out);
                }
                depth = depth.saturating_sub(1);
                last_quoted = None;
            }
            _ => {}
        }
        i += 1;
    }
    Err(malformed(
        "portraits",
        if portraits {
            format!(
                "the Portraits block never closes: a brace too few in or after the entry {}",
                near(last_entry)
            )
        } else {
            "no Portraits block".to_owned()
        },
    ))
}

/// Whether `text` holds nothing but white space and `//` comments.
fn only_comments(text: &str) -> bool {
    text.lines().all(|line| {
        let line = line.trim_start();
        line.is_empty() || line.starts_with("//")
    })
}

/// An entry's text as KeyValues, or why it is not.
fn read_entry(body: &str) -> Result<Kv> {
    Ok(Kv::Block(block(&mut Tokens { rest: body }, 0)?))
}

/// A model's entry of the file and the file's `default` entry: what the model's entry leaves out
/// is the `default` entry's, key by key, its default camera's too.
struct Entry<'a> {
    label: &'a str,
    own: &'a Kv,
    default: Option<&'a Kv>,
}

impl Entry<'_> {
    fn text(&self, key: &str) -> Option<&str> {
        [Some(self.own), self.default]
            .into_iter()
            .flatten()
            .find_map(|kv| kv.text(key))
    }

    fn camera_text(&self, key: &str) -> Option<&str> {
        fn camera<'k>(kv: &'k Kv, key: &str) -> Option<&'k str> {
            kv.get("cameras")?.get("default")?.text(key)
        }
        camera(self.own, key).or_else(|| camera(self.default?, key))
    }

    fn camera_numbers<const N: usize>(
        &self,
        key: &str,
        bound: (f32, bool),
    ) -> Refused<Option<[f32; N]>> {
        self.numbers(key, self.camera_text(key), bound)
    }

    fn refuse(&self, key: &str, why: impl std::fmt::Display) -> Refusal {
        Refusal {
            label: self.label.to_owned(),
            why: format!("{key} {why}"),
        }
    }

    /// The first `N` numbers of `text` (the key's value), each within `-max..=max` (or
    /// `0..=max` when `unsigned`); none without the key.
    fn numbers<const N: usize>(
        &self,
        key: &str,
        text: Option<&str>,
        (max, unsigned): (f32, bool),
    ) -> Refused<Option<[f32; N]>> {
        let Some(text) = text else {
            return Ok(None);
        };
        let mut words = text.split_whitespace();
        let mut out = [0.0; N];
        for slot in &mut out {
            let n: f32 = words
                .next()
                .and_then(|w| w.parse().ok())
                .ok_or_else(|| self.refuse(key, format!("is not {N} numbers: {:?}", clip(text))))?;
            let low = if unsigned { 0.0 } else { -max };
            if !n.is_finite() || !(low..=max).contains(&n) {
                return Err(self.refuse(key, format!("{n} is outside {low} to {max}")));
            }
            *slot = n;
        }
        Ok(Some(out))
    }

    fn own<const N: usize>(&self, key: &str, bound: (f32, bool)) -> Refused<Option<[f32; N]>> {
        self.numbers(key, self.text(key), bound)
    }

    fn needed<const N: usize>(&self, key: &str, bound: (f32, bool)) -> Refused<[f32; N]> {
        self.own(key, bound)?
            .ok_or_else(|| self.refuse(key, "is set by neither the entry nor the default"))
    }

    fn one(&self, key: &str, bound: (f32, bool), or: f32) -> Refused<f32> {
        Ok(self.own::<1>(key, bound)?.map_or(or, |[n]| n))
    }

    fn camera(&self) -> Refused<Camera> {
        let needed = |key: &str, n: Option<[f32; 3]>| {
            n.ok_or_else(|| self.refuse(key, "is in neither the entry's camera nor the default's"))
        };
        let fov = self
            .camera_numbers::<1>("PortraitFOV", (179.0, true))?
            .ok_or_else(|| self.refuse("PortraitFOV", "is in neither camera"))?;
        if fov[0] < MIN_FOV {
            return Err(self.refuse("PortraitFOV", format!("{} is under {MIN_FOV}", fov[0])));
        }
        let place = (MAX_PLACE, false);
        Ok(Camera {
            position: needed(
                "PortraitPosition",
                self.camera_numbers("PortraitPosition", place)?,
            )?,
            angles: needed(
                "PortraitAngles",
                self.camera_numbers("PortraitAngles", (MAX_ANGLE, false))?,
            )?,
            fov: fov[0],
            far: self.far()?,
        })
    }

    /// The far plane; entries without one see as far as the biggest model needs.
    fn far(&self) -> Refused<f32> {
        let far = self
            .camera_numbers::<1>("PortraitFar", (MAX_PLACE, true))?
            .map_or(10_000.0, |[f]| f);
        if far <= MIN_FAR {
            return Err(self.refuse("PortraitFar", format!("{far} is not past {MIN_FAR}")));
        }
        Ok(far)
    }

    fn light(&self) -> Refused<PortraitLight> {
        let color = (MAX_COLOR, true);
        let scale = (MAX_SCALE, true);
        let direction: [f32; 3] = self.needed("PortraitAmbientDirection", (MAX_PLACE, false))?;
        let way = direction.iter().map(|c| c * c).sum::<f32>().sqrt();
        if way < MIN_WAY {
            return Err(self.refuse("PortraitAmbientDirection", format!("is {way} long: no way")));
        }
        let fov = self.needed::<1>("PortraitLightFOV", (179.0, true))?[0];
        if fov < MIN_FOV {
            return Err(self.refuse("PortraitLightFOV", format!("{fov} is under {MIN_FOV}")));
        }
        Ok(PortraitLight {
            position: self.needed("PortraitLightPosition", (MAX_PLACE, false))?,
            angles: self.needed("PortraitLightAngles", (MAX_ANGLE, false))?,
            fov,
            distance: self.one("PortraitLightDistance", (MAX_PLACE, true), 0.0)?,
            color: self.needed("PortraitLightColor", color)?,
            ambient: self.needed("PortraitAmbientColor", color)?,
            // Dota's brightness of each light; plain where unset.
            scale: self.one("PortraitLightScale", scale, 1.0)?,
            ambient_scale: self.one("PortraitAmbientScale", scale, 1.0)?,
            ambient_direction: direction,
            shadow: self.own("PortraitShadowColor", color)?.unwrap_or([0.0; 3]),
            shadow_scale: self.one("PortraitShadowScale", scale, 1.0)?,
        })
    }

    /// The background: its material and the four corner colours, white where unset.
    fn background(&self) -> Refused<Option<Background>> {
        let Some(vmat) = self.text("PortraitBackgroundTexture").map(str::trim) else {
            return Ok(None);
        };
        if !vmat.ends_with(".vmat") {
            let said = clip(vmat);
            return Err(self.refuse(
                "PortraitBackgroundTexture",
                format!("{said:?} is no material"),
            ));
        }
        let mut tints = WHITE;
        for (i, slot) in tints.iter_mut().enumerate() {
            let key = format!("PortraitBackgroundColor{}", i + 1);
            if let Some(c) = self.own::<3>(&key, (MAX_TINT, true))? {
                *slot = c;
            }
        }
        Ok(Some(Background {
            material: vmat.to_owned(),
            tints,
        }))
    }

    /// The framing, and its background left out when that alone does not read (the portrait
    /// then stands on none, framed and lit as Dota says).
    fn framing(&self) -> Refused<(Framing, Option<Refusal>)> {
        let hide = self.one("PortraitHideParticles", (1.0, true), 0.0)?;
        let (background, left_out) = match self.background() {
            Ok(background) => (background, None),
            Err(why) => (None, Some(why)),
        };
        let framing = Framing {
            label: self.label.to_owned(),
            camera: self.camera()?,
            light: self.light()?,
            background,
            hide_particles: hide != 0.0,
        };
        Ok((framing, left_out))
    }
}

/// The colours Dota tints a portrait's background with where the file sets none: white, which
/// leaves the image as it is.
pub const WHITE: [[f32; 3]; 4] = [[1.0; 3]; 4];

/// The portrait camera (in the model's own inches and degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Where it stands.
    pub position: [f32; 3],
    /// Pitch, yaw and roll.
    pub angles: [f32; 3],
    /// Its field of view across the picture's width, degrees: Source's `CViewSetup::fov`
    /// ("horizontal FOV in degrees"), the projection taking the height from the picture's
    /// aspect.
    pub fov: f32,
    /// Its far plane (10000 where the entry sets none).
    pub far: f32,
}

/// The portrait's light, its ambient light and its shadow colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortraitLight {
    /// Where the light stands.
    pub position: [f32; 3],
    /// Its pitch, yaw and roll.
    pub angles: [f32; 3],
    /// Its field of view.
    pub fov: f32,
    /// How far it is from what it lights (`PortraitLightDistance`; 0 where unset).
    pub distance: f32,
    /// Its colour, 0 to 255 a channel.
    pub color: [f32; 3],
    /// The ambient colour, 0 to 255 a channel.
    pub ambient: [f32; 3],
    /// How bright the light is (1 where unset).
    pub scale: f32,
    /// How bright the ambient colour is (1 where unset).
    pub ambient_scale: f32,
    /// The way the ambient light goes, in the model's axes (`PortraitAmbientDirection`: the
    /// older file's `PortraitAmbientOrigin` with its signs turned).
    pub ambient_direction: [f32; 3],
    /// The colour of the side turned from the ambient light, 0 to 255 a channel.
    pub shadow: [f32; 3],
    /// How bright the shadow colour is.
    pub shadow_scale: f32,
}

/// The image behind a portrait.
#[derive(Clone, Debug, PartialEq)]
pub struct Background {
    /// Its material (`materials/.../x.vmat`); the image is its `g_tColor`.
    pub material: String,
    /// The colours each corner is tinted with (top left, top right, bottom right, bottom left):
    /// what the image is multiplied by, 0 to 4 a channel (1 leaves it as it is; Dota's reach 2).
    pub tints: [[f32; 3]; 4],
}

/// A model's portrait framing as Dota defines it.
#[derive(Clone, Debug, PartialEq)]
pub struct Framing {
    /// The caller's name for the model.
    pub label: String,
    /// The default camera.
    pub camera: Camera,
    /// The light.
    pub light: PortraitLight,
    /// The background, when the entry names one.
    pub background: Option<Background>,
    /// Whether the portrait leaves out the particles the model wears (`PortraitHideParticles`).
    pub hide_particles: bool,
}

/// A wanted model's entry refused: the model's label and why (the key, and what of it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The caller's name for the model.
    pub label: String,
    /// What of its entry does not read, at most 64 characters of the file's text repeated.
    pub why: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.label, self.why)
    }
}

type Refused<T> = std::result::Result<T, Refusal>;

/// The framings `framings` read, and the entries it refused, each said with its model and key.
#[derive(Debug, Default)]
pub struct Framed {
    /// The wanted models' framings, in the order wanted.
    pub framings: Vec<Framing>,
    /// Why each refused entry was: a wanted model's entry that lacks a number both it and the
    /// default leave out, or says one out of bounds (a colour past 0 to 255, a scale past 64, a
    /// field of view under 1 degree, a far plane not past 8, an ambient light of no way, an entry
    /// that is no KeyValues).
    pub refused: Vec<Refusal>,
    /// The backgrounds left out of framings that read without them: a background that is no
    /// `.vmat` material, or of a tint past 4.
    pub left_out: Vec<Refusal>,
}

/// Dota's framings of the wanted models out of `portraits.txt`, in the order of `wanted` (pairs
/// of a label and the model's `.vmdl` path; `default` names the file's `default` entry itself). A
/// model Dota has no entry for is left out; an entry that does not read is refused alone, the
/// others read. One rule fills what an entry leaves out: the file's `default` entry's, key by
/// key, and only where that has none either the plain values (a far plane of 10000, scales of 1,
/// a black shadow, no light distance, particles shown).
///
/// # Errors
///
/// The file past 16 MiB, not UTF-8 or not KeyValues, or with no `Portraits` block.
pub fn framings(portraits: &[u8], wanted: &[(&str, &str)]) -> Result<Framed> {
    if portraits.len() > MAX_FILE {
        return Err(malformed("portraits", format!("{} bytes", portraits.len())));
    }
    let text = std::str::from_utf8(portraits)
        .map_err(|_| malformed("portraits", "the file is not UTF-8"))?;
    let index = entries(text)?;
    let mut out = Framed::default();
    let default = match index.get("default").map(|body| read_entry(body)) {
        Some(Ok(kv)) => Some(kv),
        Some(Err(why)) => {
            out.refused.push(Refusal {
                label: "default".to_owned(),
                why: format!("is no KeyValues: {why}"),
            });
            None
        }
        None => None,
    };
    for (label, vmdl) in wanted {
        let Some(body) = index.get(&vmdl.to_lowercase()) else {
            continue;
        };
        let own = match read_entry(body) {
            Ok(kv) => kv,
            Err(why) => {
                out.refused.push(Refusal {
                    label: (*label).to_owned(),
                    why: format!("is no KeyValues: {why}"),
                });
                continue;
            }
        };
        let entry = Entry {
            label,
            own: &own,
            default: default.as_ref(),
        };
        match entry.framing() {
            Ok((framing, left_out)) => {
                out.framings.push(framing);
                out.left_out.extend(left_out);
            }
            Err(why) => out.refused.push(why),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Background, Camera, Framing, PortraitLight, framings};

    const FILE: &str = r#"
"Portraits" // comment
{
    "default" { "PortraitLightAngles" "1 2 3" "PortraitShadowColor" "74 74 74" "PortraitAmbientDirection" "-79.07 -84.15 -25.32" }
    "models/a.vmdl"
    {
        "PortraitLightPosition" "1 2 3"
        "PortraitLightAngles" "49.61 263.39 0.00"
        "PortraitLightFOV" "90"
        "PortraitLightDistance" "195"
        "PortraitShadowScale" "2"
        "PortraitLightColor" "231 254 187"
        "PortraitAmbientColor" "97 114 0"
        "PortraitLightScale" "2.8"
        "PortraitAmbientScale" "5"
        "PortraitBackgroundTexture" "materials/vgui/hud/heroportraits/portraitbackground_desert.vmat"
        "PortraitBackgroundColor3" ".8 .2 .1"
        "cameras" { "default" { "PortraitPosition" "162.31 63.73 144.54" "PortraitAngles" "2.75 199.94 0" "PortraitFOV" "25" } }
    }
}"#;

    #[test]
    fn a_model_gets_its_default_camera_light_and_background_and_the_files_default_shading() {
        let framed = framings(
            FILE.as_bytes(),
            &[("a", "models/a.vmdl"), ("b", "models/b.vmdl")],
        )
        .unwrap()
        .framings;
        assert_eq!(framed.len(), 1, "the model without a framing has no line");
        let framing = Framing {
            label: "a".to_owned(),
            camera: Camera {
                position: [162.31, 63.73, 144.54],
                angles: [2.75, 199.94, 0.0],
                fov: 25.0,
                far: 10_000.0,
            },
            light: PortraitLight {
                position: [1.0, 2.0, 3.0],
                angles: [49.61, 263.39, 0.0],
                fov: 90.0,
                distance: 195.0,
                color: [231.0, 254.0, 187.0],
                ambient: [97.0, 114.0, 0.0],
                scale: 2.8,
                ambient_scale: 5.0,
                ambient_direction: [-79.07, -84.15, -25.32],
                shadow: [74.0, 74.0, 74.0],
                shadow_scale: 2.0,
            },
            background: Some(Background {
                material: "materials/vgui/hud/heroportraits/portraitbackground_desert.vmat"
                    .to_owned(),
                tints: [[1.0; 3], [1.0; 3], [0.8, 0.2, 0.1], [1.0; 3]],
            }),
            hide_particles: false,
        };
        assert_eq!(framed[0], framing);
    }

    /// One rule for every key: what a model's entry leaves out is the `default` entry's, its
    /// default camera's too; the plain values stand only where the default has none either.
    const SPARSE: &str = r#"
"Portraits"
{
    "default"
    {
        "PortraitLightPosition" "10 20 30" "PortraitLightAngles" "54.79 171.70 0.00"
        "PortraitLightFOV" "69" "PortraitLightColor" "254 248 242" "PortraitLightScale" "4.5"
        "PortraitAmbientColor" "79 108 108" "PortraitAmbientDirection" "-79.07 -84.15 -25.32"
        "PortraitBackgroundTexture" "materials/vgui/hud/heroportraits/portraitbackground_gradient_top.vmat"
        "PortraitHideParticles" "1"
        "cameras" { "default" { "PortraitPosition" "446.17 -88.29 -22.69" "PortraitAngles" "334.89 166.83 0.00" "PortraitFOV" "16" } }
    }
    "models/sparse.vmdl"
    {
        "PortraitAmbientColor" "97 114 0"
        "PortraitHideParticles" "0"
        "cameras" { "default" { "PortraitFOV" "25" } }
    }
}"#;

    #[test]
    fn what_a_model_leaves_out_is_the_default_entrys_for_every_key() {
        let framed = framings(SPARSE.as_bytes(), &[("s", "models/sparse.vmdl")])
            .unwrap()
            .framings;
        let f = &framed[0];
        assert_eq!(f.camera.position, [446.17, -88.29, -22.69]);
        assert_eq!(f.camera.fov, 25.0, "the model's own field of view");
        assert_eq!(f.light.color, [254.0, 248.0, 242.0]);
        assert_eq!(f.light.scale, 4.5, "the default's scale, not 1");
        assert_eq!(f.light.ambient, [97.0, 114.0, 0.0]);
        assert_eq!(f.light.ambient_scale, 1.0, "neither sets it");
        assert_eq!(f.light.shadow, [0.0; 3], "neither sets it");
        assert!(!f.hide_particles, "the model's own 0 over the default's 1");
        let background = f.background.as_ref().unwrap();
        assert!(background.material.ends_with("gradient_top.vmat"));
        let default = framings(SPARSE.as_bytes(), &[("d", "default")])
            .unwrap()
            .framings;
        assert!(
            default[0].hide_particles,
            "the default entry is a framing too"
        );
    }

    /// An entry of a wanted model saying a number out of its bounds is refused, named with the
    /// model and the key; one of a model nobody wants is not read.
    #[test]
    fn out_of_bounds_numbers_are_refused_by_model_and_key() {
        let cases = [
            (
                "\"PortraitFOV\" \"25\"",
                "\"PortraitFOV\" \"25\" \"PortraitFar\" \"8\"",
                "PortraitFar",
            ),
            (
                "\"PortraitFOV\" \"25\"",
                "\"PortraitFOV\" \"25\" \"PortraitFar\" \"0\"",
                "PortraitFar",
            ),
            (
                "\"PortraitHideParticles\" \"0\"",
                "\"PortraitAmbientDirection\" \"1e-30 0 0\"",
                "PortraitAmbientDirection",
            ),
            (
                "\"PortraitFOV\" \"25\"",
                "\"PortraitFOV\" \"1e-30\"",
                "PortraitFOV",
            ),
            (
                "\"PortraitAmbientColor\" \"97 114 0\"",
                "\"PortraitAmbientColor\" \"300 0 0\"",
                "PortraitAmbientColor",
            ),
            (
                "\"PortraitHideParticles\" \"0\"",
                "\"PortraitLightScale\" \"1e30\"",
                "PortraitLightScale",
            ),
            (
                "\"PortraitHideParticles\" \"0\"",
                "\"PortraitAmbientDirection\" \"0 0 0\"",
                "PortraitAmbientDirection",
            ),
            (
                "\"PortraitAmbientColor\" \"97 114 0\"",
                "\"PortraitAmbientColor\" \"97 114\"",
                "PortraitAmbientColor",
            ),
            (
                "\"PortraitAmbientColor\" \"97 114 0\"",
                "\"PortraitAmbientColor\" \"NaN 0 0\"",
                "PortraitAmbientColor",
            ),
        ];
        for (from, to, key) in cases {
            let file = SPARSE.replacen(from, to, 1);
            assert_ne!(file, SPARSE, "{to}");
            let both = [("s", "models/sparse.vmdl"), ("d", "default")];
            let read = framings(file.as_bytes(), &both).unwrap();
            let said: Vec<String> = read.refused.iter().map(ToString::to_string).collect();
            assert!(
                said.len() == 1 && said[0].contains("s: ") && said[0].contains(key),
                "{to}: {said:?}"
            );
            assert_eq!(read.framings.len(), 1, "{to}: the other entry still reads");
            assert!(framings(file.as_bytes(), &[]).unwrap().framings.is_empty());
        }
        let bright = SPARSE.replacen(
            "\"PortraitHideParticles\" \"0\"",
            "\"PortraitBackgroundColor1\" \"2 2 .5\"",
            1,
        );
        let framed = framings(bright.as_bytes(), &[("s", "models/sparse.vmdl")])
            .unwrap()
            .framings;
        let tints = framed[0].background.as_ref().unwrap().tints;
        assert_eq!(
            tints[0],
            [2.0, 2.0, 0.5],
            "a tint past 1 brightens, as Dota's do"
        );
        let first_three = SPARSE.replacen("\"97 114 0\"", "\"97 114 0 1e30 x\"", 1);
        let framed = framings(first_three.as_bytes(), &[("s", "models/sparse.vmdl")])
            .unwrap()
            .framings;
        assert_eq!(
            framed[0].light.ambient,
            [97.0, 114.0, 0.0],
            "only the first three are read"
        );
    }

    /// A background that does not read (no material, a tint past 4) is left out alone: the model
    /// keeps its camera and light, and stands on no background.
    #[test]
    fn a_background_that_does_not_read_is_left_out_alone() {
        for (to, key) in [
            (
                "\"PortraitBackgroundColor1\" \"5 1 1\"",
                "PortraitBackgroundColor1",
            ),
            (
                "\"PortraitBackgroundTexture\" \"materials/a.png\"",
                "PortraitBackgroundTexture",
            ),
        ] {
            let file = SPARSE.replacen("\"PortraitHideParticles\" \"0\"", to, 1);
            let read = framings(file.as_bytes(), &[("s", "models/sparse.vmdl")]).unwrap();
            assert!(read.refused.is_empty(), "{to}: {:?}", read.refused);
            let said: Vec<String> = read.left_out.iter().map(ToString::to_string).collect();
            assert!(
                said.len() == 1 && said[0].contains("s: ") && said[0].contains(key),
                "{to}: {said:?}"
            );
            let framing = &read.framings[0];
            assert!(framing.background.is_none(), "{to}: no background");
            assert_eq!(framing.camera.fov, 25.0, "{to}: its own camera");
        }
    }

    /// An entry that is no KeyValues inside (a key with no value, a stray word, a string not
    /// closed on its line) is refused alone by its model; the entries after it are found and
    /// read. What the error repeats of the file is cut short.
    #[test]
    fn an_entry_that_is_no_keyvalues_is_refused_alone() {
        let long = "x".repeat(10_000);
        for broken in [
            "\"PortraitAmbientColor\"".to_owned(),
            "word \"PortraitAmbientColor\" \"1 2 3\"".to_owned(),
            format!("\"PortraitAmbientColor\" \"{long}\n"),
            format!("\"PortraitAmbientColor\" \"{long}\""),
        ] {
            let file = SPARSE.replacen(
                "\"models/sparse.vmdl\"",
                &format!("\"models/broken.vmdl\" {{ {broken} }}\n    \"models/sparse.vmdl\""),
                1,
            );
            let wanted = [("b", "models/broken.vmdl"), ("s", "models/sparse.vmdl")];
            let read = framings(file.as_bytes(), &wanted).unwrap();
            assert_eq!(read.framings.len(), 1, "{broken:.40}: the next entry read");
            assert_eq!(read.refused.len(), 1, "{broken:.40}: {:?}", read.refused);
            assert_eq!(read.refused[0].label, "b");
            assert!(
                read.refused[0].why.len() < 200,
                "{}",
                read.refused[0].why.len()
            );
        }
    }

    #[test]
    fn hostile_text_is_an_error() {
        let deep = "\"a\" {".repeat(100);
        assert!(framings(deep.as_bytes(), &[]).is_err());
        assert!(framings(b"\"Portraits\" { \"k\" ", &[]).is_err());
    }

    /// A brace too many in an entry closes the block early, and one too few leaves it open: the
    /// file is an error naming the entry, never an index that silently lacks the entries after it.
    #[test]
    fn a_stray_or_missing_brace_is_an_error_naming_the_entry() {
        let wanted = [("s", "models/sparse.vmdl")];
        let broken = |entry: &str| {
            let file = SPARSE.replacen(
                "\"models/sparse.vmdl\"",
                &format!("\"models/broken.vmdl\" {entry}\n    \"models/sparse.vmdl\""),
                1,
            );
            framings(file.as_bytes(), &wanted)
                .map(|read| read.framings.len())
                .unwrap_err()
                .to_string()
        };
        let stray = broken("{ \"PortraitFOV\" \"25\" } }");
        assert!(
            stray.contains("closes early") && stray.contains("models/broken.vmdl"),
            "{stray}"
        );
        let missing = broken("{ \"PortraitFOV\" \"25\"");
        assert!(
            missing.contains("never closes") && missing.contains("models/broken.vmdl"),
            "{missing}"
        );
    }
}
