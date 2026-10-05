//! Node kinds: the static behavior behind graph nodes. A graph node
//! is data plus a reference to its kind; everything a kind does lives here.

use std::collections::BTreeMap;

use crate::param::{ParamSpec, Params};
use crate::resource::Resources;
use crate::value::{PortType, Value, View};

pub struct NodeKind {
    /// Stable identifier used in project files, e.g. `raw.read`.
    pub name: &'static str,
    /// What new nodes are called, numbered when taken: `raw`, `raw 2`, …
    pub label: &'static str,
    pub params: &'static [ParamSpec],
    pub inputs: &'static [InputSpec],
    pub outputs: &'static [OutputSpec],
    /// Must be deterministic and free of side effects: results are cached by
    /// dependency stamp. Errors are user-facing messages.
    pub eval: fn(Params, &[Value], &EvalContext) -> Result<Evaluated, String>,
    /// Side effects, run only on explicit request.
    pub actions: &'static [Action],
}

/// Kinds are identified by name; their behavior is code and has no equality.
impl PartialEq for NodeKind {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl std::fmt::Debug for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl NodeKind {
    pub fn param(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    pub fn input(&self, name: &str) -> Option<&InputSpec> {
        self.inputs.iter().find(|p| p.name == name)
    }

    pub fn output_index(&self, name: &str) -> Option<usize> {
        self.outputs.iter().position(|p| p.name == name)
    }

    pub fn action(&self, name: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.name == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputSpec {
    pub name: &'static str,
    pub accepts: &'static [PortType],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputSpec {
    pub name: &'static str,
    pub ty: PortType,
}

/// A named side effect. Its inputs are evaluated at full resolution.
pub struct Action {
    pub name: &'static str,
    pub run: fn(Params, &[Value], &EvalContext) -> Result<(), String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Evaluated {
    /// One value per output port, in declaration order.
    pub outputs: Vec<Value>,
    pub view: Option<View>,
}

/// What a node's evaluation may depend on besides its params and inputs.
pub struct EvalContext<'a> {
    /// Downscaled by `2^level` along each axis; frontends keep it below 32.
    pub(crate) level: u8,
    pub(crate) resources: &'a Resources,
}

impl EvalContext<'_> {
    /// Sensor pixels per image pixel along each axis; 1 is full resolution.
    /// Demosaic adapters downsample by it after sensor-space processing.
    pub fn scale(&self) -> u32 {
        1 << self.level
    }

    pub fn resources(&self) -> &Resources {
        self.resources
    }
}

/// The node kinds a graph may use.
#[derive(Default)]
pub struct Registry {
    kinds: BTreeMap<&'static str, &'static NodeKind>,
}

impl Registry {
    pub fn with(mut self, kind: &'static NodeKind) -> Self {
        self.kinds.insert(kind.name, kind);
        self
    }

    pub fn get(&self, name: &str) -> Option<&'static NodeKind> {
        self.kinds.get(name).copied()
    }

    /// All kinds, ordered by name.
    pub fn kinds(&self) -> impl Iterator<Item = &'static NodeKind> + '_ {
        self.kinds.values().copied()
    }
}
