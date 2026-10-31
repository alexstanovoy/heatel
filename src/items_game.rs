//! The hero cosmetics of `scripts/items/items_game.txt` (52 MB of KeyValues): for each item a
//! hero wears, its slot, its model and its `visuals` asset modifiers. The default cosmetics
//! (`prefab "default_item"`) are what a hero looks like with nothing equipped: Shadow Fiend's
//! head, shoulders and arms, and the ambient effects (`particle_create`) he carries. Other
//! items replace parts of that: a `particle` or `sound` modifier names the base effect or sound
//! event it takes the place of.
//!
//! The file is read as a stream of tokens and only these items are kept, so memory follows the
//! cosmetics, not the file. The text is untrusted: sizes and nesting are bounded.

use crate::error::{Result, malformed};
use crate::keyvalues::{Token, tokens};

/// Largest file read (the install's is ~52 MB).
const MAX_FILE: usize = 128 << 20;
/// Most items kept (the install has ~15 000 with a hero).
const MAX_ITEMS: usize = 1 << 17;
/// Most asset modifiers one item has, and heroes it is used by.
const MAX_PER_ITEM: usize = 1 << 10;
/// Deepest block nesting read.
const MAX_DEPTH: usize = 64;
/// Most further members one asset modifier keeps.
const MAX_EXTRA: usize = 64;

/// An item a hero wears.
#[derive(Debug, Clone, PartialEq)]
pub struct HeroItem {
    /// The item's id (`387`).
    pub id: String,
    /// Its name (`Shadow Fiend's Head`).
    pub name: String,
    /// The prefab it builds on; `default_item` marks a hero's default cosmetic.
    pub prefab: Option<String>,
    /// The slot it fills (`head`, `shoulder`, `ambient_effects`, `hero_base`).
    pub slot: Option<String>,
    /// The model it wears (`model_player`).
    pub model: Option<String>,
    /// The folder its particles are in (`particle_folder`).
    pub particle_folder: Option<String>,
    /// The heroes that can wear it (`npc_dota_hero_nevermore`).
    pub heroes: Vec<String>,
    /// What it changes, in file order.
    pub modifiers: Vec<AssetModifier>,
}

impl HeroItem {
    /// Whether it is a default cosmetic.
    pub fn is_default(&self) -> bool {
        self.prefab
            .as_deref()
            .is_some_and(|p| p.split_whitespace().any(|p| p == "default_item"))
    }
}

/// One of an item's `visuals` entries.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetModifier {
    /// What it does.
    pub kind: ModifierKind,
    /// What it applies to: the base particle or sound event replaced, the model, or `ALL`.
    pub asset: Option<String>,
    /// What it brings: the particle started, the replacement, the activity modifier.
    pub modifier: Option<String>,
    /// Every other member of the entry (`control_number`, `cp_position`), in file order.
    pub extra: Vec<(String, String)>,
}

/// What an asset modifier does.
#[derive(Debug, Clone, PartialEq)]
pub enum ModifierKind {
    /// `particle_create`: starts a particle system (`modifier`) with the hero.
    ParticleCreate,
    /// `particle`: replaces the particle system `asset` with `modifier`.
    Particle,
    /// `particle_control_point`: sets a control point of the particle system `asset`.
    ParticleControlPoint,
    /// `sound`: replaces the sound event `asset` with `modifier`.
    Sound,
    /// `activity`: gives the hero's animations the activity modifier `modifier`.
    Activity,
    /// Any other type, by its name.
    Other(String),
}

/// The items of `items_game.txt` that a hero wears, in file order.
pub fn read_hero_items(bytes: &[u8]) -> Result<Vec<HeroItem>> {
    if bytes.len() > MAX_FILE {
        return Err(malformed("items_game", format!("{} bytes", bytes.len())));
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|e| malformed("items_game", e.to_string()))?;
    let mut reader = Reader::default();
    for token in tokens(text) {
        match token {
            Token::Open => reader.open()?,
            Token::Close => reader.close(),
            Token::Text(s) => reader.text(s),
        }
    }
    Ok(reader.items)
}

