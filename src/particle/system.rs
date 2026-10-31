//! A particle system definition (`vpcf_c`): its constants, functions, renderers and children.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::{Result, malformed};
use crate::kv3::{Document, Value};
use crate::resource::Resource;

use super::constraint::Constraint;
use super::emitter::Emitter;
use super::format::format_number;
use super::function::{Function, Timing};
use super::initializer::Initializer;
use super::models::MAX_MODELS;
use super::operator::{Force, Operator};
use super::pre_emission::PreEmission;
use super::read::{MAX_FUNCTIONS, MAX_LAYERS, MAX_NAME, Obj, WHAT};
use super::renderer::Renderer;

/// Most particles a system holds at once, whatever its definition asks for.
pub const MAX_PARTICLES: u32 = 5000;
/// Seconds undrawn before a system sleeps when its definition does not say (the schema's
/// `m_flNoDrawTimeToGoToSleep`).
pub const SLEEP_AFTER: f32 = 8.0;
/// The padding box when its definition does not say (the schema's `m_BoundingBoxMin`,
/// `m_BoundingBoxMax`).
pub const BOUNDS: [[f32; 3]; 2] = [[-10.0; 3], [10.0; 3]];

#[cfg(feature = "serde")]
fn default_sleep() -> f32 {
    SLEEP_AFTER
}

#[cfg(feature = "serde")]
fn default_bounds() -> [[f32; 3]; 2] {
    BOUNDS
}

/// Most children a system has; Dota's have up to ~20.
pub const MAX_CHILDREN: usize = 64;
/// The longest path of a child.
const MAX_PATH: usize = 256;

/// A particle system definition, every list in file order with its disabled functions left
/// out, and every value the file leaves out at the schema's default.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct System {
    /// The behaviour version (`m_nBehaviorVersion`): below 6 an initializer that writes only
    /// what earlier ones wrote is skipped, from `first_multiple_override` on it runs.
    pub behavior_version: i32,
    /// See `behavior_version` (-1 none).
    pub first_multiple_override: i32,
    /// Most particles at once.
    pub max_particles: u32,
    /// Particles made when it starts.
    pub initial_particles: u32,
    /// The longest simulation step, in seconds.
    pub max_time_step: f32,
    /// Seconds a system goes undrawn before it sleeps: no longer simulated until it is
    /// drawn again (`m_flNoDrawTimeToGoToSleep`).
    #[cfg_attr(feature = "serde", serde(default = "default_sleep"))]
    pub sleep_after: f32,
    /// The box its particles' bounds are padded by, about each of them, for whether it is in
    /// view (`m_BoundingBoxMin`, `m_BoundingBoxMax`).
    #[cfg_attr(feature = "serde", serde(default = "default_bounds"))]
    pub bounds: [[f32; 3]; 2],
    /// What a particle starts with before the initializers.
    pub constants: Constants,
    /// The children's group, which their parent's functions address.
    pub group: i32,
    /// Pre-emission operators.
    pub pre_emission: Vec<Function<PreEmission>>,
    /// Emitters.
    pub emitters: Vec<Function<Emitter>>,
    /// Initializers.
    pub initializers: Vec<Function<Initializer>>,
    /// Operators.
    pub operators: Vec<Function<Operator>>,
    /// Force generators.
    pub forces: Vec<Function<Force>>,
    /// Constraints, which `C_OP_BasicMovement` runs after it moves the particles.
    pub constraints: Vec<Function<Constraint>>,
    /// Renderers.
    pub renderers: Vec<Function<Renderer>>,
    /// Child systems.
    pub children: Vec<Child>,
}

/// What a particle starts with (`m_ConstantColor` and the like).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Constants {
    /// Colour and alpha, 0..1.
    pub color: [f32; 4],
    /// Radius.
    pub radius: f32,
    /// Lifetime in seconds.
    pub lifetime: f32,
    /// Roll in radians.
    pub rotation: f32,
    /// Roll speed in radians a second.
    pub rotation_speed: f32,
    /// Sprite sheet sequence.
    pub sequence: f32,
    /// Normal.
    pub normal: [f32; 3],
}

/// A child system.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Child {
    /// Its definition (`particles/.../x.vpcf`).
    pub path: String,
    /// It runs only from its parent's end cap on.
    pub end_cap: bool,
    /// Seconds after its parent starts that it starts.
    pub delay: f32,
}

impl System {
    /// The system of a `vpcf_c` resource: its `DATA` block, KV3.
    pub fn read(resource: &Resource<'_>) -> Result<Self> {
        let document = resource
            .kv3(b"DATA")
            .ok_or_else(|| malformed(WHAT, "no KV3 DATA block"))??;
        Self::from_document(&document)
    }

