//! The inputs a function reads its numbers from (`CPerParticleFloatInput`,
//! `CPerParticleVecInput`): a constant, a random draw, or an attribute or age mapped through a
//! remap or a curve.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::{Result, malformed};
use crate::kv3::Value;

use super::field::Field;
use super::read::{Obj, WHAT};
use super::transform::ControlPoint;

/// Most keys a curve, or stops a gradient, has; Dota's have a handful.
const MAX_KEYS: usize = 64;

/// A number input.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum FloatInput {
    /// A constant.
    Literal(f32),
    /// A uniform draw in `min..=max`, biased when `bias` says so.
    Random {
        /// Low end.
        min: f32,
        /// High end.
        max: f32,
        /// Whether a particle keeps its draw (`Constant`) or draws anew each time.
        varying: bool,
        /// The bias of the draw, for `PF_TYPE_RANDOM_BIASED`.
        bias: Option<Bias>,
        /// Whether the sign flips at random, half of the time.
        sign_flip: bool,
    },
    /// The system's age.
    CollectionAge(Mapping),
    /// How long the end cap has run, 0 before it.
    EndCapAge(Mapping),
    /// The particle's age in seconds.
    ParticleAge(Mapping),
    /// The particle's age over its lifetime.
    ParticleAgeNormalized(Mapping),
    /// A float attribute of the particle, now or at spawn; angles in degrees.
    ParticleFloat {
        /// The attribute.
        field: Field,
        /// At spawn rather than now.
        initial: bool,
        /// Its mapping.
        mapping: Mapping,
    },
    /// The particle's speed.
    ParticleSpeed(Mapping),
    /// The particle's number in emission order.
    ParticleNumber {
        /// Counted from the last.
        reverse: bool,
        /// Its mapping.
        mapping: Mapping,
    },
    /// The particle's number over the particle count.
    ParticleNumberNormalized(Mapping),
    /// A component of a control point's position.
    ControlPointComponent {
        /// The control point.
        control_point: ControlPoint,
        /// The component, 0..=2.
        component: u8,
        /// Its mapping.
        mapping: Mapping,
    },
    /// A kind this crate does not read: its literal value stands in.
    Unsupported {
        /// The kind (`PF_TYPE_...`).
        kind: String,
        /// Its `m_flLiteralValue`.
        literal: f32,
    },
}

/// The bias of a random draw or a remap.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Bias {
    /// Exponential rather than standard (gain).
    pub exponential: bool,
    /// The bias parameter, -1..1.
    pub parameter: f32,
}

/// How an input's raw value becomes its value (`PF_MAP_TYPE_...`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum Mapping {
    /// As it is.
    Direct,
    /// Times a factor.
    Mult(f32),
    /// `input` remapped onto `output`, clamped to it.
    Remap {
        /// The input range.
        input: [f32; 2],
        /// The output range.
        output: [f32; 2],
        /// The input wraps at `input[1]`.
        looped: bool,
        /// A bias of the remap.
        bias: Option<Bias>,
    },
    /// Through a curve.
    Curve(Curve),
    /// `inside` within `range`, `outside` elsewhere.
    Notched {
        /// The range.
        range: [f32; 2],
        /// The value within.
        inside: f32,
        /// The value outside.
        outside: f32,
    },
    /// Rounded: -1 down, 0 to the nearest, 1 up.
    Round(i8),
    /// At most the value.
    Min(f32),
    /// At least the value.
    Max(f32),
    /// Modulo the value.
    Mod(f32),
}

/// A piecewise cubic Hermite curve: keys `[x, y, slope in, slope out]` by increasing `x`, the
/// output clamped to `domain_min[1]..=domain_max[1]`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Curve {
    /// The keys.
    pub keys: Vec<[f32; 4]>,
    /// The domain's low corner.
    pub domain_min: [f32; 2],
    /// The domain's high corner.
    pub domain_max: [f32; 2],
    /// The input wraps at `domain_max[0]`.
    pub looped: bool,
}

