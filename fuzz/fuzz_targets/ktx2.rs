//! A KTX2 file decoded to plain channels, as a client whose GPU samples no block format reads
//! the files an export wrote: any bytes, since the files lie in a folder anyone may write. No
//! input panics, and what is decoded is a KTX2 file of plain channels that decodes again.
#![no_main]

use heatel::texture::ktx2_to_plain;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(plain) = ktx2_to_plain(data) {
        assert!(
            ktx2_to_plain(&plain).is_ok(),
            "a decoded file that does not decode again"
        );
    }
});
