//! Node kinds: the static behavior behind graph nodes (DESIGN N1). A graph node
//! is plain data naming its kind; everything a kind does lives here.

use std::collections::BTreeMap;

use crate::param::{ParamMap, ParamSpec, Params};
use crate::value::{PortType, Value, View};

pub struct NodeKind {
    /// Stable identifier used in project files, e.g. `raw.read`.
    pub name: &'static str,
    /// Bumped whenever params or ports change incompatibly; see `migrate`.
    pub version: u32,
    pub params: &'static [ParamSpec],
    pub inputs: &'static [InputSpec],
    pub outputs: &'static [OutputSpec],
    /// Must be deterministic and free of side effects: results are cached by
    /// dependency stamp (DESIGN E2). Errors are user-facing messages.
    pub eval: fn(Params, &[Value], &EvalContext) -> Result<Evaluated, String>,
    /// Side effects, run only on explicit request (DESIGN U2).
    pub actions: &'static [Action],
    /// Upgrades a node saved by an older `version` to the current one.
    pub migrate: Option<fn(from: u32, &mut Migration)>,
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
    pub run: fn(Params, &[Value]) -> Result<(), String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Evaluated {
    /// One value per output port, in declaration order.
    pub outputs: Vec<Value>,
    pub view: Option<View>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EvalContext {
    level: u8,
}

impl EvalContext {
    pub const FULL: EvalContext = EvalContext { level: 0 };

    /// Downscaled by `2^level` along each axis; `level` must be below 32.
    pub const fn downscaled(level: u8) -> Self {
        EvalContext { level }
    }

    /// Sensor pixels per image pixel along each axis; 1 is full resolution.
    /// Sources downsample by it, everything downstream inherits it.
    pub fn scale(&self) -> u32 {
        1 << self.level
    }
}

/// What `NodeKind::migrate` edits: the saved params, bindings and port names.
pub struct Migration<'a> {
    pub params: &'a mut ParamMap,
    pub(crate) bindings: &'a mut BTreeMap<String, String>,
    pub(crate) renamed_inputs: Vec<(String, String)>,
    pub(crate) renamed_outputs: Vec<(String, String)>,
}

impl Migration<'_> {
    /// Renames a parameter, whether it holds a literal or is bound to an input.
    pub fn rename_param(&mut self, old: &str, new: &str) {
        if let Some(value) = self.params.remove(old) {
            self.params.insert(new.into(), value);
        }
        if let Some(input) = self.bindings.remove(old) {
            self.bindings.insert(new.into(), input);
        }
    }

    pub fn rename_input(&mut self, old: &str, new: &str) {
        self.renamed_inputs.push((old.into(), new.into()));
    }

    pub fn rename_output(&mut self, old: &str, new: &str) {
        self.renamed_outputs.push((old.into(), new.into()));
    }
}

/// The node kinds a graph may use. Nodes of kinds missing here (or saved by a
/// newer version of a kind) are kept but cannot be evaluated (DESIGN F3).
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
}
