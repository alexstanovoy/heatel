//! KeyValues (KV1) text as Valve's scripts and localization files write it: quoted keys and
//! values, braces, `//` and `/* */` comments. Every reader is bounded: the text is an untrusted
//! file.
//!
//! ```
//! use heatel::keyvalues::{Kv, parse_kv};
//!
//! let tree = parse_kv(r#""DOTAAbilities" { "item_boots" { "ItemCost" "500" } }"#)?;
//! let cost = tree.get("DOTAAbilities").and_then(|b| b.get("item_boots")).and_then(|b| b.get("ItemCost"));
//! assert!(matches!(cost, Some(Kv::Text(t)) if t == "500"));
//! # Ok::<(), heatel::Error>(())
//! ```

use std::collections::HashMap;

use crate::budget::Budget;
use crate::error::{Result, malformed};
use crate::kv1::{OrderedTokens, quoted_end};
pub use crate::kv1::{Token, Tokens};

/// Deepest nesting of a script's blocks.
const MAX_DEPTH: usize = 64;

/// Largest file read; the largest, the localization, is ~1.8 MB.
const MAX_FILE: usize = 16 << 20;

/// Most tokens/nodes accepted by [`parse_ordered`] and [`parse_ordered_in`].
pub const MAX_ORDERED_NODES: usize = 1 << 18;

/// A file as text, without a byte-order mark; one that is not UTF-8 or larger than any of
/// Dota's (16 MiB) is an error.
pub fn text(bytes: &[u8]) -> Result<&str> {
    if bytes.len() > MAX_FILE {
        return Err(malformed("keyvalues", format!("{} bytes", bytes.len())));
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|e| malformed("keyvalues", e.to_string()))
}

/// A KeyValues node: a string, or a block of keys in file order (a repeated key keeps its
/// first value; repeated blocks merge into the first).
#[derive(Debug)]
pub enum Kv {
    /// A string value.
    Text(String),
    /// A block of keyed values.
    Block(Block),
}

/// A block's items in file order, each key's first place found by hash: a script may hold
/// any number of keys, and each is looked up as it is read.
#[derive(Debug, Default)]
pub struct Block {
    items: Vec<(String, Kv)>,
    first: HashMap<String, usize>,
}

impl Block {
    /// The value of `key` (its first, keys matching exactly).
    pub fn get(&self, key: &str) -> Option<&Kv> {
        self.position(key).map(|at| &self.items[at].1)
    }

    /// The keys and values in file order.
    pub fn items(&self) -> &[(String, Kv)] {
        &self.items
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.first.get(key).copied()
    }

    fn push(&mut self, key: String, value: Kv) {
        self.first.entry(key.clone()).or_insert(self.items.len());
        self.items.push((key, value));
    }
}

impl Kv {
    /// The value of `key` in a block (keys match exactly); none for a string.
    pub fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Self::Block(block) => block.position(key).map(|at| &block.items[at].1),
            Self::Text(_) => None,
        }
    }

    /// The string value, or none for a block.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Block(_) => None,
        }
    }

    /// A block's keys and values in file order; none for a string.
    pub fn block(&self) -> Option<&[(String, Kv)]> {
        match self {
            Self::Block(block) => Some(&block.items),
            Self::Text(_) => None,
        }
    }
}

/// The tokens of `text`, read as they are asked for.
pub fn tokens(text: &str) -> Tokens<'_> {
    Tokens::new(text)
}

/// KeyValues text as a lossless ordered tree (a [`Kv::Block`] at the root).
///
/// Unlike [`parse_kv`], this retains unquoted keys and block names (such as Valve's
/// `soundscapes_manifest` root), preserves every repeated sibling sub-block (`playlooping`,
/// `playrandom`, `rndwave`) and repeated string key in file order without merging, keeps
/// anonymous `{ ... }` blocks under an empty key `""`, and rejects unbalanced braces,
/// unterminated strings, files larger than 16 MiB, nesting deeper than 64, or more than
/// [`MAX_ORDERED_NODES`] tokens.
pub fn parse_ordered(text: &str) -> Result<Kv> {
    parse_ordered_in(text, &mut Budget::named("keyvalues"))
}

