//! Ordered Dota shop categories, neutral crafting tiers, item definitions, and recipes.
//!
//! [`categories`] reads the original shop layout. [`catalog`] combines it with the
//! gameplay item definitions and neutral crafting pools.

use std::collections::BTreeMap;

use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::keyvalues::{Kv, Token, parse_kv_in, text, tokens};

/// Most categories kept.
const MAX_SHOPS: usize = 64;
/// Most items kept of one category.
const MAX_ITEMS: usize = 128;

/// A shop category and the items it lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    /// The category's key (`consumables`, `sideshop1`, ...).
    pub name: String,
    /// Its items (`item_<name>`), in file order (at most 128).
    pub items: Vec<String>,
}

/// The categories of `shops.txt` in file order (at most 64): the blocks under its root, each
/// with the `item` values it lists. A category named twice keeps its first list.
pub fn categories(shops_txt: &[u8]) -> Result<Vec<Category>> {
    Ok(read(text(shops_txt)?)?
        .into_iter()
        .map(|(name, items)| Category { name, items })
        .collect())
}

/// The categories of `text` (`shops.txt`'s): the blocks under its root, each with the `item`
/// values it lists in order. A category named twice keeps its first list; deeper blocks are
/// skipped.
fn read(text: &str) -> Result<Vec<(String, Vec<String>)>> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut depth = 0usize;
    // The key read and waiting for its value or block.
    let mut key: Option<&str> = None;
    // The category whose block is open, by its index in `out`, if it is kept.
    let mut open: Option<usize> = None;
    for token in tokens(text) {
        match token {
            Token::Open => {
                depth += 1;
                if depth == 2 {
                    open = key
                        .filter(|name| !out.iter().any(|(n, _)| n == name))
                        .filter(|_| out.len() < MAX_SHOPS)
                        .map(|name| {
                            out.push((name.to_owned(), Vec::new()));
                            out.len() - 1
                        });
                }
                key = None;
            }
            Token::Close => {
                if depth == 0 {
                    return Err(malformed("shops", "a block closes that never opened"));
                }
                if depth == 2 {
                    open = None;
                }
                depth -= 1;
                key = None;
            }
            Token::Text(word) => match key.take() {
                None => key = Some(word),
                Some(k) => {
                    if depth == 2
                        && k == "item"
                        && let Some(at) = open
                        && out[at].1.len() < MAX_ITEMS
                    {
                        out[at].1.push(word.to_owned());
                    }
                }
            },
        }
    }
    Ok(out)
}

/// A group of categories in the shop layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShopTab {
    /// The four basic item categories.
    Basic,
    /// The six upgrade categories.
    Upgrades,
    /// Secret Shop.
    Secret,
    /// The two side shop pages.
    Side,
    /// Pregame categories and game-mode-specific layouts.
    Other,
}

impl Category {
    /// The tab this category belongs to.
    pub fn tab(&self) -> ShopTab {
        match self.name.as_str() {
            "consumables" | "attributes" | "weapons_armor" | "misc" => ShopTab::Basic,
            "basics" | "support" | "magics" | "defense" | "weapons" | "artifacts" => {
                ShopTab::Upgrades
            }
            "secretshop" => ShopTab::Secret,
            "sideshop1" | "sideshop2" => ShopTab::Side,
            _ => ShopTab::Other,
        }
    }
}

/// One gameplay item defined in `scripts/npc/items.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Script ID used by the game, when supplied.
    pub id: Option<u32>,
    /// Gold cost listed by the item definition.
    pub cost: Option<u32>,
    /// Whether the item can be purchased according to its script.
    pub purchasable: bool,
    /// Whether the item definition is a recipe.
    pub recipe: bool,
    /// Whether the item requires the Secret Shop.
    pub secret_shop: bool,
    /// Whether the item is flagged as a neutral trinket.
    pub neutral_trinket: bool,
    /// Whether the item is flagged as a neutral enhancement.
    pub neutral_enhancement: bool,
    /// The tier encoded by a legacy neutral token item.
    pub neutral_token_tier: Option<u8>,
    /// The item's search tags, in script order.
    pub tags: Vec<String>,
}