/// Where the stream is: the keys of the blocks open, the key waiting for its value, and the
/// item and modifier being read.
#[derive(Default)]
struct Reader {
    path: Vec<String>,
    key: Option<String>,
    item: Option<HeroItem>,
    modifier: Option<AssetModifier>,
    items: Vec<HeroItem>,
}

impl Reader {
    /// Whether the open blocks lie inside an entry of `items` (the root is the file's own).
    fn in_item(&self) -> bool {
        self.path.len() >= 3 && self.path[1] == "items"
    }

    fn open(&mut self) -> Result<()> {
        if self.path.len() >= MAX_DEPTH {
            return Err(malformed("items_game", "blocks nested too deep"));
        }
        self.path.push(self.key.take().unwrap_or_default());
        if self.path.len() == 3 && self.path[1] == "items" {
            self.item = Some(HeroItem {
                id: self.path[2].clone(),
                name: String::new(),
                prefab: None,
                slot: None,
                model: None,
                particle_folder: None,
                heroes: Vec::new(),
                modifiers: Vec::new(),
            });
        } else if self.in_item()
            && self.path.len() == 5
            && self.path[3] == "visuals"
            && self.path[4].starts_with("asset_modifier")
        {
            self.modifier = Some(AssetModifier {
                kind: ModifierKind::Other(String::new()),
                asset: None,
                modifier: None,
                extra: Vec::new(),
            });
        }
        Ok(())
    }

    fn close(&mut self) {
        self.key = None;
        if self.in_item() && self.path.len() == 5 {
            let item = self
                .item
                .as_mut()
                .filter(|i| i.modifiers.len() < MAX_PER_ITEM);
            if let (Some(m), Some(item)) = (self.modifier.take(), item) {
                item.modifiers.push(m);
            }
        } else if self.in_item() && self.path.len() == 3 {
            let item = self.item.take().filter(|i| !i.heroes.is_empty());
            if let Some(item) = item.filter(|_| self.items.len() < MAX_ITEMS) {
                self.items.push(item);
            }
        }
        self.path.pop();
    }

    fn text(&mut self, s: &str) {
        let Some(key) = self.key.take() else {
            self.key = Some(s.to_owned());
            return;
        };
        if !self.in_item() {
            return;
        }
        match (
            self.path.len(),
            self.path[3.min(self.path.len() - 1)].as_str(),
        ) {
            (3, _) => self.item_field(&key, s),
            (4, "used_by_heroes") if s != "0" => {
                if let Some(item) = self.item.as_mut().filter(|i| i.heroes.len() < MAX_PER_ITEM) {
                    item.heroes.push(key);
                }
            }
            (5, "visuals") => {
                if let Some(m) = self.modifier.as_mut() {
                    m.set(key, s);
                }
            }
            _ => {}
        }
    }

    fn item_field(&mut self, key: &str, value: &str) {
        let Some(item) = self.item.as_mut() else {
            return;
        };
        let value = value.to_owned();
        match key {
            "name" => item.name = value,
            "prefab" => item.prefab = Some(value),
            "item_slot" => item.slot = Some(value),
            "model_player" => item.model = Some(value),
            "particle_folder" => item.particle_folder = Some(value),
            _ => {}
        }
    }
}

impl AssetModifier {
    fn set(&mut self, key: String, value: &str) {
        match key.as_str() {
            "type" => {
                self.kind = match value {
                    "particle_create" => ModifierKind::ParticleCreate,
                    "particle" => ModifierKind::Particle,
                    "particle_control_point" => ModifierKind::ParticleControlPoint,
                    "sound" => ModifierKind::Sound,
                    "activity" => ModifierKind::Activity,
                    other => ModifierKind::Other(other.to_owned()),
                }
            }
            "asset" => self.asset = Some(value.to_owned()),
            "modifier" => self.modifier = Some(value.to_owned()),
            _ if self.extra.len() < MAX_EXTRA => self.extra.push((key, value.to_owned())),
            _ => {}
        }
    }
}