/// A vector input.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum VectorInput {
    /// A constant (a literal colour is 0..1).
    Literal([f32; 3]),
    /// A uniform draw per component.
    Random {
        /// Low corner.
        min: [f32; 3],
        /// High corner.
        max: [f32; 3],
    },
    /// A uniform draw per component added to a vector attribute of the particle times a scale.
    RandomOffset {
        /// Low corner of the draw.
        min: [f32; 3],
        /// High corner.
        max: [f32; 3],
        /// The attribute.
        field: Field,
        /// Its scale.
        scale: [f32; 3],
    },
    /// A vector attribute of the particle times a scale.
    ParticleVector {
        /// The attribute.
        field: Field,
        /// At spawn rather than now.
        initial: bool,
        /// Its scale.
        scale: [f32; 3],
    },
    /// The particle's velocity.
    ParticleVelocity,
    /// A control point's position times a scale.
    ControlPointValue {
        /// The control point.
        control_point: ControlPoint,
        /// Its scale.
        scale: [f32; 3],
    },
    /// One control point's position less another's.
    ControlPointDelta {
        /// The control point.
        control_point: ControlPoint,
        /// The one it is taken from.
        delta: ControlPoint,
    },
    /// A float input lerped from `output[0]` to `output[1]` as it goes `input[0]..input[1]`.
    FloatInterp {
        /// The float.
        input: Box<FloatInput>,
        /// Its range.
        range: [f32; 2],
        /// The vectors at its ends.
        output: [[f32; 3]; 2],
        /// Clamped to the ends.
        clamped: bool,
    },
    /// A float input through a colour gradient: stops `[position, r, g, b]` (0..1), sorted.
    Gradient {
        /// The float.
        input: Box<FloatInput>,
        /// Its range.
        range: [f32; 2],
        /// The stops.
        stops: Vec<[f32; 4]>,
    },
    /// Three float inputs.
    Components(Box<[FloatInput; 3]>),
    /// A kind this crate does not read: its literal value stands in.
    Unsupported {
        /// The kind (`PVEC_TYPE_...`).
        kind: String,
        /// Its `m_vLiteralValue`.
        literal: [f32; 3],
    },
}

const MAP_TYPES: [&str; 10] = [
    "PF_MAP_TYPE_DIRECT",
    "PF_MAP_TYPE_MULT",
    "PF_MAP_TYPE_REMAP",
    "PF_MAP_TYPE_REMAP_BIASED",
    "PF_MAP_TYPE_CURVE",
    "PF_MAP_TYPE_NOTCHED",
    "PF_MAP_TYPE_ROUND",
    "PF_MAP_TYPE_MIN",
    "PF_MAP_TYPE_MAX",
    "PF_MAP_TYPE_MOD",
];

impl FloatInput {
    /// The input member `key` of `obj`, `default` when absent.
    pub(super) fn read(obj: &Obj<'_>, key: &str, default: f32) -> Result<Self> {
        match obj.get(key) {
            None => Ok(Self::Literal(default)),
            Some(Value::Object(_)) => Self::parse(&Obj::new(obj.get(key).unwrap_or(&Value::Null))?),
            Some(_) => Ok(Self::Literal(obj.float(key, default)?)),
        }
    }

    fn parse(input: &Obj<'_>) -> Result<Self> {
        let kind = input.str("m_nType")?.unwrap_or("PF_TYPE_LITERAL");
        let mapping = || Mapping::read(input);
        Ok(match kind {
            "PF_TYPE_LITERAL" => Self::Literal(input.float("m_flLiteralValue", 0.0)?),
            "PF_TYPE_RANDOM_UNIFORM" | "PF_TYPE_RANDOM_BIASED" => Self::Random {
                min: input.float("m_flRandomMin", 0.0)?,
                max: input.float("m_flRandomMax", 1.0)?,
                varying: input.str("m_nRandomMode")? == Some("PF_RANDOM_MODE_VARYING"),
                bias: (kind == "PF_TYPE_RANDOM_BIASED")
                    .then(|| Bias::read(input))
                    .transpose()?,
                sign_flip: input.bool("m_bHasRandomSignFlip", false)?,
            },
            "PF_TYPE_COLLECTION_AGE" => Self::CollectionAge(mapping()?),
            "PF_TYPE_ENDCAP_AGE" => Self::EndCapAge(mapping()?),
            "PF_TYPE_PARTICLE_AGE" => Self::ParticleAge(mapping()?),
            "PF_TYPE_PARTICLE_AGE_NORMALIZED" => Self::ParticleAgeNormalized(mapping()?),
            "PF_TYPE_PARTICLE_FLOAT" | "PF_TYPE_PARTICLE_INITIAL_FLOAT" => Self::ParticleFloat {
                field: input.field("m_nScalarAttribute", Field::RADIUS)?,
                initial: kind == "PF_TYPE_PARTICLE_INITIAL_FLOAT",
                mapping: mapping()?,
            },
            "PF_TYPE_PARTICLE_SPEED" => Self::ParticleSpeed(mapping()?),
            "PF_TYPE_PARTICLE_NUMBER" => Self::ParticleNumber {
                reverse: input.bool("m_bReverseOrder", false)?,
                mapping: mapping()?,
            },
            "PF_TYPE_PARTICLE_NUMBER_NORMALIZED" => Self::ParticleNumberNormalized(mapping()?),
            "PF_TYPE_CONTROL_POINT_COMPONENT" => Self::ControlPointComponent {
                control_point: input.control_point("m_nControlPoint", 0)?,
                component: input.int_clamped("m_nVectorComponent", 0, 0, 2)? as u8,
                mapping: mapping()?,
            },
            other => Self::Unsupported {
                kind: other.to_owned(),
                literal: input.float("m_flLiteralValue", 0.0)?,
            },
        })
    }

