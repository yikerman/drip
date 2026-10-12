//! Node declarations generated from functions. Contracts remain ordinary Rust.
use crate::{
    Error, Result,
    eval::InputValues,
    param::{ParamSpec, Parameters},
    ports::{Data, Description, PortSpec},
    runtime::KernelContext,
};
use std::{any::Any, sync::Arc};

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
