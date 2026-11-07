//! Small synthetic legacy KV3 documents; no game install is needed.
use std::collections::HashMap;

#[allow(dead_code)] // Each integration target uses only some value shapes.
pub enum K {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Arr(Vec<K>),
    Obj(Vec<(&'static str, K)>),
}

pub fn s(text: &str) -> K {
    K::Str(text.to_owned())
}

pub fn kv3_file(root: &K) -> Vec<u8> {
    let mut table = Vec::new();
    strings(root, &mut table);
    let mut ids = HashMap::new();
    let mut unique = Vec::new();
    for text in table {
        ids.entry(text).or_insert_with(|| {
            unique.push(text);
            unique.len() as i32 - 1
        });
    }
    let mut out = b"VKV\x03".to_vec();
    out.extend([
        0x00, 0x05, 0x86, 0x1B, 0xD8, 0xF7, 0xC1, 0x40, 0xAD, 0x82, 0x75, 0xA4, 0x82, 0x67, 0xE7,
        0x14,
    ]);
    out.extend([0; 16]);
    out.extend((unique.len() as u32).to_le_bytes());
    for text in unique {
        out.extend(text.as_bytes());
        out.push(0);
    }
    value(root, &ids, &mut out);
    out.extend(u32::MAX.to_le_bytes());
    out
}

fn strings<'a>(value: &'a K, out: &mut Vec<&'a str>) {
    match value {
        K::Str(text) => out.push(text),
        K::Arr(items) => items.iter().for_each(|item| strings(item, out)),
        K::Obj(members) => members.iter().for_each(|(key, value)| {
            out.push(key);
            strings(value, out);
        }),
        _ => {}
    }
}

fn value(input: &K, ids: &HashMap<&str, i32>, out: &mut Vec<u8>) {
    match input {
        K::Bool(value) => out.extend([2, u8::from(*value)]),
        K::Int(value) => out.extend([3].into_iter().chain(value.to_le_bytes())),
        K::UInt(value) => out.extend([4].into_iter().chain(value.to_le_bytes())),
        K::Float(value) => out.extend([5].into_iter().chain(value.to_le_bytes())),
        K::Str(value) => out.extend([6].into_iter().chain(ids[value.as_str()].to_le_bytes())),
        K::Arr(items) => {
            out.extend([8].into_iter().chain((items.len() as i32).to_le_bytes()));
            items.iter().for_each(|item| value(item, ids, out));
        }
        K::Obj(members) => {
            out.extend([9].into_iter().chain((members.len() as i32).to_le_bytes()));
            for (key, item) in members {
                out.extend(ids[key].to_le_bytes());
                value(item, ids, out);
            }
        }
    }
}
