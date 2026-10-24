//! The shop's layout: which items each of Dota's shop categories lists, in Dota's order
//! (`scripts/shops.txt`).

use crate::error::{Result, malformed};
use crate::keyvalues::{Token, text, tokens};

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
