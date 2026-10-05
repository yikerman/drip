//! The node graph, with editing operations that keep it valid: connections
//! are type-compatible, every input has at most one source and the graph stays
//! acyclic. The file format lives in `project`, not here.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as Json;

use crate::node::NodeKind;
use crate::param::ParamMap;

/// Stable within a project; never reused while the graph is alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

/// A named port of a node, input or output depending on context.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Port(pub NodeId, pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Unique and renamable; how users and the CLI refer to the node.
    pub label: String,
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
    #[error("input `{input}` does not accept {found:?}")]
    TypeMismatch { input: String, found: &'static str },
    #[error("connection would create a cycle")]
    Cycle,
    #[error("node {0:?} has no parameter `{1}`")]
    UnknownParam(NodeId, String),
    #[error("invalid value {value} for parameter `{param}`")]
    InvalidParam { param: String, value: Json },
    #[error("label `{0}` is empty or already used")]
    InvalidLabel(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    nodes: BTreeMap<NodeId, Node>,
    /// Input port → the output port feeding it.
    edges: BTreeMap<Port, Port>,
    next_id: u64,
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

    /// The template's inputs: every external parameter of every node.
    pub fn inputs(&self) -> impl Iterator<Item = (NodeId, &'static str)> + '_ {
        self.nodes().flat_map(|(id, node)| node.external.iter().map(move |param| (id, *param)))
    }

    pub fn add_node(&mut self, kind: &'static NodeKind) -> NodeId {
        let label = (1..)
            .map(|n| if n == 1 { kind.label.to_string() } else { format!("{} {n}", kind.label) })
            .find(|label| self.find(label).is_none())
            .expect("unbounded");
        let node = Node {
            label,
            kind,
            params: kind
                .params
                .iter()
                .map(|p| (p.name.to_string(), p.kind.default_value()))
                .collect(),
            external: kind.params.iter().filter(|p| p.external).map(|p| p.name).collect(),
            ui: Json::Null,
        };
        let id = NodeId(self.next_id);
        self.insert(id, node);
        id
    }

    /// Adds a node built elsewhere (by the loader), keeping ids unique. The
    /// loader rejects the largest id, so this cannot overflow.
    pub(crate) fn insert(&mut self, id: NodeId, node: Node) {
        self.next_id = self.next_id.max(id.0 + 1);
        self.nodes.insert(id, node);
    }

    pub fn remove_node(&mut self, id: NodeId) -> Option<Node> {
        self.edges.retain(|input, output| input.0 != id && output.0 != id);
        self.nodes.remove(&id)
    }

    /// Connects `output` to `input`, replacing the input's previous source.
    pub fn connect(&mut self, output: Port, input: Port) -> Result<(), GraphError> {
        let source = self.node(output.0).ok_or(GraphError::UnknownNode(output.0))?.kind;
        let ty = source.outputs().find(|p| p.name == output.1).map(|p| p.ty);
        let ty = ty.ok_or_else(|| GraphError::UnknownPort(output.0, "output", output.1.clone()))?;
        let sink = self.node(input.0).ok_or(GraphError::UnknownNode(input.0))?.kind;
        let spec = sink.input(&input.1);
        let spec =
            spec.ok_or_else(|| GraphError::UnknownPort(input.0, "input", input.1.clone()))?;
        if !spec.requirement.accepts(ty) {
            return Err(GraphError::TypeMismatch { input: input.1, found: ty.name });
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

    /// `targets` and all their ancestors, each after its sources.
    pub(crate) fn upstream_order(&self, targets: &[NodeId]) -> Vec<NodeId> {
        fn visit(graph: &Graph, id: NodeId, seen: &mut BTreeSet<NodeId>, order: &mut Vec<NodeId>) {
            if seen.insert(id) {
                let inputs = graph.edges.range(Port(id, String::new())..);
                for (_, output) in inputs.take_while(|(input, _)| input.0 == id) {
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
