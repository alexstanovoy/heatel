//! The shop's layout: `scripts/shops.txt`'s categories and their items. No input panics.
#![no_main]

use heatel::shops::categories;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = categories(data);
});
