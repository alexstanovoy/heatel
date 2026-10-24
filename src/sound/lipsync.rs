//! The lip sync a voice line carries in its `vsnd_c`, as Valve's sound compiler bakes it in:
//! its sentence (the phonemes heard, each over its stretch of time, and the emphasis curve)
//! and, in newer files, a morph animation (facial flex controller curves, named by the hash of
//! the controller's name). The client moves a speaking face by them (see its `face` module).

use crate::bytes::Cursor;
use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::resource::Resource;

const WHAT: &str = "vsnd lip sync";
/// Most phonemes, emphasis samples, keys and channels a line may have; the longest of the
/// install's voice lines has a few hundred phonemes and its morph curves ~300 keys of 9.
const MAX_PHONEMES: usize = 1 << 13;
const MAX_KEYS: usize = 1 << 14;
const MAX_CHANNELS: usize = 256;

/// One phoneme heard: from `start` to `end` seconds into the line, its code (the IPA symbol's
/// code point, as Valve's phoneme tables key them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phoneme {
    /// Seconds into the line it starts at.
    pub start: f32,
    /// Seconds into the line it ends at.
    pub end: f32,
    /// The phoneme: its IPA symbol's code point.
    pub code: u32,
}

/// Flex controller curves over the line: per key time a value of each channel, normalized to
/// its controller's range (0 is its minimum, 1 its maximum).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MorphCurves {
    /// Per channel, the controller's name hashed as Source 2 hashes string tokens (MurmurHash2
    /// of the lower-cased name, seed `0x31415926`).
    pub names: Vec<u32>,
    /// Seconds into the line, increasing.
    pub times: Vec<f32>,
    /// Per key, one value per channel.
    pub values: Vec<Vec<f32>>,
    /// Seconds over which the curves weigh in at their start and out at their end.
    pub ease_in: f32,
    /// See [`ease_in`](Self::ease_in).
    pub ease_out: f32,
}

/// What a line's face does while it is spoken.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lipsync {
    /// The phonemes heard, in time order.
    pub phonemes: Vec<Phoneme>,
    /// The emphasis curve: seconds and intensity (0.5 is plain speech).
    pub emphasis: Vec<(f32, f32)>,
    /// The line's baked controller curves, in newer files.
    pub morph: Option<MorphCurves>,
}