/// [`parse_ordered`] within `budget`: every key, string, and block growth is charged before it
/// is allocated.
pub fn parse_ordered_in(text: &str, budget: &mut Budget) -> Result<Kv> {
    if text.len() > MAX_FILE {
        return Err(malformed("keyvalues", format!("{} bytes", text.len())));
    }
    let mut stack: Vec<(String, Block)> = vec![(String::new(), Block::default())];
    let mut key: Option<String> = None;
    let mut lexer = OrderedTokens::new(text);
    let mut count = 0usize;
    while let Some(token) = lexer.next_token()? {
        count += 1;
        if count > MAX_ORDERED_NODES {
            return Err(malformed(
                "keyvalues",
                format!("more than {MAX_ORDERED_NODES} tokens"),
            ));
        }
        match token {
            Token::Open => {
                if stack.len() > MAX_DEPTH {
                    return Err(malformed("keyvalues", "blocks nested too deep"));
                }
                let block_key = key.take().unwrap_or_default();
                stack.push((block_key, Block::default()));
            }
            Token::Close => {
                if stack.len() < 2 {
                    return Err(malformed("keyvalues", "unexpected closing brace"));
                }
                key = None;
                let (block_key, block) = stack.pop().expect("more than the root");
                let parent = &mut stack.last_mut().expect("the root stays").1;
                budget.charge(push_cost(parent, block_key.len()))?;
                parent.push(block_key, Kv::Block(block));
            }
            Token::Text(s) => match key.take() {
                None => {
                    budget.charge(s.len() + 16)?;
                    key = Some(s.to_owned());
                }
                Some(k) => {
                    let top = &mut stack.last_mut().expect("the root stays").1;
                    budget.charge(push_cost(top, k.len()) + s.len() + 16)?;
                    top.push(k, Kv::Text(s.to_owned()));
                }
            },
        }
    }
    if stack.len() != 1 {
        return Err(malformed("keyvalues", "unclosed block"));
    }
    Ok(Kv::Block(
        stack.pop().map(|(_, block)| block).unwrap_or_default(),
    ))
}

/// KeyValues text as a tree (a [`Kv::Block`] at the root). Keys and values are quoted strings;
/// a block without a key, or whose key already holds a string, is read and dropped; a repeated
/// block key reopens the first block of that key in its place. Blocks nested deeper than 64
/// are an error. What it makes is bounded by the text's size alone; [`parse_kv_in`] holds it to
/// a budget.
pub fn parse_kv(text: &str) -> Result<Kv> {
    parse(text, false, &mut Budget::new(usize::MAX))
}

/// [`parse_kv`] within `budget`: every key and string copied, every block's growth of its items
/// and of the index of its keys, charged before it is made.
pub fn parse_kv_in(text: &str, budget: &mut Budget) -> Result<Kv> {
    parse(text, false, budget)
}

/// [`parse_kv`] that keeps every repeated string key (a `precache` block lists its particles
/// under one key, `"particle"`, many times); [`Block::get`] still finds the first.
pub fn parse_kv_repeats(text: &str) -> Result<Kv> {
    parse(text, true, &mut Budget::new(usize::MAX))
}

/// What pushing an item of a key of `key` bytes into `block` makes: the key's copy as the index's
/// (the item's own was charged as it was read), the items' growth (a list doubles, from four),
/// and the index's (a table of a power of two buckets past 8/7 of what it holds, doubling from
/// three, an entry and a control byte each).
fn push_cost(block: &Block, key: usize) -> usize {
    let mut bytes = key + 16;
    let items = block.items.capacity();
    if block.items.len() == items {
        bytes += (2 * items).max(4).saturating_sub(items) * size_of::<(String, Kv)>();
    }
    if block.first.len() == block.first.capacity() {
        let grown = (2 * block.first.capacity()).max(3);
        let buckets = (grown * 8 / 7 + 1).next_power_of_two();
        bytes += buckets * (size_of::<(String, usize)>() + 1) + 16;
    }
    bytes
}