    /// Its value when it is a constant.
    pub fn literal(&self) -> Option<f32> {
        match self {
            Self::Literal(value) => Some(*value),
            _ => None,
        }
    }

    /// The same input with its output negated.
    pub(super) fn negated(self) -> Self {
        match self {
            Self::Literal(value) => Self::Literal(-value),
            Self::Random {
                min,
                max,
                varying,
                bias,
                sign_flip,
            } => Self::Random {
                min: -min,
                max: -max,
                varying,
                bias,
                sign_flip,
            },
            Self::Unsupported { kind, literal } => Self::Unsupported {
                kind,
                literal: -literal,
            },
            // A mapped input's output: a scale, a range or a curve negated.
            other => other.map_mapping(Mapping::negated),
        }
    }

    fn map_mapping(self, f: fn(Mapping) -> Mapping) -> Self {
        match self {
            Self::CollectionAge(m) => Self::CollectionAge(f(m)),
            Self::EndCapAge(m) => Self::EndCapAge(f(m)),
            Self::ParticleAge(m) => Self::ParticleAge(f(m)),
            Self::ParticleAgeNormalized(m) => Self::ParticleAgeNormalized(f(m)),
            Self::ParticleFloat {
                field,
                initial,
                mapping,
            } => Self::ParticleFloat {
                field,
                initial,
                mapping: f(mapping),
            },
            Self::ParticleSpeed(m) => Self::ParticleSpeed(f(m)),
            Self::ParticleNumber { reverse, mapping } => Self::ParticleNumber {
                reverse,
                mapping: f(mapping),
            },
            Self::ParticleNumberNormalized(m) => Self::ParticleNumberNormalized(f(m)),
            Self::ControlPointComponent {
                control_point,
                component,
                mapping,
            } => {
                // A direct read negates as a factor of -1.
                let mapping = if mapping == Mapping::Direct {
                    Mapping::Mult(1.0)
                } else {
                    mapping
                };
                Self::ControlPointComponent {
                    control_point,
                    component,
                    mapping: f(mapping),
                }
            }
            other => other,
        }
    }
}

impl Bias {
    fn read(obj: &Obj<'_>) -> Result<Self> {
        let kinds = [
            "PF_BIAS_TYPE_STANDARD",
            "PF_BIAS_TYPE_GAIN",
            "PF_BIAS_TYPE_EXPONENTIAL",
        ];
        Ok(Self {
            exponential: obj.choice("m_nBiasType", &kinds, 0)? == 2,
            parameter: obj.float("m_flBiasParameter", 0.0)?,
        })
    }
}

