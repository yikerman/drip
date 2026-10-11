//! Node declarations generated from functions. Contracts remain ordinary Rust.
use crate::{
    Error, Result,
    ports::{Data, Description, PortSpec},
    runtime::KernelContext,
};
use std::sync::Arc;

pub struct Metadata {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub help: &'static str,
    pub references: &'static [(&'static str, &'static str)],
    pub parameters: &'static [crate::param::ParamSpec],
    /// Admitting parameters loads assets (file I/O), so frontends should do it
    /// away from their UI thread.
    pub loads_assets: bool,
}

/// Mechanical adapter interface. Use #[node] for computational nodes.
/// Implementations are trusted producers, just like a Payload implementation.
pub trait Node: Send + Sync + 'static {
    fn metadata(&self) -> Metadata;
    fn parameters(&self) -> Result<serde_json::Value>;
    fn inputs(&self) -> Vec<PortSpec>;
    fn outputs(&self) -> Vec<PortSpec>;
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

/// A node's output descriptions, or why they are not known yet. Only unbound
/// sources are unknown; their consumers inherit the reason, and evaluation
/// reports it. A contradiction is an error instead.
#[derive(Clone)]
pub enum Outcome {
    Known(Vec<Description>),
    Unknown(Error),
}

pub struct Registration {
    pub id: &'static str,
    pub defaults: fn() -> serde_json::Value,
    pub build: fn(serde_json::Value) -> Result<Arc<dyn Node>>,
}
#[linkme::distributed_slice]
pub static NODES: [Registration];

pub fn registered(id: &str, parameters: serde_json::Value) -> Result<Arc<dyn Node>> {
    let mut matches = NODES.iter().filter(|n| n.id == id);
    let entry = matches.next().ok_or_else(|| crate::Error::Graph(format!("unknown node {id}")))?;
    if matches.next().is_some() {
        return Err(crate::Error::Graph(format!("duplicate node id {id}")));
    }
    (entry.build)(parameters)
}