fn parse(text: &str, keep_repeats: bool, budget: &mut Budget) -> Result<Kv> {
    // Each open block: its items, and its key and place in its parent (none for a dropped one).
    let mut stack: Vec<Open> = vec![(Block::default(), None)];
    let mut key: Option<String> = None;
    for token in tokens(text) {
        match token {
            Token::Open => {
                if stack.len() > MAX_DEPTH {
                    return Err(malformed("keyvalues", "blocks nested too deep"));
                }
                let top = &mut stack.last_mut().expect("the root stays").0;
                let open = match key.take() {
                    Some(k) => match top.position(&k) {
                        None => Some((k, top.items.len(), Block::default())),
                        Some(at) => match &mut top.items[at].1 {
                            Kv::Block(block) => Some((k, at, std::mem::take(block))),
                            Kv::Text(_) => None,
                        },
                    },
                    None => None,
                };
                match open {
                    Some((k, at, block)) => stack.push((block, Some((k, at)))),
                    None => stack.push((Block::default(), None)),
                }
            }
            Token::Close => close(&mut stack, budget)?,
            Token::Text(s) => match key.take() {
                None => {
                    // A key waits for its value as a copy: charged as it is made, whatever comes
                    // after it (a key left at the end is dropped, as KeyValues readers drop one).
                    budget.charge(s.len() + 16)?;
                    key = Some(s.to_owned());
                }
                Some(k) => {
                    let top = &mut stack.last_mut().expect("the root stays").0;
                    if keep_repeats || top.position(&k).is_none() {
                        budget.charge(push_cost(top, k.len()) + s.len() + 16)?;
                        top.push(k, Kv::Text(s.to_owned()));
                    }
                }
            },
        }
    }
    while stack.len() > 1 {
        close(&mut stack, budget)?;
    }
    Ok(Kv::Block(
        stack.pop().map(|(block, _)| block).unwrap_or_default(),
    ))
}

/// An open block: its items so far, and its key and place in its parent.
type Open = (Block, Option<(String, usize)>);

/// Closes the innermost open block, putting it back in its place (the root stays open): where
/// it was taken from when reopened, else after its parent's items (none came while it was open).
fn close(stack: &mut Vec<Open>, budget: &mut Budget) -> Result<()> {
    if stack.len() < 2 {
        return Ok(());
    }
    let (block, place) = stack.pop().expect("more than the root");
    let Some((key, at)) = place else {
        return Ok(());
    };
    let parent = &mut stack.last_mut().expect("the root stays").0;
    match parent.items.get_mut(at) {
        Some((k, slot)) if *k == key => *slot = Kv::Block(block),
        _ => {
            budget.charge(push_cost(parent, key.len()))?;
            parent.push(key, Kv::Block(block));
        }
    }
    Ok(())
}

/// The localization's `"key" "value"` lines (`resource/localization/*_english.txt`), keys
/// lower-cased, the first of a key kept, `\"` and `\n` in values unescaped.
pub fn localization_table(text: &str) -> HashMap<String, String> {
    let bytes = text.as_bytes();
    let mut table = HashMap::new();
    // Each try starts at a line start.
    let mut at = 0;
    while at < bytes.len() {
        let from = match localization_pair(text, at) {
            Some((key, value, end)) => {
                let value = value.replace("\\\"", "\"").replace("\\n", "\n");
                table.entry(key.to_lowercase()).or_insert(value);
                end
            }
            None => at,
        };
        match bytes[from..].iter().position(|&b| b == b'\n') {
            Some(n) => at = from + n + 1,
            None => break,
        }
    }
    table
}

