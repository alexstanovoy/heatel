//! The tooltip texts of items and abilities, made from a localization and the scripts.

use heatel::tooltips::{Wanted, tooltips};

fn wanted(names: &[&str]) -> Wanted {
    Wanted {
        names: names.iter().map(|name| (*name).to_owned()).collect(),
        heroes: Vec::new(),
    }
}

/// Some items carry their name under a form key (`<key>:n`) with no plain key, and the rest of
/// their texts under the plain keys: the item has a name, labels and notes all the same.
#[test]
fn an_item_named_only_by_its_n_form_still_has_its_name_labels_and_notes() {
    let localization = b"\
\"DOTA_Tooltip_ability_item_stone:n\" \"Stone\"\n\
\"DOTA_Tooltip_ability_item_stone_bonus_health\" \"+$health\"\n\
\"DOTA_Tooltip_ability_item_stone_Note0\" \"Does not stack.\"\n\
\"DOTA_Ability_Variable_health\" \"Health\"\n";
    let script = b"\"DOTAAbilities\" { \"item_stone\" { \"ItemCost\" \"900\" \
        \"AbilityValues\" { \"bonus_health\" \"150\" } } }";
    let found = tooltips(&wanted(&["item_stone"]), localization, &[script]).unwrap();
    let stone = found
        .get("item_stone")
        .expect("the item is named by its :n key");
    assert_eq!(stone.name, "Stone");
    assert_eq!(
        stone.labels,
        [("bonus_health".to_owned(), "+Health".to_owned())]
    );
    assert_eq!(stone.notes, ["Does not stack."]);
    assert_eq!(stone.cost.as_deref(), Some("900"));
}

/// When both keys exist, the plain one names the item, as before the form key was read.
#[test]
fn an_item_with_both_name_keys_takes_the_plain_one() {
    let localization = b"\
\"DOTA_Tooltip_ability_item_stone\" \"Plain\"\n\
\"DOTA_Tooltip_ability_item_stone:n\" \"Form\"\n";
    let found = tooltips(
        &wanted(&["item_stone"]),
        localization,
        &[b"\"DOTAAbilities\" {}"],
    )
    .unwrap();
    assert_eq!(found["item_stone"].name, "Plain");
}

/// A name with no key of either kind has no tooltip.
#[test]
fn a_name_with_no_key_has_no_tooltip() {
    let found = tooltips(&wanted(&["item_stone"]), b"", &[b"\"DOTAAbilities\" {}"]).unwrap();
    assert!(found.is_empty());
}