/// An entry in a recipe's component list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    /// Item script name, with the recipe marker removed.
    pub item: String,
    /// The script marks this component with a trailing `*`.
    pub marked: bool,
}

/// One way to build an item. Repeated components represent multiple copies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirements {
    /// The original alternative key, such as `04`.
    pub key: String,
    /// The components for this alternative, in script order.
    pub components: Vec<Component>,
}

/// A recipe edge from its component alternatives to its resulting item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    /// The recipe's item script name.
    pub item: String,
    /// The resulting item script name.
    pub result: String,
    /// Whether the recipe consumes charges from its components.
    pub consumes_charges: bool,
    /// All alternatives, in script order. Disabled recipes can have an empty list.
    pub alternatives: Vec<Requirements>,
}

/// A weighted neutral item entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeutralChoice {
    /// Item script name.
    pub item: String,
    /// The item's selection weight.
    pub weight: u32,
}

/// One neutral trinket pool within a tier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeutralPool {
    /// The pool's script key.
    pub name: String,
    /// Number of options offered from this pool.
    pub options: Option<u32>,
    /// Weighted choices in file order.
    pub items: Vec<NeutralChoice>,
}

/// The neutral enhancements offered for an attribute or globally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnhancementGroup {
    /// The attribute key or `global`.
    pub name: String,
    /// Weighted choices in file order.
    pub items: Vec<NeutralChoice>,
}

/// A neutral crafting tier and its available trinkets and enhancements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeutralTier {
    /// Tier number.
    pub number: u8,
    /// Time the tier unlocks, in seconds.
    pub start_time: Option<u32>,
    /// Number of trinket options offered.
    pub trinket_options: Option<u32>,
    /// Number of enhancement options offered.
    pub enhancement_options: Option<u32>,
    /// Madstone cost to craft.
    pub craft_cost: Option<u32>,
    /// Madstone cost to recraft.
    pub recraft_cost: Option<u32>,
    /// Experience value from the tier's script.
    pub xp_bonus: Option<u32>,
    /// The tier's trinket pools in file order.
    pub pools: Vec<NeutralPool>,
    /// The tier's attribute and global enhancements in file order.
    pub enhancements: Vec<EnhancementGroup>,
}

/// Shop layout, gameplay item definitions, neutral tiers and recipe graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    /// Every shop category in file order.
    pub categories: Vec<Category>,
    /// All gameplay item definitions, including items absent from the shop.
    pub items: BTreeMap<String, Item>,
    /// Recipes indexed by the item they build, in definition order.
    pub recipes: BTreeMap<String, Vec<Recipe>>,
    /// Neutral tiers in file order.
    pub neutral_tiers: Vec<NeutralTier>,
    /// Time the neutral Madstone limit is lifted, in seconds.
    pub madstone_no_limit_time: Option<u32>,
}

impl Catalog {
    /// Shop categories in the given tab, in file order.
    pub fn categories_in(&self, tab: ShopTab) -> impl Iterator<Item = &Category> {
        self.categories
            .iter()
            .filter(move |category| category.tab() == tab)
    }

    /// Recipes that build this item. Follow their components to traverse the graph.
    pub fn recipes_for(&self, item: &str) -> &[Recipe] {
        self.recipes.get(item).map_or(&[], Vec::as_slice)
    }

    /// Recipes that use this component or consume this recipe scroll.
    pub fn recipes_using<'a>(&'a self, item: &'a str) -> impl Iterator<Item = &'a Recipe> {
        self.recipes.values().flatten().filter(move |recipe| {
            (recipe.item == item
                && self.items.get(item).is_some_and(|entry| {
                    entry.purchasable && entry.cost.is_some_and(|cost| cost > 0)
                }))
                || recipe
                    .alternatives
                    .iter()
                    .flat_map(|alt| &alt.components)
                    .any(|part| part.item == item)
        })
    }
}

