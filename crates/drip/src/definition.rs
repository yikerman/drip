//! Node declarations generated from functions. Contracts remain ordinary Rust.
use crate::{
    Error, Result,
    param::{ParamSpec, Parameters},
    ports::{Data, Description, PortSpec},
    runtime::KernelContext,
};
use std::sync::Arc;

/// Everything about a node kind that does not depend on its parameters. One
/// static per kind; graph checks, help and editors read the same declaration.
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub help: &'static str,
    pub references: &'static [(&'static str, &'static str)],
    pub parameters: &'static [ParamSpec],
    /// Persisted default parameters.
    pub defaults: fn() -> serde_json::Value,
    /// Admitting parameters loads assets (file I/O), so frontends should do it
    /// away from their UI thread.
    pub loads_assets: bool,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
}
impl Kind {
    pub fn param(&self, name: &str) -> Option<&'static ParamSpec> {
        self.parameters.iter().find(|p| p.name == name)
    }
}

/// Mechanical adapter interface. Use #[node] for computational nodes.
/// Implementations are trusted producers, just like a Payload implementation.
pub trait Node: Send + Sync + 'static {
    fn kind(&self) -> &'static Kind;
    fn parameters(&self) -> Result<serde_json::Value>;
    /// Pure output description/check using the same global configuration as run.
    /// Inputs are known descriptions; `None` is an unconnected optional input.
    /// Graph edits use full detail; evaluation repeats this with actual inputs.
    fn contract(
        &self,
        global: &crate::runtime::GlobalContext,
        inputs: &[Option<Description>],
    ) -> Result<Outcome>;
    /// Produce every output described by the contract.
    fn run(
        &self,
        context: &KernelContext<'_>,
        inputs: &[Option<(Description, Data)>],
        outputs: &[Description],
    ) -> Result<Vec<Data>>;
}

/// A declared node type: its kind, typed parameters and typed input views.
/// Frontends bind presentation and actions through this, not through strings.
pub trait Typed: Node + Sized {
    const KIND: &'static Kind;
    type Parameters: Parameters + serde::de::DeserializeOwned;
    type Inputs<'a>;
    /// View erased input values in this node's input order and types.
    fn bind(inputs: &[Option<(Description, Data)>]) -> Result<Self::Inputs<'_>>;
}

/// A node's output descriptions, or why they are not known yet. Only unbound
/// sources are unknown; their consumers inherit the reason, and evaluation
/// reports it. A contradiction is an error instead.
#[derive(Clone)]
pub enum Outcome {
    Known(Vec<Description>),
    Unknown(Error),
}

pub struct Registration {
    pub kind: &'static Kind,
    pub build: fn(serde_json::Value) -> Result<Arc<dyn Node>>,
}
#[linkme::distributed_slice]
pub static NODES: [Registration];

pub fn registered(id: &str, parameters: serde_json::Value) -> Result<Arc<dyn Node>> {
    let mut matches = NODES.iter().filter(|n| n.kind.id == id);
    let entry = matches.next().ok_or_else(|| Error::Graph(format!("unknown node {id}")))?;
    if matches.next().is_some() {
        return Err(Error::Graph(format!("duplicate node id {id}")));
    }
    (entry.build)(parameters)
}
