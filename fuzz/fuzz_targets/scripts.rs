//! KeyValues text as the readers of an install's scripts and localization take it: the tokens,
//! the tree (with and without repeated keys), the localization's table and the items and
//! abilities a script defines. No input panics, and the work stays linear in the text.
#![no_main]

use heatel::keyvalues::{localization_table, parse_kv, parse_kv_repeats, text, tokens};
use heatel::scripts::entries;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = entries(data);
    let Ok(text) = text(data) else {
        return;
    };
    let _ = tokens(text).count();
    let _ = (parse_kv(text), parse_kv_repeats(text));
    let _ = localization_table(text);
});