impl Mapping {
    pub(super) fn read(input: &Obj<'_>) -> Result<Self> {
        let looped = input.str("m_nInputMode")? == Some("PF_INPUT_MODE_LOOPED");
        Ok(match input.choice("m_nMapType", &MAP_TYPES, 0)? {
            0 => Self::Direct,
            1 => Self::Mult(input.float("m_flMultFactor", 1.0)?),
            kind @ (2 | 3) => Self::Remap {
                input: [
                    input.float("m_flInput0", 0.0)?,
                    input.float("m_flInput1", 1.0)?,
                ],
                output: [
                    input.float("m_flOutput0", 0.0)?,
                    input.float("m_flOutput1", 1.0)?,
                ],
                looped,
                bias: (kind == 3).then(|| Bias::read(input)).transpose()?,
            },
            4 => Self::Curve(Curve::read(input.object("m_Curve")?, looped)?),
            5 => {
                let (a, b) = (
                    input.float("m_flNotchedRangeMin", 0.0)?,
                    input.float("m_flNotchedRangeMax", 1.0)?,
                );
                Self::Notched {
                    range: [a.min(b), a.max(b)],
                    inside: input.float("m_flNotchedOutputInside", 1.0)?,
                    outside: input.float("m_flNotchedOutputOutside", 0.0)?,
                }
            }
            6 => Self::Round(match input.str("m_nRoundType")? {
                Some("PF_ROUND_TYPE_FLOOR") => -1,
                Some("PF_ROUND_TYPE_CEIL") => 1,
                _ => 0,
            }),
            kind => {
                let value = input.float("m_flCompareValue", 0.0)?;
                [Self::Min(value), Self::Max(value), Self::Mod(value)][kind - 7].clone()
            }
        })
    }

    fn negated(self) -> Self {
        match self {
            Self::Direct => Self::Mult(-1.0),
            Self::Mult(factor) => Self::Mult(-factor),
            Self::Remap {
                input,
                output,
                looped,
                bias,
            } => Self::Remap {
                input,
                output: [-output[0], -output[1]],
                looped,
                bias,
            },
            Self::Notched {
                range,
                inside,
                outside,
            } => Self::Notched {
                range,
                inside: -inside,
                outside: -outside,
            },
            Self::Curve(mut curve) => {
                curve.domain_min[1] = -curve.domain_min[1];
                curve.domain_max[1] = -curve.domain_max[1];
                Self::Curve(curve)
            }
            other => other,
        }
    }
}

/// The curve's tangent kinds, in `CURVE_TANGENT_...` order of what follows.
#[derive(Clone, Copy, PartialEq)]
enum Tangent {
    Linear,
    Spline,
    Free,
    Mirror,
    Sine,
    Flat,
}

/// The slopes Source 2 gives its sine tangents.
const SINE_STEEP: f32 = 1.60305;
const SINE_SHALLOW: f32 = 0.041_337_7;

impl Curve {
    fn read(curve: Option<Obj<'_>>, looped: bool) -> Result<Self> {
        let Some(curve) = curve else {
            return Ok(Self {
                keys: Vec::new(),
                domain_min: [0.0; 2],
                domain_max: [0.0; 2],
                looped,
            });
        };
        let splines = curve.array("m_spline", MAX_KEYS)?;
        let tangents = curve.array("m_tangents", MAX_KEYS)?;
        let mut keys: Vec<[f32; 4]> = Vec::with_capacity(splines.len());
        let mut kinds = Vec::with_capacity(splines.len());
        for (spline, tangent) in splines.iter().zip(tangents) {
            let (spline, tangent) = (Obj::new(spline)?, Obj::new(tangent)?);
            let mut x = spline.float("x", 0.0)?;
            if let Some(last) = keys.last() {
                x = x.max(last[0] + 0.0001);
            }
            keys.push([
                x,
                spline.float("y", 0.0)?,
                spline.float("m_flSlopeIncoming", 0.0)?,
                spline.float("m_flSlopeOutgoing", 0.0)?,
            ]);
            kinds.push([
                tangent_kind(&tangent, "m_nIncomingTangent")?,
                tangent_kind(&tangent, "m_nOutgoingTangent")?,
            ]);
        }
        compute_slopes(&mut keys, &kinds);
        Ok(Self {
            keys,
            domain_min: curve.vec2("m_vDomainMins")?,
            domain_max: curve.vec2("m_vDomainMaxs")?,
            looped,
        })
    }
}

fn tangent_kind(tangent: &Obj<'_>, key: &str) -> Result<Tangent> {
    Ok(match tangent.str(key)? {
        Some("CURVE_TANGENT_LINEAR") => Tangent::Linear,
        Some("CURVE_TANGENT_FREE") => Tangent::Free,
        Some("CURVE_TANGENT_MIRROR") => Tangent::Mirror,
        Some("CURVE_TANGENT_SINE") => Tangent::Sine,
        Some("CURVE_TANGENT_FLAT") => Tangent::Flat,
        _ => Tangent::Spline,
    })
}

