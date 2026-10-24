//! A render mesh's morph set (its `MRPH` block, Source 2's `CMorphSetData`): the per-vertex
//! deltas of each of its flexes, packed as rectangles of a texture atlas (the mesh's
//! `_vmorf.vtex`), and the flex controllers and rules that weigh them.
//!
//! The atlas stores each delta as an RGBA8 texel scaled into its rectangle's range (`offset +
//! range * texel / 255`); a mesh's vertex `i` is texel `(i % width, i / width)` of the morph
//! set's own `width x height` grid, which each rectangle covers a part of from its source in the
//! atlas. The first bundle of a rectangle holds the position delta (and a speed), the second the
//! normal delta (and a wrinkle weight).

use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::model::{array, finite, rotate};
use crate::vertices::{SCALE, TO_GLTF};

const WHAT: &str = "morph set";
/// Most texels of a morph set's grid (its vertices), of flexes and of rectangles per flex; a hero
/// head has ~4000 vertices and ~50 flexes of one rectangle each.
const MAX_TEXELS: usize = 1 << 22;
const MAX_FLEXES: usize = 256;
const MAX_RECTS: usize = 1 << 12;
/// Most controllers, rules and operations of a rule.
const MAX_CONTROLLERS: usize = 256;
const MAX_OPS: usize = 256;
/// A delta smaller than this (metres, or a normal's units) moves nothing.
const NOTHING: f32 = 1e-6;

/// What one bundle of a rectangle holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bundle {
    PositionSpeed,
    NormalWrinkle,
    Other,
}

/// Where one bundle's texels are in the atlas, and their scale.
#[derive(Clone, Debug, PartialEq)]
struct Source {
    u: f32,
    v: f32,
    offsets: [f32; 4],
    ranges: [f32; 4],
}

/// A rectangle of a flex's deltas: its place in the grid, its size in the atlas (a fraction of
/// it) and its bundles' sources.
#[derive(Clone, Debug, PartialEq)]
struct Rect {
    x: usize,
    y: usize,
    width: f32,
    height: f32,
    sources: Vec<Source>,
}

/// A flex controller: what animation, phonemes and scenes set, within its range.
#[derive(Clone, Debug, PartialEq)]
pub struct Controller {
    /// Its name (`jawOpen`), by which every model of a character shares it.
    pub name: String,
    /// Its range: a value is held within it before the rules read it.
    pub min: f32,
    /// See [`min`](Self::min).
    pub max: f32,
}

/// One step of a flex rule, Source's `FLEX_OP_*`: a stack machine that works out a flex's
/// weight from the controllers (`Fetch1`) and the flexes before it (`Fetch2`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    /// Pushes a number.
    Const(f32),
    /// Pushes a controller's value (an index of [`Flex::controllers`]).
    Fetch1(usize),
    /// Pushes a flex's weight worked out by an earlier rule (an index of [`Flex::descs`]).
    Fetch2(usize),
    /// Pops two values and pushes their sum.
    Add,
    /// Pops `b`, then `a`, and pushes `a - b`.
    Sub,
    /// Pops two values and pushes their product.
    Mul,
    /// Pops `b`, then `a`, and pushes `a / b`, or 0 when `b` is not above 0.0001.
    Div,
    /// Negates the top value.
    Neg,
    /// Pops two values and pushes the larger.
    Max,
    /// Pops two values and pushes the smaller.
    Min,
}

/// How each flex's weight is worked out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flex {
    /// The flexes by index (Source's `m_FlexDesc`, FACS names).
    pub descs: Vec<String>,
    /// The controllers the rules read.
    pub controllers: Vec<Controller>,
    /// Rules in order: the flex each sets and its operations.
    pub rules: Vec<(usize, Vec<Op>)>,
}

/// A morph target: one flex's deltas over a vertex buffer, in glTF's frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// Its flex, an index of [`Flex::descs`].
    pub desc: usize,
    /// Per vertex, its position's delta (metres).
    pub positions: Vec<[f32; 3]>,
    /// Per vertex, its normal's delta.
    pub normals: Vec<[f32; 3]>,
}

/// A mesh's morph set as its `MRPH` block describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct MorphSet {
    width: usize,
    height: usize,
    bundles: Vec<Bundle>,
    /// Per flex (index of `flex.descs`), its rectangles.
    rects: Vec<Vec<Rect>>,
    /// The atlas texture (`models/.../x_vmorf.vtex`).
    pub atlas: String,
    /// How the flexes' weights are worked out.
    pub flex: Flex,
}

fn float(v: Option<&Value>) -> Option<f32> {
    v.and_then(Value::as_f64)
        .map(|f| f as f32)
        .filter(|f| f.is_finite())
}

fn index(v: Option<&Value>) -> Option<usize> {
    v.and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
}

fn four(v: Option<&Value>) -> Result<[f32; 4]> {
    let items = array(v);
    let mut out = [0.0; 4];
    if items.len() != 4 {
        return Err(malformed(WHAT, "a bundle's range of other than four"));
    }
    for (o, i) in out.iter_mut().zip(items) {
        *o = float(Some(i)).ok_or_else(|| malformed(WHAT, "a bundle's range"))?;
    }
    Ok(out)
}