impl Lipsync {
    /// The lip sync of a `vsnd_c`: none for a sound without (an effect, or a line compiled
    /// without its phonemes).
    ///
    /// # Errors
    ///
    /// A resource that does not read, a phoneme or key without its numbers, numbers that are
    /// not finite or out of time order, morph keys of the wrong width, and more than 8192
    /// phonemes, 16384 keys or 256 channels.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let resource = Resource::parse(bytes)?;
        match resource.kv3(b"CTRL") {
            Some(ctrl) => match ctrl?.root.get("m_vSound") {
                Some(sound) => Self::from_ctrl(sound),
                None => Ok(Self::default()),
            },
            None => resource
                .block(b"DATA")
                .map_or_else(|| Ok(Self::default()), Self::from_data),
        }
    }

    /// Whether the line carries no lip sync at all.
    pub fn is_empty(&self) -> bool {
        self.phonemes.is_empty() && self.emphasis.is_empty() && self.morph.is_none()
    }

    /// The newer layout: `CTRL.m_vSound.m_Sentences`, the first sentence (a voice line has one).
    fn from_ctrl(sound: &Value) -> Result<Self> {
        let Some(sentence) = sound
            .get("m_Sentences")
            .and_then(Value::as_array)
            .and_then(<[Value]>::first)
        else {
            return Ok(Self::default());
        };
        let list = |key: &str| sentence.get(key).and_then(Value::as_array).unwrap_or(&[]);
        let float = |v: &Value, key: &str| v.get(key).and_then(Value::as_f64).map(|f| f as f32);
        let phonemes = bounded(list("m_RunTimePhonemes"), MAX_PHONEMES, "phonemes")?
            .iter()
            .map(|p| {
                let code = p.get("m_nPhonemeCode").and_then(Value::as_u64);
                match (float(p, "m_flStartTime"), float(p, "m_flEndTime"), code) {
                    (Some(start), Some(end), Some(code)) => Ok(Phoneme {
                        start,
                        end,
                        code: u32::try_from(code).map_err(|_| malformed(WHAT, "phoneme code"))?,
                    }),
                    _ => Err(malformed(WHAT, "a phoneme without its times or code")),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let emphasis = bounded(list("m_EmphasisSamples"), MAX_KEYS, "emphasis samples")?
            .iter()
            .map(|s| match (float(s, "time"), float(s, "value")) {
                (Some(t), Some(v)) => Ok((t, v)),
                _ => Err(malformed(WHAT, "an emphasis sample without time or value")),
            })
            .collect::<Result<Vec<_>>>()?;
        let morph = match sentence.get("m_morphData") {
            Some(data @ Value::Object(_)) => MorphCurves::from_kv3(data)?,
            _ => None,
        };
        Self {
            phonemes,
            emphasis,
            morph,
        }
        .checked()
    }

    /// The older binary layout: at byte 16 of `DATA` a `u32` offset (from its own field, 0 for
    /// none) to the sentence: a voice-duck flag (`u32`), then the phonemes and the emphasis
    /// samples, each an offset (from its own field) and a count. A phoneme is its start and end
    /// (`f32`) and its code (`u32`); an emphasis sample its time and value (`f32`).
    fn from_data(data: &[u8]) -> Result<Self> {
        let mut c = Cursor::new(data, WHAT);
        c.take(16)?;
        if c.remaining() < 4 {
            return Ok(Self::default());
        }
        let at = c.pos();
        let delta = c.u32()?;
        if delta == 0 {
            return Ok(Self::default());
        }
        let sentence = at
            .checked_add(delta as usize)
            .ok_or_else(|| malformed(WHAT, "sentence offset"))?;
        let mut c = Cursor::new(data, WHAT);
        c.take(sentence)?;
        c.u32()?;
        let phonemes = table(data, &mut c, 12, MAX_PHONEMES, |r| {
            Ok(Phoneme {
                start: f32::from_bits(r.u32()?),
                end: f32::from_bits(r.u32()?),
                code: r.u32()?,
            })
        })?;
        let emphasis = table(data, &mut c, 8, MAX_KEYS, |r| {
            Ok((f32::from_bits(r.u32()?), f32::from_bits(r.u32()?)))
        })?;
        Self {
            phonemes,
            emphasis,
            morph: None,
        }
        .checked()
    }

    /// Itself when every number is finite and every stretch runs forwards.
    fn checked(self) -> Result<Self> {
        let finite = self
            .phonemes
            .iter()
            .all(|p| p.start.is_finite() && p.end.is_finite() && p.start <= p.end)
            && self
                .emphasis
                .iter()
                .all(|(t, v)| t.is_finite() && v.is_finite());
        if !finite {
            return Err(malformed(
                WHAT,
                "a phoneme or sample out of order or not finite",
            ));
        }
        Ok(self)
    }
}

impl MorphCurves {
    /// `m_morphData`: `m_times`, the channels' `m_nameHashCodes` (or their `m_nameStrings`,
    /// hashed here), `m_samples` (a row per time) and the ease times. None when it has no keys.
    fn from_kv3(data: &Value) -> Result<Option<Self>> {
        let list = |key: &str| data.get(key).and_then(Value::as_array).unwrap_or(&[]);
        let times = bounded(list("m_times"), MAX_KEYS, "morph keys")?
            .iter()
            .map(|t| t.as_f64().map(|t| t as f32))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| malformed(WHAT, "a morph key time that is no number"))?;
        let hashes = list("m_nameHashCodes");
        let names: Vec<u32> = if hashes.is_empty() {
            bounded(list("m_nameStrings"), MAX_CHANNELS, "morph channels")?
                .iter()
                .map(|n| n.as_str().map(token))
                .collect::<Option<_>>()
        } else {
            bounded(hashes, MAX_CHANNELS, "morph channels")?
                .iter()
                .map(|h| h.as_u64().and_then(|h| u32::try_from(h).ok()))
                .collect::<Option<_>>()
        }
        .ok_or_else(|| malformed(WHAT, "a morph channel's name"))?;
        let rows = list("m_samples");
        if rows.len() != times.len() {
            return Err(malformed(WHAT, "morph samples and times differ in number"));
        }
        let mut values = Vec::with_capacity(rows.len());
        for row in rows {
            let row: Vec<f32> = row
                .as_array()
                .unwrap_or(&[])
                .iter()
                .map(|v| v.as_f64().map(|v| v as f32).filter(|v| v.is_finite()))
                .collect::<Option<_>>()
                .ok_or_else(|| malformed(WHAT, "a morph sample that is no number"))?;
            if row.len() != names.len() {
                return Err(malformed(WHAT, "a morph key of the wrong width"));
            }
            values.push(row);
        }
        let ordered = times.iter().all(|t| t.is_finite()) && times.windows(2).all(|w| w[0] <= w[1]);
        if !ordered {
            return Err(malformed(WHAT, "morph key times out of order"));
        }
        let ease = |key: &str| {
            data.get(key)
                .and_then(Value::as_f64)
                .map_or(0.0, |e| (e as f32).clamp(0.0, 60.0))
        };
        Ok((!times.is_empty() && !names.is_empty()).then(|| Self {
            names,
            times,
            values,
            ease_in: ease("m_flEaseIn"),
            ease_out: ease("m_flEaseOut"),
        }))
    }
}

/// Source 2's string token of `name`: MurmurHash2 of its lower-cased bytes, seed `0x31415926`.
pub fn token(name: &str) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let data = name.to_lowercase().into_bytes();
    let mut h = 0x3141_5926 ^ data.len() as u32;
    let (words, tail) = data.as_chunks::<4>();
    for word in words {
        let mut k = u32::from_le_bytes(*word).wrapping_mul(M);
        k ^= k >> 24;
        h = h.wrapping_mul(M) ^ k.wrapping_mul(M);
    }
    if !tail.is_empty() {
        for (i, &b) in tail.iter().enumerate().rev() {
            h ^= u32::from(b) << (8 * i);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

fn bounded<'a>(list: &'a [Value], max: usize, what: &str) -> Result<&'a [Value]> {
    if list.len() > max {
        return Err(malformed(WHAT, format!("more than {max} {what}")));
    }
    Ok(list)
}

/// A table of the old layout at the cursor: an offset (from its own field) and a count of
/// `size`-byte records, read with `record`.
fn table<T>(
    data: &[u8],
    c: &mut Cursor<'_>,
    size: usize,
    max: usize,
    mut record: impl FnMut(&mut Cursor<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let at = c.offset32()?;
    let count = c.len32()?;
    if count == 0 {
        return Ok(Vec::new());
    }
    if count > max {
        return Err(malformed(WHAT, format!("{count} records, more than {max}")));
    }
    let bytes = data
        .get(at..)
        .and_then(|rest| rest.get(..count * size))
        .ok_or_else(|| malformed(WHAT, "records past the block"))?;
    let mut r = Cursor::new(bytes, WHAT);
    (0..count).map(|_| record(&mut r)).collect()
}

#[cfg(test)]
mod tests {
    use super::token;

    /// The hashes a voice line of Pudge's names its curves by are his flex controllers' names.
    #[test]
    fn tokens_hash_as_source_2_does() {
        assert_eq!(token("outerBrowRaiser"), 2_745_760_859);
        assert_eq!(token("eyeClosedAndLidRaiser"), 2_316_792_926);
        assert_eq!(token("squint"), 2_234_846_163);
        assert_eq!(token("lipStretcher"), 2_177_083_055);
    }
}
