//! Typed reads of a particle definition's KV3 members. A member that is absent takes the
//! schema's default (compiled files leave defaults out); one of the wrong kind, a number that is
//! not finite, a control point past 63 or a list longer than any definition has is an error.

use crate::error::{Result, malformed};
use crate::kv3::{Object, Value};

use super::field::Field;
use super::transform::ControlPoint;

pub(super) const WHAT: &str = "vpcf";
/// The highest control point a system has (Source 2 has 64).
pub const MAX_CONTROL_POINT: u8 = 63;
/// Most functions in one of a system's lists; Dota's have up to ~30.
pub(super) const MAX_FUNCTIONS: usize = 128;

/// An object of a definition, read member by member.
#[derive(Clone, Copy)]
pub(super) struct Obj<'a> {
    object: &'a Object,
}

impl<'a> Obj<'a> {
    pub(super) fn new(value: &'a Value) -> Result<Self> {
        value
            .as_object()
            .map(|object| Self { object })
            .ok_or_else(|| malformed(WHAT, "a function or input is not an object"))
    }

    pub(super) fn get(&self, key: &str) -> Option<&'a Value> {
        self.object.get(key).filter(|v| !matches!(v, Value::Null))
    }

    pub(super) fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub(super) fn float(&self, key: &str, default: f32) -> Result<f32> {
        self.get(key).map_or(Ok(default), |v| number(key, v))
    }

    pub(super) fn int(&self, key: &str, default: i64) -> Result<i64> {
        match self.get(key) {
            None => Ok(default),
            Some(Value::Bool(b)) => Ok(i64::from(*b)),
            Some(v) => v
                .as_i64()
                .or_else(|| {
                    v.as_f64()
                        .filter(|f| f.fract() == 0.0 && f.abs() < 1e15)
                        .map(|f| f as i64)
                })
                .ok_or_else(|| malformed(WHAT, format!("{key} is not an integer"))),
        }
    }

    /// An integer bounded to `min..=max`.
    pub(super) fn int_in(&self, key: &str, default: i64, min: i64, max: i64) -> Result<i64> {
        let value = self.int(key, default)?;
        if !(min..=max).contains(&value) {
            return Err(malformed(
                WHAT,
                format!("{key} {value} is outside {min}..={max}"),
            ));
        }
        Ok(value)
    }

    /// An integer clamped to `min..=max`, as the game reads counts out of range.
    pub(super) fn int_clamped(&self, key: &str, default: i64, min: i64, max: i64) -> Result<i64> {
        Ok(self.int(key, default)?.clamp(min, max))
    }

    pub(super) fn bool(&self, key: &str, default: bool) -> Result<bool> {
        match self.get(key) {
            None => Ok(default),
            Some(Value::Bool(b)) => Ok(*b),
            Some(v) => v
                .as_i64()
                .map(|i| i != 0)
                .ok_or_else(|| malformed(WHAT, format!("{key} is not a boolean"))),
        }
    }

    pub(super) fn str(&self, key: &str) -> Result<Option<&'a str>> {
        self.get(key)
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| malformed(WHAT, format!("{key} is not a string")))
            })
            .transpose()
    }

    /// An enumeration, by its name or its number; `names[i]` is the name of `i`.
    pub(super) fn choice(&self, key: &str, names: &[&str], default: usize) -> Result<usize> {
        match self.get(key) {
            None => Ok(default),
            Some(Value::String(name)) => names
                .iter()
                .position(|n| n == name)
                .ok_or_else(|| malformed(WHAT, format!("{key} {name:?} is not known"))),
            Some(v) => v
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|i| *i < names.len())
                .ok_or_else(|| malformed(WHAT, format!("{key} is not one of {names:?}"))),
        }
    }

    pub(super) fn vec3(&self, key: &str, default: [f32; 3]) -> Result<[f32; 3]> {
        let Some(value) = self.get(key) else {
            return Ok(default);
        };
        if let Some(text) = value.as_str() {
            return text_vector(key, text);
        }
        let items = value
            .as_array()
            .filter(|a| a.len() >= 3)
            .ok_or_else(|| malformed(WHAT, format!("{key} is not a vector")))?;
        Ok([
            number(key, &items[0])?,
            number(key, &items[1])?,
            number(key, &items[2])?,
        ])
    }

    pub(super) fn vec2(&self, key: &str) -> Result<[f32; 2]> {
        let Some(value) = self.get(key) else {
            return Ok([0.0; 2]);
        };
        let items = value
            .as_array()
            .filter(|a| a.len() >= 2)
            .ok_or_else(|| malformed(WHAT, format!("{key} is not a 2D vector")))?;
        Ok([number(key, &items[0])?, number(key, &items[1])?])
    }

    /// A colour of 0..255 channels as 0..1, its alpha left out.
    pub(super) fn color(&self, key: &str, default: [f32; 3]) -> Result<[f32; 3]> {
        let Some(rgba) = self.color4(key)? else {
            return Ok(default);
        };
        Ok([rgba[0], rgba[1], rgba[2]])
    }

    /// A colour of 0..255 channels as 0..1, alpha 1 when it has none.
    pub(super) fn color4(&self, key: &str) -> Result<Option<[f32; 4]>> {
        let Some(value) = self.get(key) else {
            return Ok(None);
        };
        let items = value
            .as_array()
            .filter(|a| (3..=4).contains(&a.len()))
            .ok_or_else(|| malformed(WHAT, format!("{key} is not a colour")))?;
        let mut rgba = [1.0; 4];
        for (out, item) in rgba.iter_mut().zip(items) {
            *out = number(key, item)?.clamp(0.0, 255.0) / 255.0;
        }
        Ok(Some(rgba))
    }

    /// A control point; -1, which some files use for none, reads as 0 as the game reads it.
    pub(super) fn control_point(&self, key: &str, default: u8) -> Result<ControlPoint> {
        let index = self
            .int_in(key, i64::from(default), -1, i64::from(MAX_CONTROL_POINT))?
            .max(0);
        ControlPoint::new(index as u8).ok_or_else(|| malformed(WHAT, format!("{key} is past 63")))
    }

    pub(super) fn field(&self, key: &str, default: Field) -> Result<Field> {
        match self.get(key) {
            None => Ok(default),
            Some(v) => {
                let id = v
                    .as_i64()
                    .ok_or_else(|| malformed(WHAT, format!("{key} is not a particle field")))?;
                Field::from_id(id)
                    .ok_or_else(|| malformed(WHAT, format!("{key} field {id} is not known")))
            }
        }
    }

    pub(super) fn object(&self, key: &str) -> Result<Option<Obj<'a>>> {
        self.get(key).map(Obj::new).transpose()
    }

    /// A list of at most `max` items.
    pub(super) fn array(&self, key: &str, max: usize) -> Result<&'a [Value]> {
        let Some(value) = self.get(key) else {
            return Ok(&[]);
        };
        let items = value
            .as_array()
            .ok_or_else(|| malformed(WHAT, format!("{key} is not a list")))?;
        if items.len() > max {
            return Err(malformed(
                WHAT,
                format!("{key} lists {} items, more than {max}", items.len()),
            ));
        }
        Ok(items)
    }
}

fn number(key: &str, value: &Value) -> Result<f32> {
    let float = match value {
        Value::Bool(b) => f64::from(u8::from(*b)),
        v => v
            .as_f64()
            .ok_or_else(|| malformed(WHAT, format!("{key} is not a number")))?,
    };
    let single = float as f32;
    if !single.is_finite() {
        return Err(malformed(WHAT, format!("{key} is not a finite number")));
    }
    Ok(single)
}

/// A vector written as text, `"x y z"`, as some older files store one.
fn text_vector(key: &str, text: &str) -> Result<[f32; 3]> {
    let mut out = [0.0; 3];
    let mut words = text.split_whitespace();
    for value in &mut out {
        *value = words
            .next()
            .and_then(|w| w.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .ok_or_else(|| malformed(WHAT, format!("{key} {text:?} is not a vector")))?;
    }
    Ok(out)
}
