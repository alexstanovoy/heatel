use crate::error::{Result, malformed};
use crate::kv3::Value;

/// Most memory one decoded tree may take (values plus the strings and blobs they copy); the
/// largest tree of the install (the map's world node) takes ~20 MB.
const MAX_TREE_BYTES: usize = 256 << 20;
/// Deepest nesting of arrays, objects and structs.
const MAX_DEPTH: usize = 256;
/// What a heap string takes besides its bytes: the allocator's header and rounding.
const STRING_OVERHEAD: usize = 16;

/// Bounds the tree a decoder builds, so no input (a string referenced a million times, a typed
/// array of zero-width constants, a pointer cycle) grows it without limit.
pub(crate) struct Budget {
    what: &'static str,
    limit: usize,
    left: usize,
}

impl Budget {
    pub(crate) fn new(what: &'static str) -> Self {
        Self::with_limit(what, MAX_TREE_BYTES)
    }

    /// A budget of `limit` bytes.
    pub(crate) fn with_limit(what: &'static str, limit: usize) -> Self {
        Self {
            what,
            limit,
            left: limit,
        }
    }

    /// The bytes charged so far.
    pub(crate) fn spent(&self) -> usize {
        self.limit - self.left
    }

    /// Charges `count` slots of `T`, as a list reserving them takes.
    pub(crate) fn slots<T>(&mut self, count: usize) -> Result<()> {
        let bytes = count
            .checked_mul(size_of::<T>())
            .ok_or_else(|| malformed(self.what, format!("{count} slots, more than it may take")))?;
        self.bytes(bytes)
    }

    /// Charges one value at nesting `depth`, as the slot it takes in its object (a key and
    /// the value) or array.
    pub(crate) fn node(&mut self, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(malformed(
                self.what,
                format!("nested deeper than {MAX_DEPTH}"),
            ));
        }
        self.bytes(size_of::<(String, Value)>())
    }

    /// Charges `len` copied bytes.
    pub(crate) fn bytes(&mut self, len: usize) -> Result<()> {
        self.left = self
            .left
            .checked_sub(len)
            .ok_or_else(|| malformed(self.what, format!("takes {len} bytes more than it may")))?;
        Ok(())
    }

    /// A copy of `text`, charged with what its allocation takes besides.
    pub(crate) fn copy(&mut self, text: &str) -> Result<String> {
        if !text.is_empty() {
            self.bytes(text.len() + STRING_OVERHEAD)?;
        }
        Ok(text.to_owned())
    }

    /// `bytes` as text, charged before it is made: each byte that is not UTF-8 reads as
    /// U+FFFD, three bytes.
    pub(crate) fn text(&mut self, bytes: &[u8]) -> Result<String> {
        match std::str::from_utf8(bytes) {
            Ok(text) => self.copy(text),
            Err(_) => {
                self.bytes(
                    bytes
                        .len()
                        .saturating_mul(3)
                        .saturating_add(STRING_OVERHEAD),
                )?;
                Ok(String::from_utf8_lossy(bytes).into_owned())
            }
        }
    }

    /// A copy of `bytes`, charged before it is made.
    pub(crate) fn bytes_of(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.bytes(bytes.len())?;
        Ok(bytes.to_vec())
    }
}
