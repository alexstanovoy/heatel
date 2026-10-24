/// A decoded KV3 document: the format GUID its writer stamped (generic, modeldoc, ...) and the
/// root value, usually an object.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// The format GUID.
    pub format: [u8; 16],
    /// The root value.
    pub root: Value,
}

/// A KV3 value. The binary forms collapse into these: every integer width into `Int` or
/// `UInt`, `float` and `double` into `Float`, typed arrays into `Array`. Flags (resource,
/// sound event, ...) are read and dropped.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer of any width.
    Int(i64),
    /// An unsigned integer of any width.
    UInt(u64),
    /// A `float` or `double`.
    Float(f64),
    /// A string.
    String(String),
    /// A binary blob.
    Blob(Vec<u8>),
    /// An array, typed or not.
    Array(Vec<Value>),
    /// An object.
    Object(Object),
}

/// A KV3 object's members in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object {
    members: Vec<(String, Value)>,
}

impl Object {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            members: Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn push(&mut self, key: String, value: Value) {
        self.members.push((key, value));
    }

    /// The member `key`; when a key repeats, the last one wins, as in Valve's reader.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.members
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// The members in file order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &Value)> {
        self.members.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// How many members it has.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether it has no members.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

impl Value {
    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?.get(key)
    }

    /// The boolean, if it is one.
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }

    /// The integer, if it is one that fits in an `i64`.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Value::Int(i) => Some(i),
            Value::UInt(u) => i64::try_from(u).ok(),
            _ => None,
        }
    }

    /// The integer, if it is one that fits in a `u64`.
    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            Value::Int(i) => u64::try_from(i).ok(),
            Value::UInt(u) => Some(u),
            _ => None,
        }
    }

    /// Any number as `f64`.
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Value::Float(f) => Some(f),
            Value::Int(i) => Some(i as f64),
            Value::UInt(u) => Some(u as f64),
            _ => None,
        }
    }

    /// The string, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The blob, if it is one.
    pub fn as_blob(&self) -> Option<&[u8]> {
        match self {
            Value::Blob(b) => Some(b),
            _ => None,
        }
    }

    /// The array's items, if it is one.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The object, if it is one.
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
}

/// KV3 as serde data: a document as `{ "format": <GUID bytes>, "root": <value> }`, an object as a
/// map in file order, a blob as bytes.
#[cfg(feature = "serde")]
mod serialize {
    use serde::ser::{Serialize, SerializeMap, SerializeStruct, Serializer};

    use super::{Document, Object, Value};

    #[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
    impl Serialize for Document {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut document = serializer.serialize_struct("Document", 2)?;
            document.serialize_field("format", &self.format)?;
            document.serialize_field("root", &self.root)?;
            document.end()
        }
    }

    #[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
    impl Serialize for Value {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                Self::Null => serializer.serialize_unit(),
                Self::Bool(b) => serializer.serialize_bool(*b),
                Self::Int(i) => serializer.serialize_i64(*i),
                Self::UInt(u) => serializer.serialize_u64(*u),
                Self::Float(f) => serializer.serialize_f64(*f),
                Self::String(s) => serializer.serialize_str(s),
                Self::Blob(b) => serializer.serialize_bytes(b),
                Self::Array(items) => serializer.collect_seq(items),
                Self::Object(object) => object.serialize(serializer),
            }
        }
    }

    #[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
    impl Serialize for Object {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(self.len()))?;
            for (key, value) in self.iter() {
                map.serialize_entry(key, value)?;
            }
            map.end()
        }
    }
}