impl MorphSet {
    /// The morph set of `MRPH` block `data`, its atlas named by the mesh's `morph_texture`.
    ///
    /// # Errors
    ///
    /// A grid without its size or past 4 Mi texels, more than 256 flexes or controllers, a
    /// rectangle or controller without its numbers, and a rule that reads past the controllers
    /// or flexes. A rule with an operation not known here is left out.
    pub fn read(data: &Value, atlas: &str) -> Result<Self> {
        let (width, height) = (index(data.get("m_nWidth")), index(data.get("m_nHeight")));
        let (Some(width), Some(height)) = (width, height) else {
            return Err(malformed(WHAT, "no grid size"));
        };
        if width.saturating_mul(height) > MAX_TEXELS {
            return Err(malformed(WHAT, format!("a grid of {width} x {height}")));
        }
        let bundles = array(data.get("m_bundleTypes"))
            .iter()
            .map(|b| match b.as_str() {
                Some("MORPH_BUNDLE_TYPE_POSITION_SPEED") => Bundle::PositionSpeed,
                Some("MORPH_BUNDLE_TYPE_NORMAL_WRINKLE") => Bundle::NormalWrinkle,
                _ => Bundle::Other,
            })
            .collect();
        let datas = array(data.get("m_morphDatas"));
        if datas.len() > MAX_FLEXES {
            return Err(malformed(WHAT, format!("{} flexes", datas.len())));
        }
        let rects = datas.iter().map(rects).collect::<Result<_>>()?;
        Ok(Self {
            width,
            height,
            bundles,
            rects,
            atlas: atlas.to_owned(),
            flex: Flex::read(data)?,
        })
    }

    /// The deltas of every flex that moves a vertex of a buffer of `vertices`, out of the atlas
    /// (`width x height` RGBA8 texels), in flex order.
    ///
    /// # Errors
    ///
    /// An atlas of the wrong length, more vertices than the grid has texels, and a rectangle
    /// past the grid or the atlas.
    pub fn targets(
        &self,
        atlas: &[u8],
        size: (usize, usize),
        vertices: usize,
    ) -> Result<Vec<Target>> {
        let (aw, ah) = size;
        if atlas.len() != aw * ah * 4 {
            return Err(malformed(WHAT, "an atlas of the wrong size"));
        }
        if vertices > self.width * self.height {
            return Err(malformed(WHAT, "more vertices than the grid has texels"));
        }
        let mut out = Vec::new();
        for (desc, rects) in self.rects.iter().enumerate() {
            let mut target = Target {
                desc,
                positions: vec![[0.0; 3]; vertices],
                normals: vec![[0.0; 3]; vertices],
            };
            for rect in rects {
                self.place(rect, atlas, size, &mut target)?;
            }
            let moves = |list: &[[f32; 3]]| list.iter().flatten().any(|d| d.abs() > NOTHING);
            if moves(&target.positions) || moves(&target.normals) {
                out.push(target);
            }
        }
        Ok(out)
    }

    /// Writes one rectangle's deltas into `target`.
    fn place(
        &self,
        rect: &Rect,
        atlas: &[u8],
        (aw, ah): (usize, usize),
        target: &mut Target,
    ) -> Result<()> {
        let texels = |f: f32, of: usize| (f * of as f32).round().max(0.0) as usize;
        let (rw, rh) = (texels(rect.width, aw), texels(rect.height, ah));
        if rect.x + rw > self.width || rect.y + rh > self.height {
            return Err(malformed(WHAT, "a rectangle past the grid"));
        }
        for (source, kind) in rect.sources.iter().zip(&self.bundles) {
            let list = match kind {
                Bundle::PositionSpeed => &mut target.positions,
                Bundle::NormalWrinkle => &mut target.normals,
                Bundle::Other => continue,
            };
            let (su, sv) = (texels(source.u, aw), texels(source.v, ah));
            if su + rw > aw || sv + rh > ah {
                return Err(malformed(WHAT, "a rectangle past the atlas"));
            }
            for row in 0..rh {
                for col in 0..rw {
                    let vertex = (rect.y + row) * self.width + rect.x + col;
                    let Some(slot) = list.get_mut(vertex) else {
                        continue;
                    };
                    let at = ((sv + row) * aw + su + col) * 4;
                    let delta: [f32; 3] = std::array::from_fn(|c| {
                        source.offsets[c] + source.ranges[c] * f32::from(atlas[at + c]) / 255.0
                    });
                    *slot = match kind {
                        Bundle::PositionSpeed => rotate(TO_GLTF, delta.map(|d| d * SCALE)),
                        _ => rotate(TO_GLTF, delta),
                    }
                    .map(finite);
                }
            }
        }
        Ok(())
    }
}

