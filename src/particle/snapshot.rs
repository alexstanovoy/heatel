//! Bounded position/normal snapshots with root-bone skinning.
use crate::{
    Resource,
    bytes::Cursor,
    error::{Result, malformed},
    kv3::Value,
};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
const WHAT: &str = "vsnap";
/// Maximum points decoded from one snapshot.
pub const MAX_SNAPSHOT_POINTS: usize = 5000;
/// Longest name an [`Snapshot::Unsupported`] carries: the reader cuts what it formats to this, and
/// [`Snapshot::validate`] rejects more, so a snapshot the reader returns always validates.
pub const MAX_UNSUPPORTED_NAME: usize = 512;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_METADATA: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
/// Static snapshot geometry; other skinning bindings remain explicitly unsupported.
pub enum Snapshot {
    /// Root-skinned local-space geometry.
    Points {
        /// Local positions in particle coordinates.
        positions: Vec<[f32; 3]>,
        /// Optional local normals, one per position.
        normals: Option<Vec<[f32; 3]>>,
    },
    /// Named stream or skinning mode not interpreted by this reader.
    Unsupported(String),
}
impl Snapshot {
    /// Reads bounded DATA metadata and raw or compressed SNAP streams.
    pub fn read(resource: &Resource<'_>) -> Result<Self> {
        let metadata = resource
            .block(b"DATA")
            .ok_or_else(|| malformed(WHAT, "missing DATA"))?;
        if metadata.len() > MAX_METADATA {
            return Err(malformed(WHAT, "metadata exceeds 64 KiB"));
        }
        let mut budget = crate::Budget::new(MAX_METADATA);
        let document = resource
            .kv3_in(b"DATA", &mut budget)
            .ok_or_else(|| malformed(WHAT, "DATA is not KV3"))??;
        let root = document
            .root
            .as_object()
            .ok_or_else(|| malformed(WHAT, "DATA is not an object"))?;
        let count = integer(
            root.get("num_particles"),
            MAX_SNAPSHOT_POINTS,
            "particle count",
        )?;
        let attributes = root
            .get("attributes")
            .and_then(Value::as_array)
            .ok_or_else(|| malformed(WHAT, "missing attributes"))?;
        if attributes.len() > 8 {
            return Err(malformed(WHAT, "more than eight attributes"));
        }
        let strings = root
            .get("string_list")
            .and_then(Value::as_array)
            .unwrap_or(&[]);
        if strings.len() > 128
            || strings
                .iter()
                .any(|v| v.as_str().is_none_or(|s| s.len() > 256))
        {
            return Err(malformed(WHAT, "invalid string table"));
        }
        let packed = resource
            .block(b"SNAP")
            .ok_or_else(|| malformed(WHAT, "missing SNAP"))?;
        let bytes = unpack(packed)?;
        read_streams(attributes, &bytes, count, strings)
    }
    /// Validates geometry read back from a serialized cache.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Points { positions, normals } => {
                if positions.len() > MAX_SNAPSHOT_POINTS
                    || normals.as_ref().is_some_and(|n| n.len() != positions.len())
                {
                    return Err(malformed(WHAT, "invalid point count"));
                }
                if !positions
                    .iter()
                    .flatten()
                    .chain(normals.iter().flatten().flatten())
                    .all(|v| v.is_finite())
                {
                    return Err(malformed(WHAT, "non-finite vector"));
                }
            }
            Self::Unsupported(name) if name.len() > MAX_UNSUPPORTED_NAME => {
                return Err(malformed(WHAT, "unsupported name too long"));
            }
            _ => (),
        }
        Ok(())
    }
}
/// An unsupported snapshot named by `why`, cut to the limit `validate` enforces (at a character
/// boundary) because the name quotes file text of up to 256 bytes twice.
fn unsupported(mut why: String) -> Snapshot {
    let mut end = why.len().min(MAX_UNSUPPORTED_NAME);
    while !why.is_char_boundary(end) {
        end -= 1;
    }
    why.truncate(end);
    Snapshot::Unsupported(why)
}
fn integer(value: Option<&Value>, max: usize, name: &str) -> Result<usize> {
    value
        .and_then(Value::as_i64)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n <= max)
        .ok_or_else(|| malformed(WHAT, format!("invalid {name}")))
}
fn vectors(bytes: &[u8], count: usize) -> Result<Vec<[f32; 3]>> {
    if bytes.len() != count * 12 {
        return Err(malformed(WHAT, "float3 stream size mismatch"));
    }
    let mut cursor = Cursor::new(bytes, WHAT);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let p = [real(&mut cursor)?, real(&mut cursor)?, real(&mut cursor)?];
        if !p.iter().all(|v| v.is_finite()) {
            return Err(malformed(WHAT, "non-finite vector"));
        }
        out.push(p);
    }
    Ok(out)
}
fn root_skinning(bytes: &[u8], count: usize, strings: &[Value]) -> Result<Option<String>> {
    if bytes.len() != count * 24 {
        return Err(malformed(WHAT, "skinning stream size mismatch"));
    }
    let mut cursor = Cursor::new(bytes, WHAT);
    for _ in 0..count {
        let bones = [cursor.u16()?, cursor.u16()?, cursor.u16()?, cursor.u16()?];
        let weights = [
            real(&mut cursor)?,
            real(&mut cursor)?,
            real(&mut cursor)?,
            real(&mut cursor)?,
        ];
        if weights.iter().any(|v| !v.is_finite() || *v < 0.0)
            || (weights.iter().sum::<f32>() - 1.0).abs() > 0.001
        {
            return Err(malformed(WHAT, "invalid skinning weights"));
        }
        for (bone, weight) in bones.into_iter().zip(weights) {
            if weight == 0.0 {
                continue;
            }
            let name = strings
                .get(usize::from(bone))
                .and_then(Value::as_str)
                .ok_or_else(|| malformed(WHAT, "skinning index outside string table"))?;
            if name != "root" {
                return Ok(Some(format!("skinning bone {name}")));
            }
        }
    }
    Ok(None)
}
fn unpack(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() > MAX_BYTES * 2 {
        return Err(malformed(WHAT, "packed SNAP too large"));
    }
    let mut input = Cursor::new(bytes, WHAT);
    let header = input.u32()?;
    let size = (header & 0x7fff_ffff) as usize;
    if size > MAX_BYTES {
        return Err(malformed(WHAT, "SNAP exceeds 1 MiB"));
    }
    if header & 0x8000_0000 != 0 {
        if input.remaining() != size {
            return Err(malformed(WHAT, "raw SNAP size mismatch"));
        }
        return Ok(input.take(size)?.to_vec());
    }
    let mut out = Vec::with_capacity(size);
    let (mut mask, mut left) = (0u16, 0u8);
    while out.len() < size {
        if left == 0 {
            mask = input.u16()?;
            left = 16;
        }
        if mask & 1 == 0 {
            out.push(input.u8()?);
        } else {
            let word = input.u16()?;
            let distance = usize::from(word >> 4) + 1;
            let length = usize::from(word & 15) + 3;
            if distance > out.len() || length > size - out.len() {
                return Err(malformed(WHAT, "invalid SNAP back-reference"));
            }
            for _ in 0..length {
                let value = out[out.len() - distance];
                out.push(value);
            }
        }
        mask >>= 1;
        left -= 1;
    }
    if input.remaining() != 0 {
        return Err(malformed(WHAT, "trailing SNAP bytes"));
    }
    Ok(out)
}