    /// The system of a decoded `DATA` block, upgraded from the format its GUID names.
    pub fn from_document(document: &Document) -> Result<Self> {
        let format = format_number(&document.format);
        let root = Obj::new(&document.root)?;
        if root
            .str("_class")?
            .is_some_and(|c| c != "CParticleSystemDefinition")
        {
            return Err(malformed(WHAT, "not a particle system definition"));
        }
        let behavior_version = root.int_in("m_nBehaviorVersion", 0, 0, 1 << 16)? as i32;
        let max_particles = root
            .int("m_nMaxParticles", 1000)?
            .clamp(1, i64::from(MAX_PARTICLES)) as u32;
        let system = Self {
            behavior_version,
            first_multiple_override: root.int_in(
                "m_nFirstMultipleOverride_BackwardCompat",
                -1,
                -1,
                1 << 16,
            )? as i32,
            max_particles,
            initial_particles: root
                .int("m_nInitialParticles", 0)?
                .clamp(0, i64::from(max_particles)) as u32,
            max_time_step: Some(root.float("m_flMaximumTimeStep", 0.1)?)
                .filter(|t| *t > 0.0)
                .unwrap_or(0.1),
            sleep_after: Some(root.float("m_flNoDrawTimeToGoToSleep", SLEEP_AFTER)?)
                .filter(|t| t.is_finite())
                .unwrap_or(SLEEP_AFTER)
                .max(0.0),
            bounds: [
                root.vec3("m_BoundingBoxMin", BOUNDS[0])?,
                root.vec3("m_BoundingBoxMax", BOUNDS[1])?,
            ],
            constants: Constants::read(&root)?,
            group: root.int_in("m_nGroupID", 0, -1, 1 << 16)? as i32,
            pre_emission: functions(&root, "m_PreEmissionOperators", PreEmission::read)?,
            emitters: functions(&root, "m_Emitters", |class, obj| {
                Emitter::read(class, obj, behavior_version)
            })?,
            initializers: functions(&root, "m_Initializers", |class, obj| {
                Initializer::read(class, obj, format, behavior_version)
            })?,
            operators: functions(&root, "m_Operators", Operator::read)?,
            forces: functions(&root, "m_ForceGenerators", |class, obj| {
                Force::read(class, obj, behavior_version)
            })?,
            constraints: functions(&root, "m_Constraints", Constraint::read)?,
            renderers: functions(&root, "m_Renderers", |class, obj| {
                Renderer::read(class, obj, format)
            })?,
            children: children(&root)?,
        };
        system.validate()?;
        Ok(system)
    }

    /// Checks the bounds the reader keeps, for a system read back from elsewhere (a cache).
    pub fn validate(&self) -> Result<()> {
        let lists = [
            ("pre-emission operators", self.pre_emission.len()),
            ("emitters", self.emitters.len()),
            ("initializers", self.initializers.len()),
            ("operators", self.operators.len()),
            ("force generators", self.forces.len()),
            ("constraints", self.constraints.len()),
            ("renderers", self.renderers.len()),
        ];
        if !(self.sleep_after >= 0.0 && self.sleep_after.is_finite())
            || !self.bounds.iter().flatten().all(|v| v.is_finite())
        {
            return Err(malformed(
                WHAT,
                "a sleep time or bounding box that is no number",
            ));
        }
        for (what, len) in lists {
            if len > MAX_FUNCTIONS {
                return Err(malformed(
                    WHAT,
                    format!("{len} {what}, more than {MAX_FUNCTIONS}"),
                ));
            }
        }
        for renderer in &self.renderers {
            let too_long = match &renderer.class {
                Renderer::Projected(p) => {
                    p.materials.len() > MAX_LAYERS || p.materials.iter().any(|m| m.len() > MAX_NAME)
                }
                Renderer::DeferredLight(l) => {
                    l.texture.as_ref().is_some_and(|t| t.len() > MAX_NAME)
                }
                Renderer::Models(m) => {
                    m.models.len() > MAX_MODELS
                        || m.models.iter().any(|(path, _)| path.len() > MAX_NAME)
                        || m.override_material
                            .as_ref()
                            .is_some_and(|o| o.len() > MAX_NAME)
                }
                _ => false,
            };
            if too_long {
                return Err(malformed(WHAT, "too many materials or too long a path"));
            }
        }
        self.validate_sets()?;
        if self.children.len() > MAX_CHILDREN
            || self.children.iter().any(|c| c.path.len() > MAX_PATH)
        {
            return Err(malformed(WHAT, "too many children or too long a path"));
        }
        if !(1..=MAX_PARTICLES).contains(&self.max_particles)
            || self.initial_particles > self.max_particles
        {
            return Err(malformed(WHAT, format!("{} particles", self.max_particles)));
        }
        if !(self.max_time_step > 0.0 && self.max_time_step.is_finite()) {
            return Err(malformed(WHAT, "a time step that is not positive"));
        }
        Ok(())
    }

    /// Checks the hitbox sets its functions on a model name.
    fn validate_sets(&self) -> Result<()> {
        let sets = self
            .initializers
            .iter()
            .filter_map(|init| match &init.class {
                Initializer::SetHitboxToModel(hitbox) => Some(&hitbox.set),
                Initializer::CreateOnModel(on_model) => Some(&on_model.set),
                _ => None,
            })
            .chain(self.operators.iter().filter_map(|op| match &op.class {
                Operator::LockToBone(lock) => Some(&lock.set),
                _ => None,
            }));
        for set in sets {
            if set.len() > MAX_NAME {
                return Err(malformed(WHAT, "a hitbox set's name is too long"));
            }
        }
        Ok(())
    }

