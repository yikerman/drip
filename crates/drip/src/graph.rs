//! Editable topology and structural port contracts.
//!
//! The graph knows node declarations, parameters and named edges. An edge is
//! valid when payload families match, even if CPU/GPU placement differs. It does
//! not inspect samples, establish physical meaning, select transfers or retain
//! computed values; those belong to node-local refinements and evaluation.
//! Missing inputs are valid during editing. Mutations preserve acyclicity and
//! at most one source per input; failed connections leave the graph unchanged.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as Json;

use crate::node::NodeKind;
use crate::param::ParamMap;
use crate::ports::TypeMismatch;
use crate::value::TypeDescriptor;

/// Stable within a project; never reused while the graph is alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

/// A named port of a node, input or output depending on context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Port(pub NodeId, pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Unique and renamable; how users and the CLI refer to the node.
    pub name: String,
    pub kind: &'static NodeKind,
    /// A valid value for every parameter of the kind.
    pub params: ParamMap,
    /// Parameters exposed as inputs of the template: they take a value per
    /// image and are cleared when saving as a template.
    pub external: BTreeSet<&'static str>,
    /// Opaque frontend state such as the node's position.
    pub ui: Json,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GraphError {
    #[error("no node {0:?}")]
    UnknownNode(NodeId),
    #[error("node {0:?} has no {1} port `{2}`")]
    UnknownPort(NodeId, &'static str, String),
    #[error("input {input:?}: {mismatch}")]
    TypeMismatch { input: Port, mismatch: TypeMismatch },
    #[error("connection would create a cycle")]
    Cycle,
    #[error("node {0:?} has no parameter `{1}`")]
    UnknownParam(NodeId, String),
    #[error("invalid value {value} for parameter `{param}`")]
    InvalidParam { param: String, value: Json },
    #[error("name `{0}` is empty or already used")]
    InvalidName(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    nodes: BTreeMap<NodeId, Node>,
    /// Input port → the output port feeding it.
    edges: BTreeMap<Port, Port>,
    next_id: u64,
    // Removed identities must not become another node while a frontend still
    // holds a reference. Imported sparse IDs must not exhaust the low range.
    issued_ids: BTreeSet<NodeId>,
}

impl Graph {
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().map(|(id, node)| (*id, node))
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.nodes().find(|(_, node)| node.name == name).map(|(id, _)| id)
    }

    /// Edges as (output, input) pairs.
    pub fn edges(&self) -> impl Iterator<Item = (&Port, &Port)> {
        self.edges.iter().map(|(input, output)| (output, input))
    }

    pub fn source(&self, input: &Port) -> Option<&Port> {
        self.edges.get(input)
    }

    /// The template's inputs: every external parameter of every node.
    pub fn inputs(&self) -> impl Iterator<Item = (NodeId, &'static str)> + '_ {
        self.nodes().flat_map(|(id, node)| node.external.iter().map(move |param| (id, *param)))
    }

    pub fn add_node(&mut self, kind: &'static NodeKind) -> NodeId {
        let name = (1_u64..)
            .map(|n| if n == 1 { kind.name.to_string() } else { format!("{} {n}", kind.name) })
            .find(|name| self.find(name).is_none())
            .expect("unbounded");
        let node = Node {
            name,
            kind,
            params: kind
                .params
                .iter()
                .map(|p| (p.name.to_string(), p.kind.default_value()))
                .collect(),
            external: kind.params.iter().filter(|p| p.external).map(|p| p.name).collect(),
            ui: Json::Null,
        };
        let id = self.allocate_id();
        self.nodes.insert(id, node);
        id
    }

    /// Insert an already validated project record. The loader checks duplicate
    /// IDs and reserves u64::MAX; its presence near that boundary does not prevent
    /// subsequent editing from allocating unused IDs in the low range.
    pub(crate) fn insert(&mut self, id: NodeId, node: Node) {
        self.next_id = self.next_id.max(id.0.saturating_add(1));
        self.issued_ids.insert(id);
        self.nodes.insert(id, node);
    }

    fn allocate_id(&mut self) -> NodeId {
        // A graph with every non-reserved u64 identity cannot fit in memory.
        assert!((self.issued_ids.len() as u128) < u64::MAX as u128, "node ID space exhausted");
        loop {
            if self.next_id == u64::MAX {
                self.next_id = 0;
            }
            let id = NodeId(self.next_id);
            self.next_id += 1;
            if self.issued_ids.insert(id) {
                return id;
            }
        }
    }

    pub fn remove_node(&mut self, id: NodeId) -> Option<Node> {
        self.edges.retain(|input, output| input.0 != id && output.0 != id);
        self.nodes.remove(&id)
    }

    /// Connects `output` to `input`, replacing the input's previous source.
    pub fn connect(&mut self, output: Port, input: Port) -> Result<(), GraphError> {
        let source = self.node(output.0).ok_or(GraphError::UnknownNode(output.0))?.kind;
        if source.output_index(&output.1).is_none() {
            return Err(GraphError::UnknownPort(output.0, "output", output.1.clone()));
        }
        let sink = self.node(input.0).ok_or(GraphError::UnknownNode(input.0))?.kind;
        let spec = sink
            .input(&input.1)
            .ok_or_else(|| GraphError::UnknownPort(input.0, "input", input.1.clone()))?;
        if self.reaches(input.0, output.0) {
            return Err(GraphError::Cycle);
        }
        let ty = self.output_type(&output).expect("checked output port");
        spec.requirement
            .check(&ty)
            .map_err(|mismatch| GraphError::TypeMismatch { input: input.clone(), mismatch })?;
        self.edges.insert(input, output);
        Ok(())
    }

    /// Output types are declared even while inputs are unconnected. None means
    /// the node or output port does not exist, not that its type is unresolved.
    pub fn output_type(&self, port: &Port) -> Option<TypeDescriptor> {
        self.node(port.0)?.kind.outputs().find(|p| p.name == port.1).map(|p| p.ty)
    }

    pub fn disconnect(&mut self, input: &Port) -> Option<Port> {
        self.edges.remove(input)
    }

    pub fn set_param(&mut self, id: NodeId, name: &str, value: Json) -> Result<(), GraphError> {
        let node = self.node_mut(id)?;
        let spec =
            node.kind.param(name).ok_or_else(|| GraphError::UnknownParam(id, name.into()))?;
        if !spec.kind.accepts(&value) {
            return Err(GraphError::InvalidParam { param: name.into(), value });
        }
        node.params.insert(name.into(), value);
        Ok(())
    }

    /// Exposes parameter `name` as a template input, or stops doing so.
    pub fn set_external(
        &mut self,
        id: NodeId,
        name: &str,
        external: bool,
    ) -> Result<(), GraphError> {
        let node = self.node_mut(id)?;
        let spec =
            node.kind.param(name).ok_or_else(|| GraphError::UnknownParam(id, name.into()))?;
        if external {
            node.external.insert(spec.name);
        } else {
            node.external.remove(spec.name);
        }
        Ok(())
    }

    pub fn set_name(&mut self, id: NodeId, name: &str) -> Result<(), GraphError> {
        if name.is_empty() || self.find(name).is_some_and(|other| other != id) {
            return Err(GraphError::InvalidName(name.into()));
        }
        self.node_mut(id)?.name = name.into();
        Ok(())
    }

    pub fn set_ui(&mut self, id: NodeId, ui: Json) -> Result<(), GraphError> {
        self.node_mut(id)?.ui = ui;
        Ok(())
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, GraphError> {
        self.nodes.get_mut(&id).ok_or(GraphError::UnknownNode(id))
    }

    /// Whether `to` is reachable from `from` along edges (or is `from`).
    fn reaches(&self, from: NodeId, to: NodeId) -> bool {
        let mut stack = vec![from];
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if id == to {
                return true;
            }
            if seen.insert(id) {
                let consumers = self.edges.iter().filter(|(_, output)| output.0 == id);
                stack.extend(consumers.map(|(input, _)| input.0));
            }
        }
        false
    }

    /// `targets` and all their ancestors, each after its sources. The explicit
    /// DFS stack supports loaded deep graphs without consuming the Rust stack.
    pub(crate) fn upstream_order(&self, targets: &[NodeId]) -> Vec<NodeId> {
        let mut seen = BTreeSet::new();
        let mut order = Vec::new();
        let mut stack: Vec<_> = targets.iter().rev().map(|&id| (id, false)).collect();
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                order.push(id);
            } else if seen.insert(id) {
                stack.push((id, true));
                let sources: Vec<_> = self
                    .edges
                    .range(Port(id, String::new())..)
                    .take_while(|(input, _)| input.0 == id)
                    .map(|(_, source)| source.0)
                    .collect();
                stack.extend(sources.into_iter().rev().map(|source| (source, false)));
            }
        }
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_handles_deep_chains_and_shared_ancestors_iteratively() {
        // Topological traversal needs only edges. Bypass interactive insertion
        // here to isolate stack behavior from name allocation and cycle checks.
        let mut graph = Graph::default();
        let depth = 20_000;
        for id in 1..depth {
            graph.edges.insert(Port(NodeId(id), "in".into()), Port(NodeId(id - 1), "out".into()));
        }
        let order = graph.upstream_order(&[NodeId(depth - 1), NodeId(depth / 2)]);
        assert_eq!(order, (0..depth).map(NodeId).collect::<Vec<_>>());
    }
}
