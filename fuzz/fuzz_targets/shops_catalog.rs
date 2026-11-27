//! Shop categories, recipes and neutral tiers from a multipart KeyValues input.
#![no_main]

use heatel::fuzzing::files;
use heatel::shops::{self, ShopTab};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let parts = files::split(data);
    let bytes = |index| parts.get(index).map_or(&[][..], |(_, bytes)| *bytes);
    if let Ok(catalog) = shops::catalog(bytes(0), bytes(1), bytes(2)) {
        for name in catalog.items.keys().take(64) {
            let _ = catalog.recipes_for(name);
            let _ = catalog.recipes_using(name).count();
        }
        let _ = catalog.categories_in(ShopTab::Basic).count();
    }
});
