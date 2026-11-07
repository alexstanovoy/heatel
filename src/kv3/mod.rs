//! Binary KeyValues3, the typed tree format of most Source 2 resource blocks.

mod binary;
mod decode;
mod legacy;
mod text;
mod value;

pub(crate) use decode::zstd_to_vec;
pub use decode::{decode, decode_in, is_kv3};
pub use text::{parse_text, parse_text_in};
pub use value::{Document, Object, Value};
