//! The tooltip texts: the localization (the input's first file) and the item and ability
//! scripts (the rest), read for the first names the localization's own keys give. No input
//! panics.
#![no_main]

use heatel::fuzzing::files;
use heatel::keyvalues::{localization_table, text};
use heatel::tooltips::{Wanted, tooltips};
use libfuzzer_sys::fuzz_target;

/// The localization key prefix of a tooltip's name.
const PREFIX: &str = "dota_tooltip_ability_";

fuzz_target!(|data: &[u8]| {
    let parts: Vec<&[u8]> = files::split(data).into_iter().map(|(_, b)| b).collect();
    let mut names: Vec<String> = text(parts[0])
        .map(localization_table)
        .unwrap_or_default()
        .keys()
        .filter_map(|k| k.strip_prefix(PREFIX))
        .map(str::to_owned)
        .collect();
    names.sort();
    names.truncate(64);
    let wanted = Wanted {
        names,
        heroes: Vec::new(),
    };
    let _ = tooltips(&wanted, parts[0], &parts[1..]);
});
