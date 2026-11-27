//! Compiled sound events and their operator stack definitions.

use std::collections::BTreeMap;

use super::events::public_operator_variables;
use crate::error::{Result, malformed};
use crate::kv3::{Object, Value};
use crate::{Budget, Resource};

const MAX_INHERITANCE: usize = 64;

/// A collection of event and operator stack definitions, indexed by exact name.
///
/// Pass the same budget across reads to bound allocations for the whole collection.
#[derive(Debug, Default)]
pub struct SoundEventGraph {
    events: Definitions,
    stacks: Definitions,
}

impl SoundEventGraph {
    /// Reads and adds a compiled `vsndevts_c` resource to this graph.
    pub fn read_events(&mut self, resource: &Resource, budget: &mut Budget) -> Result<()> {
        self.add_events(read_data(resource, "vsndevts", budget)?, budget)
    }

    /// Reads and adds a compiled `vsndstck_c` resource to this graph.
    pub fn read_stacks(&mut self, resource: &Resource, budget: &mut Budget) -> Result<()> {
        self.add_stacks(read_data(resource, "vsndstck", budget)?, budget)
    }

    /// Adds a decoded event dictionary. Later definitions of the same name win.
    pub fn add_events(&mut self, data: Value, budget: &mut Budget) -> Result<()> {
        self.events.add(data, "vsndevts", budget)
    }

    /// Adds a decoded stack dictionary. Later definitions of the same name win.
    pub fn add_stacks(&mut self, data: Value, budget: &mut Budget) -> Result<()> {
        self.stacks.add(data, "vsndstck", budget)
    }

    /// Event names in lexical order.
    pub fn event_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.events.index.keys().map(String::as_str)
    }

    /// Stack names in lexical order.
    pub fn stack_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.stacks.index.keys().map(String::as_str)
    }

    /// The event's own fields, before inheritance or defaults.
    pub fn event_definition(&self, name: &str) -> Option<&Value> {
        self.events.get(name).map(|(_, definition)| definition)
    }

    /// A stack's full compiled operator tree, including private operators and connections.
    pub fn stack_definition(&self, name: &str) -> Option<&Value> {
        self.stacks.get(name).map(|(_, definition)| definition)
    }

    /// Resolves the named event with nearest ancestor precedence, then stack defaults.
    ///
    /// An unknown event returns `Ok(None)`. Missing, malformed, cyclic and overly deep
    /// base chains return an error.
    pub fn resolve(&self, name: &str) -> Result<Option<ResolvedSoundEvent<'_>>> {
        let Some((name, definition)) = self.events.get(name) else {
            return Ok(None);
        };
        let base_chain = self.resolve_bases(name, definition)?;
        Ok(Some(ResolvedSoundEvent {
            graph: self,
            name,
            definition,
            base_chain,
        }))
    }

    fn resolve_bases<'a>(&'a self, name: &str, mut definition: &'a Value) -> Result<BaseChain<'a>> {
        let mut chain = BaseChain::default();
        loop {
            let base = match definition.get("base") {
                None | Some(Value::Null) => return Ok(chain),
                Some(Value::String(base)) if base.is_empty() => return Ok(chain),
                Some(Value::String(base)) => base,
                _ => {
                    return Err(malformed(
                        "vsndevts",
                        format!("event {name}: base is not a name"),
                    ));
                }
            };
            let (base, parent) = self.events.get(base).ok_or_else(|| {
                malformed("vsndevts", format!("event {name}: missing base {base}"))
            })?;
            if base == name || chain.iter().any(|(seen, _)| *seen == base) {
                return Err(malformed(
                    "vsndevts",
                    format!("event {name}: base cycle at {base}"),
                ));
            }
            let Some(slot) = chain.entries.get_mut(chain.len) else {
                return Err(malformed(
                    "vsndevts",
                    format!("event {name}: more than {MAX_INHERITANCE} bases"),
                ));
            };
            *slot = (base, parent);
            chain.len += 1;
            definition = parent;
        }
    }

    fn stack_default<'a>(&'a self, mut stack: &'a str, key: &str) -> Option<&'a Value> {
        for _ in 0..MAX_INHERITANCE {
            let variables = public_operator_variables(self.stack_definition(stack)?)?;
            if let Some(value) = variables
                .get(key)
                .and_then(|variable| variable.get("value"))
            {
                return Some(value);
            }
            stack = variables.str_field("import_public_opvars")?;
        }
        None
    }
}

