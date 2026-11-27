//! Unsigned EAC channels and the ETC2 alpha plane. Indices are column major on disk.

pub(super) fn alpha(data: &[u8]) -> [u8; 16] {
    let base = i16::from(data[0]);
    let multiplier = i16::from(data[1] >> 4);
    values(data, base, multiplier, 255).map(|value| value as u8)
}

pub(super) fn red(data: &[u8]) -> [[f32; 3]; 16] {
    unsigned(data).map(|red| [red, 0.0, 0.0])
}

pub(super) fn red_green(data: &[u8]) -> [[f32; 3]; 16] {
    let red = unsigned(&data[..8]);
    let green = unsigned(&data[8..]);
    std::array::from_fn(|i| [red[i], green[i], 0.0])
}

fn unsigned(data: &[u8]) -> [f32; 16] {
    let base = i16::from(data[0]) * 8 + 4;
    let multiplier = (i16::from(data[1] >> 4) * 8).max(1);
    values(data, base, multiplier, 2047).map(|value| f32::from(value) / 2047.0)
}

fn values(data: &[u8], base: i16, multiplier: i16, max: i16) -> [i16; 16] {
    let bits = u64::from_be_bytes(data.try_into().expect("one EAC channel"));
    let modifiers = MODIFIERS[(data[1] & 15) as usize];
    std::array::from_fn(|i| {
        let column_order = i % 4 * 4 + i / 4;
        let index = ((bits >> (45 - column_order * 3)) & 7) as usize;
        (base + modifiers[index] * multiplier).clamp(0, max)
    })
}

// ETC2/EAC modifier table from the Khronos Data Format Specification.
const MODIFIERS: [[i16; 8]; 16] = [
    [-3, -6, -9, -15, 2, 5, 8, 14],
    [-3, -7, -10, -13, 2, 6, 9, 12],
    [-2, -5, -8, -13, 1, 4, 7, 12],
    [-2, -4, -6, -13, 1, 3, 5, 12],
    [-3, -6, -8, -12, 2, 5, 7, 11],
    [-3, -7, -9, -11, 2, 6, 8, 10],
    [-4, -7, -8, -11, 3, 6, 7, 10],
    [-3, -5, -8, -11, 2, 4, 7, 10],
    [-2, -6, -8, -10, 1, 5, 7, 9],
    [-2, -5, -8, -10, 1, 4, 7, 9],
    [-2, -4, -8, -10, 1, 3, 7, 9],
    [-2, -5, -7, -10, 1, 4, 6, 9],
    [-3, -4, -7, -10, 2, 3, 6, 9],
    [-1, -2, -3, -10, 0, 1, 2, 9],
    [-4, -6, -8, -9, 3, 5, 7, 8],
    [-3, -5, -7, -9, 2, 4, 6, 8],
];
