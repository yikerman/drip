//! Node declarations generated from functions. Contracts remain ordinary Rust.
use crate::{
    Error, Result,
    eval::InputValues,
    image::{Description, Image},
    param::{ParamSpec, Parameters},
    ports::PortSpec,
    runtime::KernelContext,
};
use std::{any::Any, sync::Arc};

/// Everything about a node kind that does not depend on its parameters. One
/// static per kind; graph checks, help and editors read the same declaration.
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    /// What the node is for, from `registry::TAGS`; structure such as ports
    /// and payloads stays in the declaration.
    pub tags: &'static [&'static Tag],
    pub help: &'static str,
    pub references: &'static [(&'static str, &'static str)],
    pub parameters: &'static [ParamSpec],
    /// Persisted default parameters.
    pub defaults: fn() -> serde_json::Value,
    /// Admit persisted parameters using this kind's typed definition.
    pub build: fn(serde_json::Value) -> Result<Arc<dyn Node>>,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
    pub shape: Shape,
    /// An effect run on evaluated inputs, such as writing a file. Any frontend,
    /// headless or interactive, runs it the same way.
    pub action: Option<Action>,
}
/// How evaluation runs a node kind. The segment compiler adds shapes it can
/// fuse into one kernel (maps, warps) and derive regions and scale from;
/// `Kernel` and `Source` stay opaque to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The node's function runs on every evaluation. It asks its inputs for
    /// the residency it needs, and each output is materialized where the
    /// node wrote it.
    Kernel,
    /// The node loads its outputs once, when first evaluated, and keeps them
    /// with the admitted node.
    Source,
}
/// A registered label for finding nodes by purpose, listed in `registry::TAGS`.
pub struct Tag {
    pub id: &'static str,
    pub description: &'static str,
}
#[derive(Clone, Copy)]
pub struct Action {
    pub name: &'static str,
    /// Runs on the admitted node, which holds its parameters and assets.
    pub run: fn(&dyn Node, &InputValues) -> Result<()>,
}
/// Files a node's parameters name (`#[node(assets)]`), read when an action or
/// preparation first needs them and kept with the node, error included. Files
/// are assumed not to change; the node stays editable and persistable.
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
    /// Inputs are the evaluated descriptions; `None` is an unconnected optional input.
    fn contract(
        &self,
        global: &crate::runtime::GlobalContext,
        inputs: &[Option<Description>],
    ) -> Result<Vec<Description>>;
    /// Produce every output described by the contract, in output order.
    fn run(
        &self,
        context: &KernelContext<'_>,
        inputs: &[Option<Image>],
        outputs: &[Description],
    ) -> Result<Vec<Image>>;
}

/// A declared node type: its kind, typed parameters and typed input views.
/// Frontends bind presentation and actions through this, not through strings.
pub trait Typed: Node + Sized {
    const KIND: &'static Kind;
    type Parameters: Parameters + serde::Serialize + serde::de::DeserializeOwned;
    type Inputs<'a>;
    /// View erased input values in this node's input order and types.
    fn bind(inputs: &[Option<Image>]) -> Result<Self::Inputs<'_>>;
    /// View an admitted node as this type.
    fn of(node: &dyn Node) -> Result<&Self> {
        (node as &dyn Any)
            .downcast_ref()
            .ok_or_else(|| Error::Graph(format!("node is not {}", Self::KIND.id)))
    }
}
