//! Text KeyValues3: the form Valve's own scripts take (`scripts/soundmixers.txt`), a header
//! comment, then objects `{ key = value }`, arrays `[ a, b ]`, strings, numbers and the words
//! `true`, `false` and `null`.
//!
//! Every input is bounded: its size, its nesting and the number of values it holds.

use super::{Document, Object, Value};
use crate::error::{Result, malformed};

/// Most bytes a text document holds.
const MAX_BYTES: usize = 4 << 20;
/// Deepest nesting of objects and arrays.
const MAX_DEPTH: usize = 32;
/// Most values (members and elements) a document holds.
const MAX_VALUES: usize = 1 << 17;
/// Longest string, key or word.
const MAX_TOKEN: usize = 1 << 12;

/// Reads a text KV3 document. Numbers read as `Float` (or `Int` for whole ones), and a flag in
/// front of a string (`resource:"x"`) is dropped, as the binary reader drops it. Anything the
/// grammar above does not hold is refused, with where it was met.
pub fn parse_text(bytes: &[u8]) -> Result<Document> {
    if bytes.len() > MAX_BYTES {
        return Err(malformed(
            "kv3 text",
            "larger than any script of the install",
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| malformed("kv3 text", "not UTF-8"))?;
    let mut reader = Reader {
        text: text.as_bytes(),
        at: 0,
        values: 0,
    };
    reader.skip_space()?;
    let root = reader.value(0)?;
    reader.skip_space()?;
    if reader.at != reader.text.len() {
        return Err(reader.fault("text after the document"));
    }
    Ok(Document {
        format: [0; 16],
        root,
    })
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
    values: usize,
}

impl Reader<'_> {
    fn fault(&self, what: &str) -> crate::error::Error {
        let line = 1 + self.text[..self.at.min(self.text.len())]
            .iter()
            .filter(|b| **b == b'\n')
            .count();
        malformed("kv3 text", format!("line {line}: {what}"))
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    /// Skips white space, commas, `//` and `/* */` comments, and `<!-- -->` headers.
    fn skip_space(&mut self) -> Result<()> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n' | b',') => self.at += 1,
                Some(b'/') if self.text.get(self.at + 1) == Some(&b'/') => {
                    while self.peek().is_some_and(|b| b != b'\n') {
                        self.at += 1;
                    }
                }
                Some(b'/') if self.text.get(self.at + 1) == Some(&b'*') => {
                    self.skip_until(b"*/")?;
                }
                Some(b'<') if self.text[self.at..].starts_with(b"<!--") => {
                    self.skip_until(b"-->")?;
                }
                _ => return Ok(()),
            }
        }
    }

    fn skip_until(&mut self, end: &[u8]) -> Result<()> {
        match self.text[self.at..]
            .windows(end.len())
            .position(|w| w == end)
        {
            Some(at) => {
                self.at += at + end.len();
                Ok(())
            }
            None => Err(self.fault("a comment that never ends")),
        }
    }

    fn count(&mut self) -> Result<()> {
        self.values += 1;
        if self.values > MAX_VALUES {
            return Err(self.fault("more values than any script has"));
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(self.fault("nested deeper than any script"));
        }
        self.count()?;
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(_) => self.word(),
            None => Err(self.fault("a value was expected")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value> {
        self.at += 1;
        let mut object = Object::default();
        loop {
            self.skip_space()?;
            match self.peek() {
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(object));
                }
                None => return Err(self.fault("an object that never ends")),
                _ => {}
            }
            let key = if self.peek() == Some(b'"') {
                self.string()?
            } else {
                self.token()?
            };
            self.skip_space()?;
            if self.peek() != Some(b'=') {
                return Err(self.fault("'=' after a key"));
            }
            self.at += 1;
            self.skip_space()?;
            let value = self.value(depth + 1)?;
            object.push(key, value);
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value> {
        self.at += 1;
        let mut list = Vec::new();
        loop {
            self.skip_space()?;
            match self.peek() {
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(list));
                }
                None => return Err(self.fault("an array that never ends")),
                _ => list.push(self.value(depth + 1)?),
            }
        }
    }

    fn string(&mut self) -> Result<String> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => return Err(self.fault("a string that never ends")),
                Some(b'"') => {
                    self.at += 1;
                    return String::from_utf8(out).map_err(|_| self.fault("a string not UTF-8"));
                }
                Some(b'\\') => {
                    self.at += 1;
                    match self.peek() {
                        Some(b'n') => out.push(b'\n'),
                        Some(b't') => out.push(b'\t'),
                        Some(c @ (b'"' | b'\\' | b'/')) => out.push(c),
                        _ => return Err(self.fault("an escape the format does not have")),
                    }
                    self.at += 1;
                }
                Some(c) => {
                    out.push(c);
                    self.at += 1;
                }
            }
            if out.len() > MAX_TOKEN {
                return Err(self.fault("a string longer than any script has"));
            }
        }
    }

    /// A bare word: letters, digits and `_ . - + : / @`.
    fn token(&mut self) -> Result<String> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b"_.-+:/@".contains(&b))
        {
            self.at += 1;
            if self.at - start > MAX_TOKEN {
                return Err(self.fault("a word longer than any script has"));
            }
        }
        if self.at == start {
            return Err(self.fault("a word was expected"));
        }
        Ok(String::from_utf8_lossy(&self.text[start..self.at]).into_owned())
    }

    /// `true`, `false`, `null`, a number, or a flag in front of a string.
    fn word(&mut self) -> Result<Value> {
        let word = self.token()?;
        if self.peek() == Some(b'"') && word.ends_with(':') {
            return Ok(Value::String(self.string()?));
        }
        match word.as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            "null" => Ok(Value::Null),
            _ => {
                if let Ok(whole) = word.parse::<i64>() {
                    return Ok(Value::Int(whole));
                }
                match word.parse::<f64>() {
                    Ok(number) if number.is_finite() => Ok(Value::Float(number)),
                    _ => Err(self.fault(&format!("'{word}' is no value"))),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_with_objects_arrays_strings_and_numbers_reads() {
        let text =
            b"<!-- kv3 encoding:text -->\n{ A = [ { n = \"x\" v = 0.5 i = 3 b = true }, ] // c\n }";
        let document = parse_text(text).expect("a document");
        let root = document.root.as_object().expect("an object");
        let first = root.get("A").and_then(Value::as_array).expect("an array")[0]
            .as_object()
            .expect("an object");
        assert_eq!(first.get("n").and_then(Value::as_str), Some("x"));
        assert_eq!(first.get("v").and_then(Value::as_f64), Some(0.5));
        assert_eq!(first.get("i").and_then(Value::as_f64), Some(3.0));
        assert_eq!(first.get("b").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn nesting_past_the_bound_is_refused_not_recursed_into() {
        let text = "{ a = ".repeat(1000) + &"}".repeat(1000);
        let err = parse_text(text.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("nested deeper"), "{err}");
    }

    #[test]
    fn a_number_that_is_not_finite_is_refused() {
        for bad in ["{ v = NaN }", "{ v = inf }", "{ v = 1e999 }"] {
            let err = parse_text(bad.as_bytes()).unwrap_err();
            assert!(err.to_string().contains("is no value"), "{bad}: {err}");
        }
    }

    #[test]
    fn a_truncated_document_is_refused_with_its_line() {
        let err = parse_text(b"{\n a = [ 1 2\n").unwrap_err();
        assert!(err.to_string().contains("line 3"), "{err}");
    }
}