/// Each key's slopes from its tangent kinds, as Source 2's curve editor resolves them.
fn compute_slopes(keys: &mut [[f32; 4]], kinds: &[[Tangent; 2]]) {
    let points: Vec<[f32; 2]> = keys.iter().map(|k| [k[0], k[1]]).collect();
    let last = points.len().saturating_sub(1);
    for (i, key) in keys.iter_mut().enumerate() {
        let delta = |a: usize, b: usize| [points[b][0] - points[a][0], points[b][1] - points[a][1]];
        let previous = if i > 0 { delta(i - 1, i) } else { [0.0; 2] };
        let next = if i < last { delta(i, i + 1) } else { [0.0; 2] };
        let slope = |d: [f32; 2]| if d[0] == 0.0 { 0.0 } else { d[1] / d[0] };
        let central = match (i > 0, i < last) {
            (true, true) => slope([previous[0] + next[0], previous[1] + next[1]]),
            (true, false) => slope(previous),
            (false, _) => slope(next),
        };
        let inverse = |dx: f32| if dx == 0.0 { 1.0 } else { 1.0 / dx };
        let [incoming, outgoing] = kinds[i];
        key[2] = match incoming {
            Tangent::Linear => slope(previous),
            Tangent::Spline => central,
            Tangent::Free => key[2],
            Tangent::Sine => {
                inverse(previous[0])
                    * if previous[1] <= 0.0 {
                        -SINE_STEEP
                    } else {
                        -SINE_SHALLOW
                    }
            }
            Tangent::Mirror | Tangent::Flat => 0.0,
        };
        key[3] = match outgoing {
            Tangent::Linear => slope(next),
            Tangent::Spline => central,
            Tangent::Free => key[3],
            Tangent::Mirror => key[2],
            Tangent::Sine => {
                inverse(next[0])
                    * if next[1] <= 0.0 {
                        SINE_SHALLOW
                    } else {
                        SINE_STEEP
                    }
            }
            Tangent::Flat => 0.0,
        };
        if incoming == Tangent::Mirror {
            key[2] = key[3];
        }
    }
}

impl VectorInput {
    /// The input member `key` of `obj`, `default` when absent.
    pub(super) fn read(obj: &Obj<'_>, key: &str, default: [f32; 3]) -> Result<Self> {
        match obj.get(key) {
            None => Ok(Self::Literal(default)),
            Some(Value::String(name)) => match name.as_str() {
                "pvec_color_white" => Ok(Self::Literal([1.0; 3])),
                "pvec_color_black" => Ok(Self::Literal([0.0; 3])),
                _ => Ok(Self::Literal(obj.vec3(key, default)?)),
            },
            Some(Value::Object(_)) => Self::parse(
                &obj.object(key)?
                    .ok_or_else(|| malformed(WHAT, "lost input"))?,
            ),
            Some(_) => Ok(Self::Literal(obj.vec3(key, default)?)),
        }
    }