/// A flex's rectangles (`m_morphRectDatas`).
fn rects(data: &Value) -> Result<Vec<Rect>> {
    let list = array(data.get("m_morphRectDatas"));
    if list.len() > MAX_RECTS {
        return Err(malformed(
            WHAT,
            format!("{} rectangles of one flex", list.len()),
        ));
    }
    list.iter()
        .map(|r| {
            let at = (index(r.get("m_nXLeftDst")), index(r.get("m_nYTopDst")));
            let size = (
                float(r.get("m_flUWidthSrc")),
                float(r.get("m_flVHeightSrc")),
            );
            let ((Some(x), Some(y)), (Some(width), Some(height))) = (at, size) else {
                return Err(malformed(WHAT, "a rectangle without its place or size"));
            };
            let sources = array(r.get("m_bundleDatas"))
                .iter()
                .map(|b| {
                    let (Some(u), Some(v)) =
                        (float(b.get("m_flULeftSrc")), float(b.get("m_flVTopSrc")))
                    else {
                        return Err(malformed(WHAT, "a bundle without its place"));
                    };
                    Ok(Source {
                        u,
                        v,
                        offsets: four(b.get("m_offsets"))?,
                        ranges: four(b.get("m_ranges"))?,
                    })
                })
                .collect::<Result<_>>()?;
            let fraction = |f: f32| (0.0..=1.0).contains(&f);
            if !fraction(width) || !fraction(height) {
                return Err(malformed(WHAT, "a rectangle larger than the atlas"));
            }
            Ok(Rect {
                x,
                y,
                width,
                height,
                sources,
            })
        })
        .collect()
}

impl Flex {
    /// `m_FlexDesc`, `m_FlexControllers` and `m_FlexRules`. Rules with an operation this reader
    /// does not know are left out whole (their flex keeps its rest weight); a malformed one is
    /// an error.
    fn read(data: &Value) -> Result<Self> {
        let descs: Vec<String> = array(data.get("m_FlexDesc"))
            .iter()
            .map(|d| {
                d.get("m_szFacs")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect();
        let list = array(data.get("m_FlexControllers"));
        if descs.len() > MAX_FLEXES || list.len() > MAX_CONTROLLERS {
            return Err(malformed(WHAT, "too many flexes or controllers"));
        }
        let controllers = list
            .iter()
            .map(|c| {
                let name = c.get("m_szName").and_then(Value::as_str);
                match (name, float(c.get("min")), float(c.get("max"))) {
                    (Some(name), Some(min), Some(max)) if min <= max => Ok(Controller {
                        name: name.to_owned(),
                        min,
                        max,
                    }),
                    _ => Err(malformed(WHAT, "a controller without its name or range")),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let rules_list = array(data.get("m_FlexRules"));
        if rules_list.len() > MAX_FLEXES {
            return Err(malformed(WHAT, "too many flex rules"));
        }
        let mut rules = Vec::new();
        for rule in rules_list {
            let flex = index(rule.get("m_nFlex")).filter(|&f| f < descs.len());
            let ops = array(rule.get("m_FlexOps"));
            let (Some(flex), true) = (flex, ops.len() <= MAX_OPS) else {
                return Err(malformed(WHAT, "a rule of no flex, or too long"));
            };
            let ops: Vec<Option<Op>> = ops
                .iter()
                .map(|op| self::op(op, descs.len(), controllers.len()))
                .collect::<Result<_>>()?;
            let known: Option<Vec<Op>> = ops.into_iter().collect();
            if let Some(ops) = known.filter(|ops| balanced(ops)) {
                rules.push((flex, ops));
            }
        }
        Ok(Self {
            descs,
            controllers,
            rules,
        })
    }
}

/// One operation: none for an operation not known here; an error for one that reads past the
/// controllers or flexes.
fn op(op: &Value, descs: usize, controllers: usize) -> Result<Option<Op>> {
    let data = op.get("m_Data").and_then(Value::as_i64).unwrap_or(0);
    let slot = |of: usize| {
        usize::try_from(data)
            .ok()
            .filter(|&i| i < of)
            .ok_or_else(|| malformed(WHAT, format!("an operation reading {data} of {of}")))
    };
    Ok(Some(match op.get("m_OpCode").and_then(Value::as_str) {
        Some("FLEX_OP_CONST") => Op::Const(finite(f32::from_bits(data as i32 as u32))),
        Some("FLEX_OP_FETCH1") => Op::Fetch1(slot(controllers)?),
        Some("FLEX_OP_FETCH2") => Op::Fetch2(slot(descs)?),
        Some("FLEX_OP_ADD") => Op::Add,
        Some("FLEX_OP_SUB") => Op::Sub,
        Some("FLEX_OP_MUL") => Op::Mul,
        Some("FLEX_OP_DIV") => Op::Div,
        Some("FLEX_OP_NEG") => Op::Neg,
        Some("FLEX_OP_MAX") => Op::Max,
        Some("FLEX_OP_MIN") => Op::Min,
        _ => return Ok(None),
    }))
}

/// Whether a rule leaves exactly one value on the stack and never pops an empty one.
fn balanced(ops: &[Op]) -> bool {
    let mut depth: usize = 0;
    for op in ops {
        depth = match op {
            Op::Const(_) | Op::Fetch1(_) | Op::Fetch2(_) => depth + 1,
            Op::Neg if depth >= 1 => depth,
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Max | Op::Min if depth >= 2 => depth - 1,
            _ => return false,
        };
    }
    depth == 1
}
