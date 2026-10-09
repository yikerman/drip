//! Node declarations generated from functions. Contracts remain ordinary Rust.
use crate::{
    Result,
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
}

/// Mechanical adapter interface. Use #[node] for computational nodes.
/// Implementations are trusted producers, just like a Payload implementation.
pub trait Node: Send + Sync + 'static {
    fn metadata(&self) -> Metadata;
    /// Rebuilding this source may perform file I/O before producing an immutable
    /// node. Frontends should schedule admission away from their UI thread.
    fn loads_assets(&self) -> bool {
        false
    }
    fn parameters(&self) -> Result<serde_json::Value> {
        Err(crate::Error::Graph("this bound value has no persistent source definition".into()))
    }
    fn inputs(&self) -> Vec<PortSpec>;
    fn outputs(&self) -> Vec<PortSpec>;
    /// Pure output description/check using the same global configuration as run.
    /// Graph edits use full detail; evaluation repeats this with actual inputs.
    fn contract(
        &self,
        global: &crate::runtime::GlobalContext,
        inputs: &[Option<Description>],
    ) -> Result<Vec<Option<Description>>>;
    fn run(
        &self,
        context: &KernelContext<'_>,
        input_descs: &[Option<Description>],
        inputs: &[Option<Data>],
        output_descs: &[Option<Description>],
    ) -> Result<Vec<Option<Data>>>;
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
