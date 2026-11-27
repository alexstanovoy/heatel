//! Unsigned BPTC (BC6H), retaining the exact decoded half-float RGB channels.

#[derive(Clone, Copy)]
enum Endpoint {
    R0,
    G0,
    B0,
    R1,
    G1,
    B1,
    R2,
    G2,
    B2,
    R3,
    G3,
    B3,
}

use Endpoint::*;

struct Field {
    endpoint: Endpoint,
    low: u8,
    /// Negative when bits occur in reverse order.
    width: i8,
}

impl Endpoint {
    const fn field(self, low: u8, width: i8) -> Field {
        Field {
            endpoint: self,
            low,
            width,
        }
    }
}

struct Mode {
    id: u8,
    precision: u8,
    /// Zero for a mode that stores absolute endpoints.
    delta: [u8; 3],
    fields: &'static [Field],
}

impl Mode {
    const fn new(id: u8, precision: u8, delta: [u8; 3], fields: &'static [Field]) -> Self {
        Self {
            id,
            precision,
            delta,
            fields,
        }
    }

    fn endpoints(&self, bits: &mut u128) -> [[u32; 3]; 4] {
        let mut values = [[0; 3]; 4];
        for field in self.fields {
            let width = field.width.unsigned_abs();
            let mut value = take(bits, width);
            if field.width < 0 {
                value = value.reverse_bits() >> (32 - width);
            }
            let slot = field.endpoint as usize;
            values[slot / 3][slot % 3] |= value << field.low;
        }
        if self.delta[0] != 0 {
            let first = values[0];
            for endpoint in &mut values[1..] {
                for (channel, value) in endpoint.iter_mut().enumerate() {
                    let sign = 1 << (self.delta[channel] - 1);
                    let delta = (*value ^ sign).wrapping_sub(sign);
                    *value = first[channel].wrapping_add(delta) & ((1 << self.precision) - 1);
                }
            }
        }
        values.map(|rgb| rgb.map(|value| unquantize(value, self.precision)))
    }
}

pub(super) fn block(data: &[u8]) -> [[u16; 3]; 16] {
    let mut bits = u128::from_le_bytes(data.try_into().expect("one BC6H block"));
    let short_mode = bits & 2 == 0;
    let id = take(&mut bits, if short_mode { 2 } else { 5 }) as u8;
    let Some(mode) = MODES.iter().find(|mode| mode.id == id) else {
        return [[0; 3]; 16];
    };
    let endpoints = mode.endpoints(&mut bits);
    let (partition, anchor, index_bits, weights) = if id & 3 == 3 {
        (0, 0, 4, &WEIGHTS_4[..])
    } else {
        let (partition, anchor) = PARTITIONS[take(&mut bits, 5) as usize];
        (partition, anchor, 3, &WEIGHTS_3[..])
    };
    std::array::from_fn(|i| {
        let slice = usize::from((partition >> i) & 1) * 2;
        let width = index_bits - u8::from(i == 0 || i == anchor);
        let weight = weights[take(&mut bits, width) as usize];
        std::array::from_fn(|channel| {
            let first = endpoints[slice][channel];
            let last = endpoints[slice + 1][channel];
            let interpolated = (first * (64 - weight) + last * weight + 32) >> 6;
            ((interpolated * 31) >> 6) as u16
        })
    })
}

fn take(bits: &mut u128, width: u8) -> u32 {
    let value = *bits as u32 & ((1 << width) - 1);
    *bits >>= width;
    value
}

fn unquantize(value: u32, precision: u8) -> u32 {
    if precision >= 15 || value == 0 {
        value
    } else if value == (1 << precision) - 1 {
        65535
    } else {
        ((value << 15) + 0x4000) >> (precision - 1)
    }
}

const WEIGHTS_3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const WEIGHTS_4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

