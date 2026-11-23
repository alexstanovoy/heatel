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
    /// Creates an empty KV3 object.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty KV3 object with space for `capacity` members.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            members: Vec::with_capacity(capacity),
        }
    }

    /// Appends a `(key, value)` member to the object in file order.
    pub fn push(&mut self, key: String, value: Value) {
        self.members.push((key, value));
    }

    /// Appends a `(key, value)` member, converting key and value into [`String`] and [`Value`].
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.members.push((key.into(), value.into()));
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

impl<K: Into<String>, V: Into<Value>> FromIterator<(K, V)> for Object {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let (lower, _) = iter.size_hint();
        let mut object = Self::with_capacity(lower);
        for (key, value) in iter {
            object.insert(key, value);
        }
        object
    }
}

impl<K: Into<String>, V: Into<Value>, const N: usize> From<[(K, V); N]> for Object {
    fn from(members: [(K, V); N]) -> Self {
        members.into_iter().collect()
    }
}

impl Value {
    /// Constructs [`Value::Null`].
    pub fn null() -> Self {
        Self::Null
    }

    /// Constructs [`Value::Bool`].
    pub fn bool(value: bool) -> Self {
        Self::Bool(value)
    }

    /// Constructs [`Value::Int`].
    pub fn int(value: i64) -> Self {
        Self::Int(value)
    }

    /// Constructs [`Value::UInt`].
    pub fn uint(value: u64) -> Self {
        Self::UInt(value)
    }

    /// Constructs [`Value::Float`].
    pub fn float(value: f64) -> Self {
        Self::Float(value)
    }

    /// Constructs [`Value::String`].
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    /// Constructs [`Value::Blob`].
    pub fn blob(value: impl Into<Vec<u8>>) -> Self {
        Self::Blob(value.into())
    }

    /// Constructs [`Value::Array`] from any iterable of values convertible into [`Value`].
    pub fn from_array<I, V>(items: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        Self::Array(items.into_iter().map(Into::into).collect())
    }

    /// Constructs [`Value::Object`] from any iterable of `(key, value)` pairs.
    pub fn object<I, K, V>(members: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<Value>,
    {
        Self::Object(members.into_iter().collect())
    }

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

    /// Any finite number as `f32`.
    pub fn finite_f32(&self) -> Option<f32> {
        self.as_f64().map(|f| f as f32).filter(|f| f.is_finite())
    }

    /// Any finite number as `f64`.
    pub fn finite_f64(&self) -> Option<f64> {
        self.as_f64().filter(|f| f.is_finite())
    }

    /// The integer, if it fits in a `usize`.
    pub fn as_usize(&self) -> Option<usize> {
        self.as_u64().and_then(|u| usize::try_from(u).ok())
    }

    /// An array of exactly `N` finite `f32` numbers.
    pub fn f32_array<const N: usize>(&self) -> Option<[f32; N]> {
        let items = self.as_array()?;
        if items.len() != N {
            return None;
        }
        let mut out = [0.0; N];
        for (o, v) in out.iter_mut().zip(items) {
            *o = v.finite_f32()?;
        }
        Some(out)
    }

    /// The array's items, or an empty slice when not an array.
    pub fn slice(&self) -> &[Value] {
        self.as_array().unwrap_or_default()
    }

    pub(crate) fn array(&self, key: &str) -> &[Value] {
        self.get(key).map_or(&[], Value::slice)
    }

    pub(crate) fn str_field(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub(crate) fn f32_field(&self, key: &str) -> Option<f32> {
        self.get(key).and_then(Value::finite_f32)
    }

    pub(crate) fn usize_field(&self, key: &str) -> Option<usize> {
        self.get(key).and_then(Value::as_usize)
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

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Self::UInt(u64::from(value))
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Self::UInt(value)
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Self::Float(f64::from(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Self::Array(value)
    }
}

impl<V: Into<Value>, const N: usize> From<[V; N]> for Value {
    fn from(value: [V; N]) -> Self {
        Self::Array(value.into_iter().map(Into::into).collect())
    }
}

impl From<Object> for Value {
    fn from(value: Object) -> Self {
        Self::Object(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{Object, Value};

    #[test]
    fn public_constructors_build_values_and_objects_directly() {
        let mut obj = Object::new();
        obj.insert("flag", true);
        obj.insert("count", 42_i32);
        obj.insert("count", 7_u32);
        assert_eq!(obj.len(), 3);
        assert_eq!(obj.get("flag").and_then(Value::as_bool), Some(true));
        assert_eq!(obj.get("count").and_then(Value::as_u64), Some(7));

        let entity = Value::object([
            ("classname", Value::from("env_soundscape")),
            ("origin", Value::from_array([1.0_f64, 2.0, 3.0])),
            ("radius", Value::int(512)),
            ("raw", Value::blob([0xDE_u8, 0xAD])),
            ("none", Value::null()),
        ]);
        assert_eq!(entity.str_field("classname"), Some("env_soundscape"));
        assert_eq!(entity.get("radius").and_then(Value::as_i64), Some(512));
        assert_eq!(
            entity.get("raw").and_then(Value::as_blob),
            Some(&[0xDE, 0xAD][..])
        );
        assert_eq!(entity.get("none"), Some(&Value::Null));
        assert_eq!(
            entity.get("origin").and_then(Value::f32_array::<3>),
            Some([1.0, 2.0, 3.0])
        );
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
