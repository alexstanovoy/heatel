//! `items_game.txt`: the hero cosmetics read from any bytes. No input panics, and the items it
//! keeps stay within the bounds of the reader.
#![no_main]

use heatel::items_game::read_hero_items;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(items) = read_hero_items(data) {
        for item in &items {
            let _ = item.is_default();
            assert!(!item.heroes.is_empty());
        }
    }
});
