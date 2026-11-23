//! Source-to-glTF coordinate transforms: Source is Z-up in inches, glTF is Y-up in metres.

/// Inches to metres.
pub(crate) const SCALE: f32 = 0.0254;
/// Source's Z-up frame to glTF's Y-up one: a rotation taking `(x, y, z)` to `(y, z, x)`.
pub(crate) const TO_GLTF: [f32; 4] = [-0.5, -0.5, -0.5, 0.5];

/// A number as glTF may hold it: NaN and infinities (only a damaged file has them) become 0.
pub(crate) fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// Whether a bone rest scale is finite, non-zero, and invertible.
pub(crate) fn valid_bone_scale(scale: f32) -> bool {
    scale.is_finite() && scale != 0.0 && scale.recip().is_finite()
}

/// Normalizes a quaternion in `f64`, returning `None` for zero or non-finite rotations.
pub(crate) fn unit_rotation(rotation: [f32; 4]) -> Option<[f32; 4]> {
    let length = rotation
        .into_iter()
        .map(|value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    (length.is_finite() && length > 0.0)
        .then(|| rotation.map(|value| (f64::from(value) / length) as f32))
}

/// Rotates `v` by the unit quaternion `q` (`[x, y, z, w]`).
pub(crate) fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

/// The quaternion product `a * b` (apply `b`, then `a`).
pub(crate) fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// A position in Source inches converted to glTF metres.
pub(crate) fn position(v: [f32; 3]) -> [f32; 3] {
    rotate(TO_GLTF, v.map(|c| c * SCALE)).map(finite)
}

/// A direction vector in Source's frame rotated to glTF's frame.
pub(crate) fn direction(v: [f32; 3]) -> [f32; 3] {
    rotate(TO_GLTF, v).map(finite)
}

/// A bone-local transform in glTF's frame: the translation scaled to metres and, on a root
/// bone, the frame change baked in (children inherit it through the hierarchy).
pub(crate) fn bone_to_gltf(
    position: [f32; 3],
    rotation: [f32; 4],
    root: bool,
) -> ([f32; 3], [f32; 4]) {
    let translation = position.map(|v| v * SCALE);
    let (translation, rotation) = if root {
        (rotate(TO_GLTF, translation), multiply(TO_GLTF, rotation))
    } else {
        (translation, rotation)
    };
    (translation.map(finite), rotation.map(finite))
}
