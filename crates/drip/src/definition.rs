//! Node declarations generated from functions. Contracts remain ordinary Rust.
use crate::{
    Error, Result,
    eval::InputValues,
    param::{ParamSpec, Parameters},
    ports::{Data, Description, PortSpec},
    runtime::KernelContext,
};
use std::{
    any::Any,
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

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
    /// Admitting parameters loads files (a source's outputs or `#[node(assets)]`),
    /// so frontends should do it away from their UI thread.
    pub loads_assets: bool,
    /// Admit persisted parameters using this kind's typed definition.
    pub build: fn(serde_json::Value) -> Result<Arc<dyn Node>>,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
    /// An effect run on evaluated inputs, such as writing a file. Any frontend,
    /// headless or interactive, runs it the same way.
    pub action: Option<Action>,
}
#[derive(Clone, Copy)]
pub struct Action {
    pub name: &'static str,
    /// Runs on the admitted node, which holds its parameters and assets.
    pub run: fn(&dyn Node, &InputValues) -> Result<()>,
}
/// Files a node's parameters name, read once when the node is admitted
/// (`#[node(assets)]`), so evaluation and actions perform no file I/O. A load
/// error is kept with the node, which stays editable and persistable.
pub trait Assets {
    type Loaded: Send + Sync + 'static;
    fn load(&self) -> Result<Self::Loaded>;
}
impl Kind {
    pub fn param(&self, name: &str) -> Option<&'static ParamSpec> {
        self.parameters.iter().find(|p| p.name == name)
    }
}

/// Mechanical adapter interface. Use #[node] for computational nodes.
/// Implementations are trusted producers, just like a Payload implementation.
pub trait Node: Any + Send + Sync {
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
    type Parameters: Parameters + serde::Serialize + serde::de::DeserializeOwned;
    type Inputs<'a>;
    /// View erased input values in this node's input order and types.
    fn bind(inputs: &[Option<(Description, Data)>]) -> Result<Self::Inputs<'_>>;
    /// View an admitted node as this type.
    fn of(node: &dyn Node) -> Result<&Self> {
        (node as &dyn Any)
            .downcast_ref()
            .ok_or_else(|| Error::Graph(format!("node is not {}", Self::KIND.id)))
    }
}

/// A node's output descriptions, or why they are not known yet. Only unbound
/// sources are unknown; their consumers inherit the reason, and evaluation
/// reports it. A contradiction is an error instead.
#[derive(Clone)]
pub enum Outcome {
    Known(Vec<Description>),
    Unknown(Error),
}

#[linkme::distributed_slice]
pub static NODES: [&'static Kind];

static KINDS: LazyLock<BTreeMap<&'static str, &'static Kind>> = LazyLock::new(|| {
    let mut kinds = BTreeMap::new();
    for &kind in NODES {
        assert!(kinds.insert(kind.id, kind).is_none(), "duplicate node ID {}", kind.id);
    }
    kinds
});
pub fn kind(id: &str) -> Option<&'static Kind> {
    KINDS.get(id).copied()
}
pub fn kinds() -> impl Iterator<Item = &'static Kind> {
    KINDS.values().copied()
}
pub fn registered(id: &str, parameters: serde_json::Value) -> Result<Arc<dyn Node>> {
    let kind = kind(id).ok_or_else(|| Error::Graph(format!("unknown node {id}")))?;
    (kind.build)(parameters)
}