/// Reads `scripts/shops.txt`, `scripts/npc/items.txt` and `scripts/npc/neutral_items.txt`.
pub fn catalog(shops: &[u8], items: &[u8], neutral_items: &[u8]) -> Result<Catalog> {
    let mut output = Catalog {
        categories: categories(shops)?,
        ..Catalog::default()
    };
    let mut budget = Budget::named("shop catalog");
    let items = parse_kv_in(text(items)?, &mut budget)?;
    for (name, node) in children(items.get("DOTAAbilities")) {
        if !name.starts_with("item_") || node.block().is_none() {
            continue;
        }
        let item = item(node);
        if item.recipe
            && let Some(recipe) = recipe(name, node)
        {
            output
                .recipes
                .entry(recipe.result.clone())
                .or_default()
                .push(recipe);
        }
        output.items.insert(name.clone(), item);
    }
    let neutral = parse_kv_in(text(neutral_items)?, &mut budget)?;
    if let Some(root) = neutral.get("neutral_items") {
        output.neutral_tiers = children(root.get("neutral_tiers"))
            .iter()
            .filter_map(|(number, node)| Some(neutral_tier(number.parse().ok()?, node)))
            .collect();
        output.madstone_no_limit_time = root
            .get("madstone_limits")
            .and_then(|limits| time(limits, "madstone_no_limit_time"));
    }
    Ok(output)
}

fn children(node: Option<&Kv>) -> &[(String, Kv)] {
    node.and_then(Kv::block).unwrap_or_default()
}

fn field<'a>(node: &'a Kv, name: &str) -> Option<&'a str> {
    node.get(name)?.as_str().map(str::trim)
}

fn number(node: &Kv, key: &str) -> Option<u32> {
    field(node, key)?.parse().ok()
}