fn real(cursor: &mut Cursor<'_>) -> Result<f32> {
    let value = f32::from_bits(cursor.u32()?);
    if !value.is_finite() {
        return Err(malformed(WHAT, "non-finite number"));
    }
    Ok(value)
}

fn read_streams(
    attributes: &[Value],
    bytes: &[u8],
    count: usize,
    strings: &[Value],
) -> Result<Snapshot> {
    let mut positions = None;
    let mut normals = None;
    for attribute in attributes {
        let attr = attribute
            .as_object()
            .ok_or_else(|| malformed(WHAT, "attribute is not an object"))?;
        let name = attr
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed(WHAT, "missing attribute name"))?;
        let kind = attr
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed(WHAT, "missing attribute type"))?;
        if name.len() > 256 || kind.len() > 256 {
            return Err(malformed(WHAT, "attribute name too long"));
        }
        let start = integer(attr.get("data_offset"), MAX_BYTES, "attribute offset")?;
        let size = integer(attr.get("data_size"), MAX_BYTES, "attribute size")?;
        let stream = bytes
            .get(
                start
                    ..start
                        .checked_add(size)
                        .ok_or_else(|| malformed(WHAT, "attribute range overflow"))?,
            )
            .ok_or_else(|| malformed(WHAT, "attribute outside SNAP"))?;
        match (name, kind) {
            ("position", "float3") if positions.is_none() => {
                positions = Some(vectors(stream, count)?)
            }
            ("normal", "float3") if normals.is_none() => normals = Some(vectors(stream, count)?),
            ("skinning", "skinning") => {
                if let Some(why) = root_skinning(stream, count, strings)? {
                    return Ok(unsupported(why));
                }
            }
            _ => return Ok(unsupported(format!("attribute {name}:{kind}"))),
        }
    }
    let result = Snapshot::Points {
        positions: positions.ok_or_else(|| malformed(WHAT, "missing positions"))?,
        normals,
    };
    result.validate()?;
    Ok(result)
}
