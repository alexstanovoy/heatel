//! Shared Valve KeyValues1 (KV1) lexical primitives.

use crate::error::{Result, malformed};

/// A token of KeyValues text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token<'a> {
    /// A quoted string's text, escapes left as they are.
    Text(&'a str),
    /// `{`
    Open,
    /// `}`
    Close,
}

/// The quoted strings and braces of KeyValues text, one at a time; comments and anything else
/// are skipped.
#[derive(Clone, Debug)]
pub struct Tokens<'a> {
    text: &'a str,
    at: usize,
    no_comment_end: Option<usize>,
    no_quote_end: usize,
}

impl<'a> Tokens<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            text,
            at: 0,
            no_comment_end: None,
            no_quote_end: 0,
        }
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        let bytes = self.text.as_bytes();
        while self.at < bytes.len() {
            let i = self.at;
            self.at += 1;
            match bytes[i] {
                b'"' if i >= self.no_quote_end => match quoted_end(bytes, i + 1) {
                    Ok(end) => {
                        self.at = end + 1;
                        return Some(Token::Text(&self.text[i + 1..end]));
                    }
                    Err(stop) => self.no_quote_end = stop,
                },
                b'{' => return Some(Token::Open),
                b'}' => return Some(Token::Close),
                b'/' if bytes.get(i + 1) == Some(&b'/') => self.at = line_end(bytes, i),
                b'/' if bytes.get(i + 1) == Some(&b'*') && self.no_comment_end.is_none() => {
                    match self.text[i + 2..].find("*/") {
                        Some(n) => self.at = i + n + 4,
                        None => self.no_comment_end = Some(i),
                    }
                }
                _ => {}
            }
        }
        None
    }
}

pub(crate) fn line_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |n| from + n)
}

pub(crate) fn quoted_end(bytes: &[u8], from: usize) -> std::result::Result<usize, usize> {
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Ok(i),
            b'\\' if bytes.get(i + 1).is_some_and(|&b| b != b'\n') => i += 2,
            b'\\' => return Err(i),
            _ => i += 1,
        }
    }
    Err(bytes.len())
}

pub(crate) fn only_comments(text: &str) -> bool {
    text.lines().all(|l| {
        let l = l.trim_start();
        l.is_empty() || l.starts_with("//")
    })
}

pub(crate) fn next_strict<'a>(rest: &mut &'a str, what: &'static str) -> Result<Option<Token<'a>>> {
    loop {
        *rest = rest.trim_start();
        match rest.strip_prefix("//") {
            Some(after) => *rest = after.split_once('\n').map_or("", |(_, tail)| tail),
            None => break,
        }
    }
    match rest.as_bytes().first() {
        None => Ok(None),
        Some(&b'{') => {
            *rest = &rest[1..];
            Ok(Some(Token::Open))
        }
        Some(&b'}') => {
            *rest = &rest[1..];
            Ok(Some(Token::Close))
        }
        Some(&b'"') => {
            let body = &rest[1..];
            let end = body
                .find('"')
                .ok_or_else(|| malformed(what, "an unterminated string"))?;
            *rest = &body[end + 1..];
            Ok(Some(Token::Text(&body[..end])))
        }
        Some(_) => Err(malformed(what, "an unquoted token")),
    }
}