fn flag(node: &Kv, key: &str) -> bool {
    field(node, key).is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

fn list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn item(node: &Kv) -> Item {
    Item {
        id: number(node, "ID"),
        cost: number(node, "ItemCost"),
        purchasable: node.get("ItemPurchasable").is_none() || flag(node, "ItemPurchasable"),
        recipe: flag(node, "ItemRecipe"),
        secret_shop: flag(node, "SecretShop"),
        neutral_trinket: flag(node, "ItemIsNeutralActiveDrop"),
        neutral_enhancement: flag(node, "ItemIsNeutralPassiveDrop"),
        neutral_token_tier: field(node, "TokenTier").and_then(|value| value.parse().ok()),
        tags: list(field(node, "ItemShopTags").unwrap_or_default())
            .map(str::to_owned)
            .collect(),
    }
}

fn recipe(name: &str, node: &Kv) -> Option<Recipe> {
    let result = field(node, "ItemResult").filter(|value| !value.is_empty())?;
    let alternatives = children(node.get("ItemRequirements"))
        .iter()
        .filter_map(|(key, value)| {
            Some(Requirements {
                key: key.clone(),
                components: list(value.as_str()?)
                    .map(|name| {
                        let base = name.strip_suffix('*');
                        Component {
                            item: base.unwrap_or(name).to_owned(),
                            marked: base.is_some(),
                        }
                    })
                    .collect(),
            })
        })
        .collect();
    Some(Recipe {
        item: name.to_owned(),
        result: result.to_owned(),
        consumes_charges: flag(node, "ItemRecipeConsumesCharges"),
        alternatives,
    })
}

fn choices(node: Option<&Kv>) -> Vec<NeutralChoice> {
    children(node)
        .iter()
        .filter_map(|(name, weight)| {
            Some(NeutralChoice {
                item: name.clone(),
                weight: weight.as_str()?.trim().parse().ok()?,
            })
        })
        .collect()
}

fn neutral_tier(tier: u8, node: &Kv) -> NeutralTier {
    NeutralTier {
        number: tier,
        start_time: time(node, "start_time"),
        trinket_options: number(node, "trinket_options"),
        enhancement_options: number(node, "enhancement_options"),
        craft_cost: number(node, "craft_cost"),
        recraft_cost: number(node, "recraft_cost"),
        xp_bonus: number(node, "xp_bonus"),
        pools: children(node.get("item_pools"))
            .iter()
            .map(|(name, pool)| NeutralPool {
                name: name.clone(),
                options: number(pool, "options"),
                items: choices(pool.get("items")),
            })
            .collect(),
        enhancements: children(node.get("enhancements"))
            .iter()
            .map(|(name, group)| EnhancementGroup {
                name: name.clone(),
                items: choices(Some(group)),
            })
            .collect(),
    }
}

fn time(node: &Kv, key: &str) -> Option<u32> {
    let (minutes, seconds) = field(node, key)?.split_once(':')?;
    let minutes: u32 = minutes.parse().ok()?;
    let seconds: u32 = seconds.parse().ok()?;
    (seconds < 60).then_some(())?;
    minutes.checked_mul(60)?.checked_add(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_alternatives_keep_order_repeats_markers_and_paid_scroll_edges() {
        let shops = br#""dota_shops" {
            "consumables" { "item" "item_a" "item" "item_b" "item" "item_a" }
            "secretshop" { "item" "item_result" }
        }"#;
        let items = br#""DOTAAbilities" {
            "item_a" { "ItemCost" "100" }
            "item_b" { "ItemCost" "200" "ItemPurchasable" "0" }
            "item_result" { "ItemCost" "500" "SecretShop" "1" }
            "item_recipe_result" {
                "ItemCost" "200" "ItemRecipe" "1" "ItemResult" "item_result"
                "ItemRequirements" { "04" "item_a*;item_a;item_b;" "01" "item_b;item_b" }
            }
        }"#;
        let read = catalog(shops, items, b"").unwrap();
        assert_eq!(
            read.categories_in(ShopTab::Basic).next().unwrap().items,
            ["item_a", "item_b", "item_a"]
        );
        assert_eq!(read.categories_in(ShopTab::Secret).count(), 1);
        let [recipe] = read.recipes_for("item_result") else {
            panic!("expected one recipe")
        };
        assert_eq!(recipe.alternatives[0].key, "04");
        let components = &recipe.alternatives[0].components;
        assert_eq!(
            components
                .iter()
                .map(|part| part.item.as_str())
                .collect::<Vec<_>>(),
            ["item_a", "item_a", "item_b"]
        );
        assert!(components[0].marked);
        assert_eq!(read.recipes_using("item_a").collect::<Vec<_>>(), [recipe]);
        assert_eq!(
            read.recipes_using("item_recipe_result").collect::<Vec<_>>(),
            [recipe]
        );
        assert_eq!(read.items["item_result"].cost, Some(500));
        assert!(!read.items["item_b"].purchasable);
    }

    #[test]
    fn neutral_tier_preserves_weights_attribute_groups_and_unlock_time() {
        let neutral = br#""neutral_items" {
            "madstone_limits" { "madstone_no_limit_time" "65:00" }
            "neutral_tiers" { "5" {
                "start_time" "60:15" "craft_cost" "14"
                "item_pools" { "pool_a" { "options" "1" "items" { "item_a" "3" "item_b" "1" } } }
                "enhancements" { "strength" { "item_enhancement" "2" } }
            } }
        }"#;
        let read = catalog(b"", b"", neutral).unwrap();
        let [tier] = &read.neutral_tiers[..] else {
            panic!("expected one tier")
        };
        assert_eq!(tier.number, 5);
        assert_eq!(tier.start_time, Some(3615));
        assert_eq!(tier.craft_cost, Some(14));
        assert_eq!(tier.pools[0].items[0].weight, 3);
        assert_eq!(tier.enhancements[0].name, "strength");
        assert_eq!(tier.enhancements[0].items[0].item, "item_enhancement");
        assert_eq!(read.madstone_no_limit_time, Some(3900));
    }
}
