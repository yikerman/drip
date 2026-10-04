//! The node graph as plain data, with editing operations that keep it valid:
//! nodes exist and are of registered kinds, connections are type-compatible
//! and the graph stays acyclic.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::node::{NodeKind, Registry};
use crate::param::ParamMap;
use crate::value::PortType;

/// Stable within a project; never reused while the graph is alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u64);

/// A named port of a node, input or output depending on context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Port(pub NodeId, pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Unique within the graph; how users and the CLI refer to the node.
    pub label: String,
    pub kind: String,
    /// Literal parameter values.
    #[serde(default)]
    pub params: ParamMap,
    /// Parameters taken from graph inputs instead: param name → input name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,
    /// Opaque frontend state such as the node's position.
    #[serde(default, skip_serializing_if = "Json::is_null")]
    pub ui: Json,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GraphError {
    #[error("no node {0:?}")]
    UnknownNode(NodeId),
    #[error("unknown node kind `{0}`")]
    UnknownKind(String),
    #[error("node {0:?} has no {1} port `{2}`")]
    UnknownPort(NodeId, &'static str, String),
    #[error("input `{input}` does not accept {found:?}")]
    TypeMismatch { input: String, found: PortType },
    #[error("connection would create a cycle")]
    Cycle,
    #[error("node {0:?} has no parameter `{1}`")]
    UnknownParam(NodeId, String),
    #[error("invalid value {value} for parameter `{param}`")]
    InvalidParam { param: String, value: Json },
    #[error("graph has no input `{0}`")]
    UnknownInput(String),
    #[error("input `{input}` takes a different kind of value than parameter `{param}`")]
    IncompatibleInput { input: String, param: String },
    #[error("label `{0}` is empty or already used")]
    InvalidLabel(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub(crate) nodes: BTreeMap<NodeId, Node>,
    /// Input port → the output port feeding it.
    pub(crate) edges: BTreeMap<Port, Port>,
    pub(crate) next_id: u64,
}

impl Graph {
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().map(|(id, node)| (*id, node))
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn find(&self, label: &str) -> Option<NodeId> {
        self.nodes().find(|(_, node)| node.label == label).map(|(id, _)| id)
    }

    /// Edges as (output, input) pairs.
    pub fn edges(&self) -> impl Iterator<Item = (&Port, &Port)> {
        self.edges.iter().map(|(input, output)| (output, input))
    }

    pub fn source(&self, input: &Port) -> Option<&Port> {
        self.edges.get(input)
    }

    /// The graph's inputs: every name some parameter is bound to (DESIGN F5).
    pub fn inputs(&self) -> BTreeSet<&str> {
        self.nodes.values().flat_map(|node| node.bindings.values().map(String::as_str)).collect()
    }

    pub fn add_node(&mut self, kind: &NodeKind) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        let label = (1..)
            .map(|n| if n == 1 { kind.name.to_string() } else { format!("{} {n}", kind.name) })
            .find(|label| self.find(label).is_none())
            .expect("unbounded");
        let params =
            kind.params.iter().map(|p| (p.name.to_string(), p.kind.default_value())).collect();
        let node = Node {
            label,
            kind: kind.name.into(),
            params,
            bindings: BTreeMap::new(),
            ui: Json::Null,
        };
        self.nodes.insert(id, node);
        id
    }

    pub fn remove_node(&mut self, id: NodeId) -> Option<Node> {
        self.edges.retain(|input, output| input.0 != id && output.0 != id);
        self.nodes.remove(&id)
    }

    /// Connects `output` to `input`, replacing the input's previous source.
    pub fn connect(
        &mut self,
        registry: &Registry,
        output: Port,
        input: Port,
    ) -> Result<(), GraphError> {
        let ty = self
            .kind(registry, output.0)?
            .outputs
            .iter()
            .find(|p| p.name == output.1)
            .map(|p| p.ty);
        let ty = ty.ok_or_else(|| GraphError::UnknownPort(output.0, "output", output.1.clone()))?;
        let spec = self.kind(registry, input.0)?.input(&input.1);
        let spec =
            spec.ok_or_else(|| GraphError::UnknownPort(input.0, "input", input.1.clone()))?;
        if !spec.accepts.contains(&ty) {
            return Err(GraphError::TypeMismatch { input: input.1, found: ty });
        }
        if self.reaches(input.0, output.0) {
            return Err(GraphError::Cycle);
        }
        self.edges.insert(input, output);
        Ok(())
    }

    pub fn disconnect(&mut self, input: &Port) -> Option<Port> {
        self.edges.remove(input)
    }

    pub fn set_param(
        &mut self,
        registry: &Registry,
        id: NodeId,
        name: &str,
        value: Json,
    ) -> Result<(), GraphError> {
        let spec = self
            .kind(registry, id)?
            .param(name)
            .ok_or_else(|| GraphError::UnknownParam(id, name.into()))?;
        if !spec.kind.accepts(&value) {
            return Err(GraphError::InvalidParam { param: name.into(), value });
        }
        self.nodes.get_mut(&id).expect("checked by kind").params.insert(name.into(), value);
        Ok(())
    }

    pub fn set_label(&mut self, id: NodeId, label: &str) -> Result<(), GraphError> {
        if label.is_empty() || self.find(label).is_some_and(|other| other != id) {
            return Err(GraphError::InvalidLabel(label.into()));
        }
        self.node_mut(id)?.label = label.into();
        Ok(())
    }

    pub fn set_ui(&mut self, id: NodeId, ui: Json) -> Result<(), GraphError> {
        self.node_mut(id)?.ui = ui;
        Ok(())
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, GraphError> {
        self.nodes.get_mut(&id).ok_or(GraphError::UnknownNode(id))
    }

    pub(crate) fn kind(
        &self,
        registry: &Registry,
        id: NodeId,
    ) -> Result<&'static NodeKind, GraphError> {
        let node = self.nodes.get(&id).ok_or(GraphError::UnknownNode(id))?;
        Ok(registry.get(&node.kind).expect("graphs hold only registered kinds"))
    }

    /// Whether `to` is reachable from `from` along edges (or is `from`).
    pub(crate) fn reaches(&self, from: NodeId, to: NodeId) -> bool {
        let mut stack = vec![from];
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if id == to {
                return true;
            }
            if seen.insert(id) {
                stack.extend(
                    self.edges
                        .iter()
                        .filter(|(_, output)| output.0 == id)
                        .map(|(input, _)| input.0),
                );
            }
        }
        false
    }

    /// `targets` and all their ancestors, each after its sources.
    pub(crate) fn upstream_order(&self, targets: &[NodeId]) -> Vec<NodeId> {
        fn visit(graph: &Graph, id: NodeId, seen: &mut BTreeSet<NodeId>, order: &mut Vec<NodeId>) {
            if seen.insert(id) {
                for (_, output) in graph
                    .edges
                    .range(Port(id, String::new())..)
                    .take_while(|(input, _)| input.0 == id)
                {
                    visit(graph, output.0, seen, order);
                }
                order.push(id);
            }
        }
        let (mut seen, mut order) = (BTreeSet::new(), Vec::new());
        for &id in targets {
            visit(self, id, &mut seen, &mut order);
        }
        order
    }
}
