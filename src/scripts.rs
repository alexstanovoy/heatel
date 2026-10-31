//! The items and abilities an install's scripts define: each one's name, the texture its icon
//! is named after, the model it lies as on the ground, and the particle systems it precaches.
//! Read out of the plain KeyValues text of `scripts/npc/items.txt`, `npc_abilities.txt` and the
//! heroes' scripts, so a client can import every one of them without a list of its own.

use crate::error::{Result, malformed};
use crate::keyvalues::{Kv, parse_kv_repeats, text};

/// Most entries one script lists: Dota's `items.txt` holds a few hundred and `npc_abilities.txt`
/// a few thousand.
pub const MAX_ENTRIES: usize = 20_000;

/// Most precached particles read of one entry.
const MAX_PARTICLES: usize = 64;

/// One item or ability a script defines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// The block's name: `item_blink`, `nevermore_requiem`.
    pub name: String,
    /// `AbilityTextureName`, when the script gives one (an item's icon is named after its name
    /// without `item_` otherwise).
    pub texture: Option<String>,
    /// `Model`: the model an item lies as on the ground, a `.vmdl` path.
    pub model: Option<String>,
    /// The `particle` entries of its `precache` block, `.vpcf` paths.
    pub particles: Vec<String>,
    /// Whether it is an item (`ItemCost` is given) rather than an ability.
    pub item: bool,
}

impl Entry {
    /// The name its icon is stored under: `AbilityTextureName`, else the name without `item_`
    /// for an item, else the name itself.
    pub fn icon_name(&self) -> &str {
        self.texture.as_deref().unwrap_or_else(|| {
            if self.item {
                self.name.strip_prefix("item_").unwrap_or(&self.name)
            } else {
                &self.name
            }
        })
    }
}

/// Every item and ability block of a script, in file order, the first of each name.
pub fn entries(script: &[u8]) -> Result<Vec<Entry>> {
    let tree = parse_kv_repeats(text(script)?)?;
    let mut out: Vec<Entry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<(&str, &Kv)> = Vec::new();
    push_block(&mut stack, &tree);
    while let Some((name, block)) = stack.pop() {
        if block.block().is_none() {
            continue;
        }
        let holds = ["AbilityValues", "ItemCost", "AbilityBehavior"]
            .iter()
            .any(|key| block.get(key).is_some());
        if holds && seen.insert(name) {
            if out.len() >= MAX_ENTRIES {
                return Err(malformed(
                    "scripts",
                    format!("more than {MAX_ENTRIES} entries"),
                ));
            }
            out.push(entry(name, block));
        }
        push_block(&mut stack, block);
    }
    Ok(out)
}

/// Pushes a node's children so that they pop in file order.
fn push_block<'a>(stack: &mut Vec<(&'a str, &'a Kv)>, node: &'a Kv) {
    stack.extend(
        node.block()
            .unwrap_or_default()
            .iter()
            .rev()
            .map(|(k, v)| (k.as_str(), v)),
    );
}

fn entry(name: &str, block: &Kv) -> Entry {
    let string = |key: &str| match block.get(key) {
        Some(Kv::Text(text)) if !text.is_empty() => Some(text.clone()),
        _ => None,
    };
    let particles = block
        .get("precache")
        .and_then(Kv::block)
        .unwrap_or_default()
        .iter()
        .filter(|(key, _)| key == "particle")
        .filter_map(|(_, value)| match value {
            Kv::Text(path) => Some(path.clone()),
            Kv::Block(_) => None,
        })
        .take(MAX_PARTICLES)
        .collect();
    Entry {
        name: name.to_owned(),
        texture: string("AbilityTextureName"),
        model: string("Model"),
        particles,
        item: block.get("ItemCost").is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = r#""DOTAAbilities"
{
    "Version" "1"
    "item_blink"
    {
        "ItemCost" "2250"
        "Model" "models/props_gameplay/blink.vmdl"
        "precache"
        {
            "particle" "particles/items_fx/blink_dagger_start.vpcf"
            "particle" "particles/items_fx/blink_dagger_end.vpcf"
            "soundfile" "soundevents/game_sounds_items.vsndevts"
        }
    }
    "item_recipe_x"
    {
        "ItemCost" "100"
        "AbilityTextureName" "item_recipe"
    }
    "nevermore_requiem"
    {
        "AbilityBehavior" "DOTA_ABILITY_BEHAVIOR_NO_TARGET"
        "AbilityTextureName" "nevermore_requiem"
    }
    "nevermore_shadowraze1"
    {
        "AbilityValues" { "damage" "90" }
    }
    "not_an_ability" { "Foo" "bar" }
}
"#;

    #[test]
    fn items_and_abilities_come_out_with_their_icons_models_and_particles() {
        let entries = entries(SCRIPT.as_bytes()).expect("a script");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "item_blink",
                "item_recipe_x",
                "nevermore_requiem",
                "nevermore_shadowraze1"
            ]
        );
        assert_eq!(entries[0].icon_name(), "blink");
        assert_eq!(
            entries[0].model.as_deref(),
            Some("models/props_gameplay/blink.vmdl")
        );
        assert_eq!(entries[0].particles.len(), 2);
        assert_eq!(entries[1].icon_name(), "item_recipe");
        assert!(!entries[2].item && entries[3].icon_name() == "nevermore_shadowraze1");
    }
}