fn read_data(resource: &Resource, format: &'static str, budget: &mut Budget) -> Result<Value> {
    let document = resource
        .kv3_in(b"DATA", budget)
        .ok_or_else(|| malformed(format, "no KV3 DATA block"))??;
    Ok(document.root)
}

type Definition<'a> = (&'a str, &'a Value);
type Location = (usize, usize);

#[derive(Debug, Default)]
struct Definitions {
    documents: Vec<Object>,
    index: BTreeMap<String, Location>,
}

impl Definitions {
    fn add(&mut self, data: Value, format: &'static str, budget: &mut Budget) -> Result<()> {
        let Value::Object(root) = data else {
            return Err(malformed(format, "not an object"));
        };
        let names = || {
            root.iter()
                .enumerate()
                .filter(|(_, (_, value))| value.as_object().is_some())
        };
        if names().next().is_none() {
            return Ok(());
        }
        budget.slots::<Object>(2)?;
        for (_, (name, _)) in names() {
            // Allow for unused slots and links in partially filled index nodes.
            budget.slots::<(String, Location)>(3)?;
            budget.string_of(name.len())?;
        }
        let document = self.documents.len();
        for (member, (name, _)) in names() {
            self.index.insert(name.to_owned(), (document, member));
        }
        self.documents.push(root);
        Ok(())
    }

    fn get(&self, name: &str) -> Option<Definition<'_>> {
        let &(document, member) = self.index.get(name)?;
        self.documents.get(document)?.member(member)
    }
}

#[derive(Debug)]
struct BaseChain<'a> {
    entries: [Definition<'a>; MAX_INHERITANCE],
    len: usize,
}

impl Default for BaseChain<'_> {
    fn default() -> Self {
        Self {
            entries: [("", &Value::Null); MAX_INHERITANCE],
            len: 0,
        }
    }
}

impl<'a> BaseChain<'a> {
    fn iter(&self) -> impl Iterator<Item = &Definition<'a>> {
        self.entries[..self.len].iter()
    }
}

/// An event with a validated base chain and access to its operator stack.
#[derive(Debug)]
pub struct ResolvedSoundEvent<'a> {
    graph: &'a SoundEventGraph,
    name: &'a str,
    definition: &'a Value,
    base_chain: BaseChain<'a>,
}

impl<'a> ResolvedSoundEvent<'a> {
    /// The event's exact name.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The immediate parent through the oldest ancestor.
    pub fn base_chain(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.base_chain.iter().map(|(name, _)| *name)
    }

    /// The effective operator stack type.
    pub fn kind(&self) -> Option<&'a str> {
        self.authored("type").and_then(Value::as_str)
    }

    /// The full compiled operator tree for the effective type, if loaded.
    pub fn stack_definition(&self) -> Option<&'a Value> {
        self.graph.stack_definition(self.kind()?)
    }

    /// The nearest event definition's value of a field, including explicit zero, null
    /// and empty arrays.
    pub fn authored(&self, key: &str) -> Option<&'a Value> {
        self.definition.get(key).or_else(|| {
            self.base_chain
                .iter()
                .find_map(|(_, fields)| fields.get(key))
        })
    }

    /// A field after inheritance and public operator-variable defaults.
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.authored(key)
            .or_else(|| self.graph.stack_default(self.kind()?, key))
    }
}