// Mode layouts and partition masks from the Khronos Data Format Specification, BPTC.
// Endpoint fields are in on-disk order after the mode bits.
#[rustfmt::skip]
const MODES: [Mode; 14] = [
    Mode::new( 0, 10, [5, 5, 5], &[
        G2.field(4, 1), B2.field(4, 1), B3.field(4, 1), R0.field(0, 10),
        G0.field(0, 10), B0.field(0, 10), R1.field(0, 5), G3.field(4, 1),
        G2.field(0, 4), G1.field(0, 5), B3.field(0, 1), G3.field(0, 4),
        B1.field(0, 5), B3.field(1, 1), B2.field(0, 4), R2.field(0, 5),
        B3.field(2, 1), R3.field(0, 5), B3.field(3, 1),
    ]),
    Mode::new( 1,  7, [6, 6, 6], &[
        G2.field(5, 1), G3.field(4, 1), G3.field(5, 1), R0.field(0, 7),
        B3.field(0, 1), B3.field(1, 1), B2.field(4, 1), G0.field(0, 7),
        B2.field(5, 1), B3.field(2, 1), G2.field(4, 1), B0.field(0, 7),
        B3.field(3, 1), B3.field(5, 1), B3.field(4, 1), R1.field(0, 6),
        G2.field(0, 4), G1.field(0, 6), G3.field(0, 4), B1.field(0, 6),
        B2.field(0, 4), R2.field(0, 6), R3.field(0, 6),
    ]),
    Mode::new( 2, 11, [5, 4, 4], &[
        R0.field(0, 10), G0.field(0, 10), B0.field(0, 10), R1.field(0, 5),
        R0.field(10, 1), G2.field(0, 4), G1.field(0, 4), G0.field(10, 1),
        B3.field(0, 1), G3.field(0, 4), B1.field(0, 4), B0.field(10, 1),
        B3.field(1, 1), B2.field(0, 4), R2.field(0, 5), B3.field(2, 1),
        R3.field(0, 5), B3.field(3, 1),
    ]),
    Mode::new( 6, 11, [4, 5, 4], &[
        R0.field(0, 10), G0.field(0, 10), B0.field(0, 10), R1.field(0, 4),
        R0.field(10, 1), G3.field(4, 1), G2.field(0, 4), G1.field(0, 5),
        G0.field(10, 1), G3.field(0, 4), B1.field(0, 4), B0.field(10, 1),
        B3.field(1, 1), B2.field(0, 4), R2.field(0, 4), B3.field(0, 1),
        B3.field(2, 1), R3.field(0, 4), G2.field(4, 1), B3.field(3, 1),
    ]),
    Mode::new(10, 11, [4, 4, 5], &[
        R0.field(0, 10), G0.field(0, 10), B0.field(0, 10), R1.field(0, 4),
        R0.field(10, 1), B2.field(4, 1), G2.field(0, 4), G1.field(0, 4),
        G0.field(10, 1), B3.field(0, 1), G3.field(0, 4), B1.field(0, 5),
        B0.field(10, 1), B2.field(0, 4), R2.field(0, 4), B3.field(1, 1),
        B3.field(2, 1), R3.field(0, 4), B3.field(4, 1), B3.field(3, 1),
    ]),
    Mode::new(14,  9, [5, 5, 5], &[
        R0.field(0, 9), B2.field(4, 1), G0.field(0, 9), G2.field(4, 1),
        B0.field(0, 9), B3.field(4, 1), R1.field(0, 5), G3.field(4, 1),
        G2.field(0, 4), G1.field(0, 5), B3.field(0, 1), G3.field(0, 4),
        B1.field(0, 5), B3.field(1, 1), B2.field(0, 4), R2.field(0, 5),
        B3.field(2, 1), R3.field(0, 5), B3.field(3, 1),
    ]),
    Mode::new(18,  8, [6, 5, 5], &[
        R0.field(0, 8), G3.field(4, 1), B2.field(4, 1), G0.field(0, 8),
        B3.field(2, 1), G2.field(4, 1), B0.field(0, 8), B3.field(3, 1),
        B3.field(4, 1), R1.field(0, 6), G2.field(0, 4), G1.field(0, 5),
        B3.field(0, 1), G3.field(0, 4), B1.field(0, 5), B3.field(1, 1),
        B2.field(0, 4), R2.field(0, 6), R3.field(0, 6),
    ]),
    Mode::new(22,  8, [5, 6, 5], &[
        R0.field(0, 8), B3.field(0, 1), B2.field(4, 1), G0.field(0, 8),
        G2.field(5, 1), G2.field(4, 1), B0.field(0, 8), G3.field(5, 1),
        B3.field(4, 1), R1.field(0, 5), G3.field(4, 1), G2.field(0, 4),
        G1.field(0, 6), G3.field(0, 4), B1.field(0, 5), B3.field(1, 1),
        B2.field(0, 4), R2.field(0, 5), B3.field(2, 1), R3.field(0, 5),
        B3.field(3, 1),
    ]),
    Mode::new(26,  8, [5, 5, 6], &[
        R0.field(0, 8), B3.field(1, 1), B2.field(4, 1), G0.field(0, 8),
        B2.field(5, 1), G2.field(4, 1), B0.field(0, 8), B3.field(5, 1),
        B3.field(4, 1), R1.field(0, 5), G3.field(4, 1), G2.field(0, 4),
        G1.field(0, 5), B3.field(0, 1), G3.field(0, 4), B1.field(0, 6),
        B2.field(0, 4), R2.field(0, 5), B3.field(2, 1), R3.field(0, 5),
        B3.field(3, 1),
    ]),
    Mode::new(30,  6, [0; 3], &[
        R0.field(0, 6), G3.field(4, 1), B3.field(0, 1), B3.field(1, 1),
        B2.field(4, 1), G0.field(0, 6), G2.field(5, 1), B2.field(5, 1),
        B3.field(2, 1), G2.field(4, 1), B0.field(0, 6), G3.field(5, 1),
        B3.field(3, 1), B3.field(5, 1), B3.field(4, 1), R1.field(0, 6),
        G2.field(0, 4), G1.field(0, 6), G3.field(0, 4), B1.field(0, 6),
        B2.field(0, 4), R2.field(0, 6), R3.field(0, 6),
    ]),
    Mode::new( 3, 10, [0; 3], &[
        R0.field(0, 10), G0.field(0, 10), B0.field(0, 10), R1.field(0, 10),
        G1.field(0, 10), B1.field(0, 10),
    ]),
    Mode::new( 7, 11, [9, 9, 9], &[
        R0.field(0, 10), G0.field(0, 10), B0.field(0, 10), R1.field(0, 9),
        R0.field(10, 1), G1.field(0, 9), G0.field(10, 1), B1.field(0, 9),
        B0.field(10, 1),
    ]),
    Mode::new(11, 12, [8, 8, 8], &[
        R0.field(0, 10), G0.field(0, 10), B0.field(0, 10), R1.field(0, 8),
        R0.field(10, -2), G1.field(0, 8), G0.field(10, -2), B1.field(0, 8),
        B0.field(10, -2),
    ]),
    Mode::new(15, 16, [4, 4, 4], &[
        R0.field(0, 10), G0.field(0, 10), B0.field(0, 10), R1.field(0, 4),
        R0.field(10, -6), G1.field(0, 4), G0.field(10, -6), B1.field(0, 4),
        B0.field(10, -6),
    ]),
];

// One subset bit per texel, and the index of subset 1's anchor.
const PARTITIONS: [(u16, usize); 32] = [
    (0xcccc, 15),
    (0x8888, 15),
    (0xeeee, 15),
    (0xecc8, 15),
    (0xc880, 15),
    (0xfeec, 15),
    (0xfec8, 15),
    (0xec80, 15),
    (0xc800, 15),
    (0xffec, 15),
    (0xfe80, 15),
    (0xe800, 15),
    (0xffe8, 15),
    (0xff00, 15),
    (0xfff0, 15),
    (0xf000, 15),
    (0xf710, 15),
    (0x008e, 2),
    (0x7100, 8),
    (0x08ce, 2),
    (0x008c, 2),
    (0x7310, 8),
    (0x3100, 8),
    (0x8cce, 15),
    (0x088c, 2),
    (0x3110, 8),
    (0x6666, 2),
    (0x366c, 2),
    (0x17e8, 8),
    (0x0ff0, 8),
    (0x718e, 2),
    (0x399c, 2),
];