/// A `"key" "value"` pair starting at line start `at` (whitespace may come first): the key, the
/// raw value and where the match ends.
fn localization_pair(text: &str, at: usize) -> Option<(&str, &str, usize)> {
    let bytes = text.as_bytes();
    let skip_ws = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        i
    };
    let i = skip_ws(at);
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let key_start = i + 1;
    let key_end = key_start + bytes[key_start..].iter().position(|&b| b == b'"')?;
    if key_end == key_start {
        return None;
    }
    let i = skip_ws(key_end + 1);
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let end = quoted_end(bytes, i + 1).ok()?;
    Some((&text[key_start..key_end], &text[i + 1..end], end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localization_handles_adjacent_quotes_escaped_quotes_and_empty_values() {
        let input = concat!(
            "\"lang\"\n{\n",
            "\"Tokens\"\n{\n",
            "\"DOTA_Tooltip_ability_special_bonus_unique_witch_doctor_2\"\"+12% Maledict Damage\"\n",
            "\"escaped_key\" \"say \\\"hello\\\"\\nworld\"\n",
            "\"adjacent_escaped\"\"say \\\"hi\\\"\"\n",
            "\"empty_spaced\" \"\"\n",
            "\"empty_adjacent\"\"\"\n",
            "\"normal\" \"value\"\n",
            "}\n}\n",
        );
        let table = localization_table(input);
        assert_eq!(
            table
                .get("dota_tooltip_ability_special_bonus_unique_witch_doctor_2")
                .map(String::as_str),
            Some("+12% Maledict Damage")
        );
        assert_eq!(
            table.get("escaped_key").map(String::as_str),
            Some("say \"hello\"\nworld")
        );
        assert_eq!(
            table.get("adjacent_escaped").map(String::as_str),
            Some("say \"hi\"")
        );
        assert_eq!(table.get("empty_spaced").map(String::as_str), Some(""));
        assert_eq!(table.get("empty_adjacent").map(String::as_str), Some(""));
        assert_eq!(table.get("normal").map(String::as_str), Some("value"));
    }

    #[test]
    fn kv_parser_handles_adjacent_quotes_escaped_quotes_empty_values_and_nested_blocks() {
        let input = r#""Root" { "Sub" { "k1""v1" "k2" "" "k3""a\"b" "Inner" { "x""y" } } }"#;
        let tree = parse_kv(input).unwrap();
        let sub = tree.get("Root").unwrap().get("Sub").unwrap();
        assert_eq!(sub.get("k1").and_then(Kv::as_str), Some("v1"));
        assert_eq!(sub.get("k2").and_then(Kv::as_str), Some(""));
        assert_eq!(sub.get("k3").and_then(Kv::as_str), Some(r#"a\"b"#));
        assert_eq!(
            sub.get("Inner").unwrap().get("x").and_then(Kv::as_str),
            Some("y")
        );
    }

    #[test]
    fn parse_ordered_preserves_unquoted_names_and_repeated_sibling_blocks() {
        let input = r#"
// comment
soundscapes_manifest
{
    "file" "scripts/a.txt"
    /* block comment */
    "playlooping" { "wave" "sounds/a.vsnd" }
    "playlooping" { "wave" "sounds/b.vsnd" }
    "playrandom"
    {
        "rndwave"
        {
            "wave" "sounds/r1.vsnd"
            "wave" "sounds/r2.vsnd"
        }
        "rndwave"
        {
            "wave" "sounds/r3.vsnd"
        }
    }
}
"#;
        let tree = parse_ordered(input).unwrap();
        let root = tree.block().unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].0, "soundscapes_manifest");
        let items = root[0].1.block().unwrap();
        let keys: Vec<&str> = items.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["file", "playlooping", "playlooping", "playrandom"]);
        assert_eq!(
            items[1].1.get("wave").and_then(Kv::as_str),
            Some("sounds/a.vsnd")
        );
        assert_eq!(
            items[2].1.get("wave").and_then(Kv::as_str),
            Some("sounds/b.vsnd")
        );
        let rndwaves: Vec<&[(String, Kv)]> = items[3]
            .1
            .block()
            .unwrap()
            .iter()
            .filter_map(|(k, v)| (k == "rndwave").then(|| v.block()).flatten())
            .collect();
        assert_eq!(rndwaves.len(), 2);
        assert_eq!(rndwaves[0].len(), 2);
        assert_eq!(rndwaves[1].len(), 1);
    }

    #[test]
    fn parse_ordered_enforces_bounds_and_syntax() {
        assert!(parse_ordered("\"a\" { \"b\" \"c\"").is_err());
        assert!(parse_ordered("}").is_err());
        assert!(parse_ordered("\"unterminated").is_err());
        let deep = format!(
            "{} {}",
            "a {".repeat(MAX_DEPTH + 1),
            "}".repeat(MAX_DEPTH + 1)
        );
        assert!(parse_ordered(&deep).is_err());
        let mut tiny = Budget::new(16);
        assert!(parse_ordered_in("\"a\" \"bbbbbbbbbbbbbbbbbbbb\"", &mut tiny).is_err());
    }
}
