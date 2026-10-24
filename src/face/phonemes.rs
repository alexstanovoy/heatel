//! Valve's phoneme tables (`expressions/heroes/phonemes.txt`, the text the `.vfe` Dota's client
//! loads as `heroes/phonemes` is compiled from): for each phoneme the flex controller settings
//! of a mouth saying it, which a speaking face adds up as it says a line's phonemes.
//!
//! The file is a `$keys` line naming the controllers, an optional `$hasweighting` (each setting
//! is then followed by its influence, which phonemes do not use), and per phoneme its name, its
//! code (`"0x025a"`, or the one character it stands for), a number per key (two with
//! weighting) and a description.

use crate::error::{Result, malformed};

const WHAT: &str = "phonemes";
/// Most keys and phonemes; Dota's file has 31 and 50.
const MAX_KEYS: usize = 256;
const MAX_PHONEMES: usize = 1024;

/// A phoneme table: the controllers it sets and each phoneme's settings of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhonemeTable {
    /// The flex controllers the settings are of, by name.
    pub keys: Vec<String>,
    /// The phonemes, in file order.
    pub phonemes: Vec<PhonemeSetting>,
}

/// One phoneme of a [`PhonemeTable`].
#[derive(Clone, Debug, PartialEq)]
pub struct PhonemeSetting {
    /// Its name (`"aa"`).
    pub name: String,
    /// Its code: the IPA symbol's code point, as a line's phonemes name it.
    pub code: u32,
    /// Its setting of each of the table's keys.
    pub settings: Vec<f32>,
}

impl PhonemeTable {
    /// Reads a phoneme table's text.
    ///
    /// # Errors
    ///
    /// Text that is not UTF-8, a line of the wrong number of settings or with a number that does
    /// not read, a code that is neither hexadecimal nor one character, no keys or no phonemes,
    /// and more than 256 keys or 1024 phonemes.
    pub fn parse(source: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(source).map_err(|_| malformed(WHAT, "not UTF-8"))?;
        let mut table = Self::default();
        let mut weighted = false;
        for line in text.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("$keys") {
                table.keys = rest.split_whitespace().map(str::to_owned).collect();
                if table.keys.len() > MAX_KEYS {
                    return Err(malformed(WHAT, format!("{} keys", table.keys.len())));
                }
            } else if line.starts_with("$hasweighting") {
                weighted = true;
            } else if line.starts_with('"') {
                if table.phonemes.len() >= MAX_PHONEMES {
                    return Err(malformed(WHAT, "too many phonemes"));
                }
                table.phonemes.push(row(line, table.keys.len(), weighted)?);
            }
        }
        if table.keys.is_empty() || table.phonemes.is_empty() {
            return Err(malformed(WHAT, "no keys or no phonemes"));
        }
        Ok(table)
    }
}

/// One phoneme's line.
fn row(line: &str, keys: usize, weighted: bool) -> Result<PhonemeSetting> {
    let mut quoted = line.split('"');
    quoted.next();
    let name = quoted.next().unwrap_or_default().to_owned();
    quoted.next();
    let code = quoted
        .next()
        .ok_or_else(|| malformed(WHAT, "a phoneme without its code"))?;
    let numbers: Vec<f32> = quoted
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .map(|n| n.parse::<f32>().ok().filter(|f| f.is_finite()))
        .collect::<Option<_>>()
        .ok_or_else(|| malformed(WHAT, "a setting that is no number"))?;
    let per_key = if weighted { 2 } else { 1 };
    if numbers.len() != keys * per_key {
        return Err(malformed(
            WHAT,
            format!("{} numbers for {keys} keys", numbers.len()),
        ));
    }
    let code = match code.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => {
            let mut chars = code.chars();
            chars
                .next()
                .filter(|_| chars.next().is_none())
                .map(u32::from)
        }
    }
    .ok_or_else(|| malformed(WHAT, format!("phoneme code {code:?}")))?;
    Ok(PhonemeSetting {
        name,
        code,
        settings: numbers.iter().step_by(per_key).copied().collect(),
    })
}
