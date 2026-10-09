//! Discovery derives labels, controls and port declarations from the node itself.
use drip::{definition::NODES, param::ParamSpec, ports::PortSpec};
use serde_json::Value;
use std::{collections::BTreeMap, sync::LazyLock};

pub struct NodeKind {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub documentation: &'static str,
    pub references: &'static [(&'static str, &'static str)],
    pub params: &'static [ParamSpec],
    pub defaults: fn() -> Value,
    inputs: Vec<PortSpec>,
    outputs: Vec<PortSpec>,
}
impl NodeKind {
    pub fn inputs(&self) -> std::slice::Iter<'_, PortSpec> {
        self.inputs.iter()
    }
    pub fn outputs(&self) -> std::slice::Iter<'_, PortSpec> {
        self.outputs.iter()
    }
    pub fn param(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }
}
static KINDS: LazyLock<BTreeMap<&'static str, NodeKind>> = LazyLock::new(|| {
    let mut kinds = BTreeMap::new();
    for registration in NODES {
        let node = (registration.build)((registration.defaults)()).expect("valid node defaults");
        let meta = node.metadata();
        let kind = NodeKind {
            id: meta.id,
            name: meta.name,
            category: meta.category,
            documentation: meta.help,
            references: meta.references,
            params: meta.parameters,
            defaults: registration.defaults,
            inputs: node.inputs(),
            outputs: node.outputs(),
        };
        assert!(kinds.insert(meta.id, kind).is_none(), "duplicate node ID {}", meta.id);
    }
    kinds
});
pub fn get(id: &str) -> Option<&'static NodeKind> {
    KINDS.get(id)
}
#[derive(Default)]
pub struct Registry;
impl Registry {
    pub fn kinds(&self) -> impl Iterator<Item = &'static NodeKind> {
        KINDS.values()
    }
    pub fn get(&self, id: &str) -> Option<&'static NodeKind> {
        get(id)
    }
}
