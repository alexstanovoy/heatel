use std::io;

/// Everything that can go wrong reading Valve's files. Every input is an untrusted local file, so
/// a malformed one is an ordinary error, never a panic.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No package holds the file.
    #[error("{0}: no such file")]
    NotFound(String),
    /// Reading a file failed.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// Why.
        source: io::Error,
    },
    /// A read past a file's end: the index that asked for it is wrong.
    #[error("{path}: bytes {offset}..+{len} run past its end ({size} bytes)")]
    OutOfRange {
        /// The file.
        path: String,
        /// Where the read starts.
        offset: u64,
        /// How many bytes it asks for.
        len: usize,
        /// The file's size.
        size: u64,
    },
    /// A file is not what its format says, or larger than any real one.
    #[error("{what}: {detail}")]
    Malformed {
        /// The reader that refused it (`vpk`, `kv3`, `vtex`, ...).
        what: &'static str,
        /// What is wrong.
        detail: String,
    },
    /// A file's bytes do not match the CRC its package lists.
    #[error("{path}: CRC32 {actual:08x}, the VPK directory says {expected:08x}")]
    Crc {
        /// The file.
        path: String,
        /// The package's CRC.
        expected: u32,
        /// The bytes' CRC.
        actual: u32,
    },
}

/// A result of this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

pub(crate) fn malformed(what: &'static str, detail: impl Into<String>) -> Error {
    Error::Malformed {
        what,
        detail: detail.into(),
    }
}