    fn parse(input: &Obj<'_>) -> Result<Self> {
        let kind = input.str("m_nType")?.unwrap_or("PVEC_TYPE_LITERAL");
        Ok(match kind {
            "PVEC_TYPE_LITERAL" => Self::Literal(input.vec3("m_vLiteralValue", [0.0; 3])?),
            "PVEC_TYPE_LITERAL_COLOR" => Self::Literal(input.color("m_LiteralColor", [0.0; 3])?),
            "PVEC_TYPE_RANDOM_UNIFORM" => Self::Random {
                min: input.vec3("m_vRandomMin", [0.0; 3])?,
                max: input.vec3("m_vRandomMax", [0.0; 3])?,
            },
            "PVEC_TYPE_RANDOM_UNIFORM_OFFSET" => Self::RandomOffset {
                min: input.vec3("m_vRandomMin", [0.0; 3])?,
                max: input.vec3("m_vRandomMax", [0.0; 3])?,
                field: input.field("m_nVectorAttribute", Field::COLOR)?,
                scale: input.vec3("m_vVectorAttributeScale", [1.0; 3])?,
            },
            "PVEC_TYPE_PARTICLE_VECTOR" | "PVEC_TYPE_PARTICLE_INITIAL_VECTOR" => {
                Self::ParticleVector {
                    field: input.field("m_nVectorAttribute", Field::COLOR)?,
                    initial: kind == "PVEC_TYPE_PARTICLE_INITIAL_VECTOR",
                    scale: input.vec3("m_vVectorAttributeScale", [1.0; 3])?,
                }
            }
            "PVEC_TYPE_PARTICLE_VELOCITY" => Self::ParticleVelocity,
            "PVEC_TYPE_CP_DELTA" => Self::ControlPointDelta {
                control_point: input.control_point("m_nControlPoint", 0)?,
                delta: input.control_point("m_nDeltaControlPoint", 0)?,
            },
            "PVEC_TYPE_CP_VALUE" => Self::ControlPointValue {
                control_point: input.control_point("m_nControlPoint", 0)?,
                scale: input.vec3("m_vCPValueScale", [1.0; 3])?,
            },
            "PVEC_TYPE_FLOAT_INTERP_CLAMPED"
            | "PVEC_TYPE_FLOAT_INTERP_OPEN"
            | "PVEC_TYPE_FLOAT_INTERP_GRADIENT" => Self::interpolated(input, kind)?,
            "PVEC_TYPE_FLOAT_COMPONENTS" => Self::Components(Box::new([
                FloatInput::read(input, "m_FloatComponentX", 0.0)?,
                FloatInput::read(input, "m_FloatComponentY", 0.0)?,
                FloatInput::read(input, "m_FloatComponentZ", 0.0)?,
            ])),
            other => Self::Unsupported {
                kind: other.to_owned(),
                literal: input.vec3("m_vLiteralValue", [0.0; 3])?,
            },
        })
    }

    /// A float input's interpolation, between two vectors or through a gradient.
    fn interpolated(input: &Obj<'_>, kind: &str) -> Result<Self> {
        let float = Box::new(FloatInput::read(input, "m_FloatInterp", 0.0)?);
        let range = [
            input.float("m_flInterpInput0", 0.0)?,
            input.float("m_flInterpInput1", 1.0)?,
        ];
        if kind == "PVEC_TYPE_FLOAT_INTERP_GRADIENT" {
            return Ok(Self::Gradient {
                input: float,
                range,
                stops: gradient(input)?,
            });
        }
        Ok(Self::FloatInterp {
            input: float,
            range,
            output: [
                input.vec3("m_vInterpOutput0", [0.0; 3])?,
                input.vec3("m_vInterpOutput1", [1.0; 3])?,
            ],
            clamped: kind == "PVEC_TYPE_FLOAT_INTERP_CLAMPED",
        })
    }

    /// The same input with its output's Y negated.
    pub(super) fn negated_y(self) -> Self {
        let flip = |v: [f32; 3]| [v[0], -v[1], v[2]];
        match self {
            Self::Literal(v) => Self::Literal(flip(v)),
            Self::Random { min, max } => Self::Random {
                min: flip(min),
                max: flip(max),
            },
            Self::RandomOffset {
                min,
                max,
                field,
                scale,
            } => Self::RandomOffset {
                min: flip(min),
                max: flip(max),
                field,
                scale: flip(scale),
            },
            Self::ParticleVector {
                field,
                initial,
                scale,
            } => Self::ParticleVector {
                field,
                initial,
                scale: flip(scale),
            },
            Self::ControlPointValue {
                control_point,
                scale,
            } => Self::ControlPointValue {
                control_point,
                scale: flip(scale),
            },
            Self::FloatInterp {
                input,
                range,
                output,
                clamped,
            } if clamped => Self::FloatInterp {
                input,
                range,
                output: [flip(output[0]), flip(output[1])],
                clamped,
            },
            Self::Components(components) => {
                let [x, y, z] = *components;
                Self::Components(Box::new([x, y.negated(), z]))
            }
            other => other,
        }
    }
}

/// A gradient's stops, sorted by position.
fn gradient(input: &Obj<'_>) -> Result<Vec<[f32; 4]>> {
    let Some(gradient) = input.object("m_Gradient")? else {
        return Ok(Vec::new());
    };
    let mut stops = Vec::new();
    for stop in gradient.array("m_Stops", MAX_KEYS)? {
        let stop = Obj::new(stop)?;
        let [r, g, b] = stop.color("m_Color", [0.0; 3])?;
        stops.push([stop.float("m_flPosition", 0.0)?.clamp(0.0, 1.0), r, g, b]);
    }
    stops.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(stops)
}