    /// The classes of its functions and renderers this crate does not read, in file order.
    pub fn unsupported_classes(&self) -> Vec<&str> {
        let pre_emission = self.pre_emission.iter().filter_map(|f| match &f.class {
            PreEmission::Unsupported(class) => Some(class.as_str()),
            _ => None,
        });
        let emitters = self.emitters.iter().filter_map(|f| match &f.class {
            Emitter::Unsupported(class) => Some(class.as_str()),
            _ => None,
        });
        let initializers = self.initializers.iter().filter_map(|f| match &f.class {
            Initializer::Unsupported(class) => Some(class.as_str()),
            _ => None,
        });
        let operators = self.operators.iter().filter_map(|f| match &f.class {
            Operator::Unsupported(class) => Some(class.as_str()),
            _ => None,
        });
        let forces = self.forces.iter().filter_map(|f| match &f.class {
            Force::Unsupported(class) => Some(class.as_str()),
            _ => None,
        });
        let constraints = self.constraints.iter().filter_map(|f| match &f.class {
            Constraint::Unsupported(class) => Some(class.as_str()),
            _ => None,
        });
        let renderers = self.renderers.iter().filter_map(|f| match &f.class {
            Renderer::Unsupported(class) => Some(class.as_str()),
            _ => None,
        });
        pre_emission
            .chain(emitters)
            .chain(initializers)
            .chain(operators)
            .chain(forces)
            .chain(constraints)
            .chain(renderers)
            .collect()
    }

    /// The models its model renderers draw (`models/.../x.vmdl`).
    pub fn models(&self) -> impl Iterator<Item = &str> {
        self.renderers
            .iter()
            .filter_map(|r| match &r.class {
                Renderer::Models(m) => Some(m.paths()),
                _ => None,
            })
            .flatten()
    }

    /// The textures its renderers draw with (`materials/.../x.vtex`).
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        self.renderers
            .iter()
            .filter_map(|r| r.class.look())
            .flat_map(|look| {
                let more = look.layers.iter().map(|l| l.texture.as_str());
                look.texture.as_deref().into_iter().chain(more)
            })
    }

    /// The materials its projected renderers draw with (`materials/.../x.vmat`), whose
    /// textures and blends are theirs.
    pub fn materials(&self) -> impl Iterator<Item = &str> {
        self.renderers
            .iter()
            .filter_map(|r| match &r.class {
                Renderer::Projected(p) => Some(p.materials.iter().map(String::as_str)),
                _ => None,
            })
            .flatten()
            .filter(|m| !m.is_empty())
    }
}

impl Constants {
    fn read(root: &Obj<'_>) -> Result<Self> {
        Ok(Self {
            color: root.color4("m_ConstantColor")?.unwrap_or([1.0; 4]),
            radius: root.float("m_flConstantRadius", 5.0)?,
            lifetime: root.float("m_flConstantLifespan", 1.0)?,
            // The constants are radians already, unlike every other source of the angles.
            rotation: root.float("m_flConstantRotation", 0.0)?,
            rotation_speed: root.float("m_flConstantRotationSpeed", 0.0)?,
            sequence: root.float("m_nConstantSequenceNumber", 0.0)?,
            normal: root.vec3("m_ConstantNormal", [0.0, 0.0, 1.0])?,
        })
    }
}

/// The enabled functions of list `key`, each read by `read` from its class and members.
fn functions<T>(
    root: &Obj<'_>,
    key: &str,
    read: impl Fn(&str, &Obj<'_>) -> Result<T>,
) -> Result<Vec<Function<T>>> {
    let mut out = Vec::new();
    for item in root.array(key, MAX_FUNCTIONS)? {
        let obj = Obj::new(item)?;
        if obj.bool("m_bDisableOperator", false)? {
            continue;
        }
        let class = obj
            .str("_class")?
            .ok_or_else(|| malformed(WHAT, format!("a member of {key} has no class")))?;
        out.push(Function {
            class: read(class, &obj)?,
            timing: Timing::read(&obj)?,
        });
    }
    Ok(out)
}

fn children(root: &Obj<'_>) -> Result<Vec<Child>> {
    let mut out = Vec::new();
    for item in root.array("m_Children", MAX_CHILDREN)? {
        let child = Obj::new(item)?;
        let path = match child.get("m_ChildRef") {
            Some(Value::String(path)) => path,
            _ => continue,
        };
        if child.bool("m_bDisableChild", false)? || path.is_empty() {
            continue;
        }
        if path.len() > MAX_PATH {
            return Err(malformed(WHAT, "a child's path is too long"));
        }
        out.push(Child {
            path: path.clone(),
            end_cap: child.bool("m_bEndCap", false)?,
            delay: child.float("m_flDelay", 0.0)?.max(0.0),
        });
    }
    Ok(out)
}
