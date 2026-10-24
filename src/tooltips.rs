//! The tooltip texts of Dota's items and abilities: their names, the shop search's aliases,
//! descriptions, lore, notes, number labels and Dota's own numbers, read out of the install's
//! localization and item/ability scripts (plain KeyValues text in the package).

use std::collections::{BTreeMap, HashMap};

use crate::error::Result;
use crate::keyvalues::{Kv, localization_table, parse_kv, text};

const LOCALIZATION: &str = "resource/localization/abilities_english.txt";
const PREFIX: &str = "dota_tooltip_ability_";
const VARIABLE: &str = "dota_ability_variable_";
/// The localization's search aliases of an item: `DOTA_SearchAlias_Ability_<name>`.
const ALIAS: &str = "dota_searchalias_ability_";
/// Most aliases kept of one name.
const MAX_ALIASES: usize = 16;

/// Whose texts to read: Dota names (`item_<name>` for items, the ability's name otherwise) and
/// the heroes whose ability scripts hold them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    /// The items' and abilities' names.
    pub names: Vec<String>,
    /// The heroes (`pudge`) whose `npc_dota_hero_<hero>.txt` scripts are searched too.
    pub heroes: Vec<String>,
}

impl Wanted {
    /// The install files the texts come from: the localization, then the scripts in the order
    /// their blocks are searched. Read each from the package and give them to [`tooltips`].
    pub fn sources(&self) -> Vec<String> {
        let mut heroes: Vec<String> = self
            .heroes
            .iter()
            .map(|h| format!("scripts/npc/heroes/npc_dota_hero_{h}.txt"))
            .collect();
        heroes.sort();
        heroes.dedup();
        let mut out = vec![
            LOCALIZATION.to_owned(),
            "scripts/npc/items.txt".to_owned(),
            "scripts/npc/npc_abilities.txt".to_owned(),
        ];
        out.extend(heroes);
        out
    }
}

/// An item's or ability's tooltip, as Dota's localization and scripts give it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tooltip {
    /// Its shown name.
    pub name: String,
    /// The description, when there is one.
    pub description: Option<String>,
    /// The lore line, when there is one.
    pub lore: Option<String>,
    /// The notes, in order.
    pub notes: Vec<String>,
    /// The other names the shop's search finds it by (at most 16).
    pub aliases: Vec<String>,
    /// The script's `AbilityValues`, each as the space-separated text of its levels, in script
    /// order.
    pub values: Vec<(String, String)>,
    /// `ItemCost`, from the script or its values.
    pub cost: Option<String>,
    /// `AbilityCooldown`, from the script or its values.
    pub cooldown: Option<String>,
    /// `AbilityManaCost`, from the script or its values.
    pub mana: Option<String>,
    /// `AbilityCastRange`, from the script or its values.
    pub range: Option<String>,
    /// The localization's label of each value that has one (`$variables` filled in), in
    /// the values' order.
    pub labels: Vec<(String, String)>,
}

/// The tooltips of `wanted`'s names that the localization has a name for, by name, from the
/// localization and the scripts (the files of [`Wanted::sources`], in that order).
pub fn tooltips(
    wanted: &Wanted,
    localization: &[u8],
    scripts: &[&[u8]],
) -> Result<BTreeMap<String, Tooltip>> {
    let loc = localization_table(text(localization)?);
    let variables: HashMap<&str, &str> = loc
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(VARIABLE)?, v.as_str())))
        .collect();
    let mut found: HashMap<&str, &Kv> = HashMap::new();
    let trees = scripts
        .iter()
        .map(|s| parse_kv(text(s)?))
        .collect::<Result<Vec<_>>>()?;
    for tree in &trees {
        find_blocks(tree, &wanted.names, &mut found);
    }
    let mut out = BTreeMap::new();
    for name in &wanted.names {
        let Some(title) = loc.get(&format!("{PREFIX}{name}")) else {
            continue;
        };
        let block = found.get(name.as_str()).copied();
        out.insert(name.clone(), entry(name, title, block, &loc, &variables));
    }
    Ok(out)
}

/// One name's tooltip.
fn entry(
    name: &str,
    title: &str,
    block: Option<&Kv>,
    loc: &HashMap<String, String>,
    variables: &HashMap<&str, &str>,
) -> Tooltip {
    let key = |suffix: &str| format!("{PREFIX}{name}{suffix}");
    let notes = (0..)
        .map_while(|n| loc.get(&key(&format!("_note{n}"))))
        .cloned()
        .collect();
    // The other names the shop's search finds it by, `;` between them.
    let aliases = loc
        .get(&format!("{ALIAS}{name}"))
        .map(|list| {
            list.split(';')
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .take(MAX_ALIASES)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let values: Vec<(String, String)> = block
        .and_then(|b| b.get("AbilityValues"))
        .and_then(Kv::block)
        .unwrap_or_default()
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), level_values(Some(v))?)))
        .collect();
    let field = |script_key: &str| {
        let own = level_values(block.and_then(|b| b.get(script_key))).filter(|s| !s.is_empty());
        own.or_else(|| {
            values
                .iter()
                .find(|(k, _)| k == script_key)
                .map(|(_, v)| v.clone())
        })
    };
    let labels = values
        .iter()
        .filter_map(|(k, _)| {
            let label = loc.get(&key(&format!("_{}", k.to_lowercase())))?;
            (!label.is_empty()).then(|| (k.clone(), substitute(label, variables)))
        })
        .collect();
    Tooltip {
        name: title.to_owned(),
        description: loc.get(&key("_description")).cloned(),
        lore: loc.get(&key("_lore")).cloned(),
        notes,
        aliases,
        cost: field("ItemCost"),
        cooldown: field("AbilityCooldown"),
        mana: field("AbilityManaCost"),
        range: field("AbilityCastRange"),
        values,
        labels,
    }
}

/// A script value as the space-separated text Dota prints: a string, or a block's `value`.
fn level_values(v: Option<&Kv>) -> Option<String> {
    match v? {
        Kv::Text(s) => Some(s.trim().to_owned()),
        Kv::Block(_) => match v?.get("value")? {
            Kv::Text(s) => Some(s.trim().to_owned()),
            Kv::Block(_) => None,
        },
    }
}

/// `$name` replaced by the localization's variable of that name, where there is one.
fn substitute(label: &str, variables: &HashMap<&str, &str>) -> String {
    let mut out = String::new();
    let mut rest = label;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let end = tail
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(tail.len());
        let word = &tail[..end];
        match variables.get(word) {
            Some(v) if !word.is_empty() => out.push_str(v),
            _ => {
                out.push('$');
                out.push_str(word);
            }
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The first block anywhere under `node` (depth first, in file order) for each wanted name
/// that holds an ability or item.
fn find_blocks<'a>(node: &'a Kv, wanted: &[String], found: &mut HashMap<&'a str, &'a Kv>) {
    let mut stack: Vec<(&'a str, &'a Kv)> = Vec::new();
    let push = |stack: &mut Vec<(&'a str, &'a Kv)>, node: &'a Kv| {
        stack.extend(
            node.block()
                .unwrap_or_default()
                .iter()
                .rev()
                .map(|(k, v)| (k.as_str(), v)),
        );
    };
    push(&mut stack, node);
    while let Some((k, v)) = stack.pop() {
        if v.block().is_none() {
            continue;
        }
        let holds = ["AbilityValues", "ItemCost", "AbilityBehavior"]
            .iter()
            .any(|key| v.get(key).is_some());
        if holds && !found.contains_key(k) && wanted.iter().any(|w| w == k) {
            found.insert(k, v);
        }
        push(&mut stack, v);
    }
}
