//! Editor state over the checked processing DAG. Parameters and edges live only
//! in the DAG; labels, template inputs and layout belong to the frontend.
mod project;
pub use drip::Error as GraphError;
pub use drip::definition::Kind;
pub use drip::ports::{NodeId, PortId as Port};
use drip::{Result, graph::Dag};
pub use project::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
pub struct Graph {
    pub dag: Dag,
    state: BTreeMap<NodeId, State>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct State {
    name: String,
    external: BTreeSet<String>,
    ui: Json,
}
/// A short-lived view, assembled from the DAG rather than a second parameter store.
pub struct Node {
    pub name: String,
    pub kind: &'static Kind,
    pub params: Json,
    pub external: BTreeSet<String>,
    pub ui: Json,
}
impl Node {
    /// The chosen file of the node's first file parameter that reads
    /// (`output` false) or writes (`output` true), as its schema declares.
    pub fn file(&self, output: bool) -> Option<&str> {
        self.kind.parameters.iter().find_map(|spec| match spec.kind {
            drip::param::ParamKind::Path { output: writes, .. } if writes == output => {
                self.params[spec.name].as_str()
            }
            _ => None,
        })
    }
}
impl Graph {
    pub fn from_dag(dag: Dag) -> Result<Self> {
        let mut graph = Self { dag, ..Self::default() };
        let ids: Vec<_> = graph.dag.nodes().map(|(id, _)| id).collect();
        for id in ids {
            let kind = crate::registry::REGISTRY
                .kind(graph.dag.node(id)?.kind().id)
                .ok_or_else(|| GraphError::Graph("unregistered editor node".into()))?;
            graph.initialize_state(id, kind);
        }
        Ok(graph)
    }
    pub fn kind(&self, id: NodeId) -> Option<&'static Kind> {
        self.dag.node(id).ok().map(|node| node.kind())
    }
    pub fn name(&self, id: NodeId) -> Option<&str> {
        self.state.get(&id).map(|state| state.name.as_str())
    }
    pub fn ui(&self, id: NodeId) -> Option<&Json> {
        self.state.get(&id).map(|state| &state.ui)
    }
    pub fn node(&self, id: NodeId) -> Option<Node> {
        let node = self.dag.node(id).ok()?;
        let kind = node.kind();
        let state = self.state.get(&id);
        Some(Node {
            name: state.map(|s| s.name.clone()).unwrap_or_else(|| kind.name.into()),
            kind,
            params: node.parameters().expect("editor nodes have serializable parameters"),
            external: state.map(|s| s.external.clone()).unwrap_or_default(),
            ui: state.map(|s| s.ui.clone()).unwrap_or_default(),
        })
    }
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, Node)> + '_ {
        self.dag.nodes().map(|(id, _)| (id, self.node(id).expect("registered editor node")))
    }
    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.state.iter().find(|(_, s)| s.name == name).map(|(&id, _)| id)
    }
    pub fn add_node(&mut self, kind: &'static Kind) -> Result<NodeId> {
        let id = self.dag.add((kind.build)((kind.defaults)())?)?;
        self.initialize_state(id, kind);
        Ok(id)
    }
    fn initialize_state(&mut self, id: NodeId, kind: &Kind) {
        let mut name = kind.name.to_owned();
        let mut suffix = 2;
        while self.state.values().any(|s| s.name == name) {
            name = format!("{} {suffix}", kind.name);
            suffix += 1;
        }
        self.state.insert(
            id,
            State {
                name,
                external: kind
                    .parameters
                    .iter()
                    .filter(|p| p.external)
                    .map(|p| p.name.into())
                    .collect(),
                ui: Json::Null,
            },
        );
    }

    pub fn remove_node(&mut self, id: NodeId) -> Result<()> {
        self.dag.remove(id)?;
        self.state.remove(&id);
        Ok(())
    }
    pub fn edges(&self) -> impl Iterator<Item = (Port, Port)> + '_ {
        self.dag.edges()
    }
    pub fn source(&self, input: Port) -> Option<Port> {
        self.edges().find(|(_, to)| *to == input).map(|(from, _)| from)
    }
    pub fn connect(&mut self, from: Port, to: Port) -> Result<()> {
        self.dag.edit(|edit| {
            edit.disconnect(to)?;
            edit.connect_ids(from, to)
        })
    }
    pub fn disconnect(&mut self, input: Port) -> Result<()> {
        self.dag.edit(|edit| edit.disconnect(input))
    }
    pub fn set_param(&mut self, id: NodeId, name: &str, value: Json) -> Result<()> {
        let mut params = self.dag.node(id)?.parameters()?;
        let field = params
            .get_mut(name)
            .ok_or_else(|| GraphError::Graph(format!("unknown parameter {name}")))?;
        *field = value;
        self.dag.set_parameters(id, params)
    }
    pub fn set_name(&mut self, id: NodeId, name: &str) -> Result<()> {
        if name.trim().is_empty() || self.find(name).is_some_and(|other| other != id) {
            return Err(GraphError::Graph("node names must be nonempty and unique".into()));
        }
        self.state_mut(id)?.name = name.into();
        Ok(())
    }
    pub fn set_external(&mut self, id: NodeId, name: &str, external: bool) -> Result<()> {
        let kind = self.dag.node(id)?.kind();
        if kind.param(name).is_none() {
            return Err(GraphError::Graph(format!("unknown parameter {name}")));
        }
        let inputs = &mut self.state_mut(id)?.external;
        if external {
            inputs.insert(name.into());
        } else {
            inputs.remove(name);
        }
        Ok(())
    }
    pub fn set_ui(&mut self, id: NodeId, ui: Json) -> Result<()> {
        self.state_mut(id)?.ui = ui;
        Ok(())
    }
    pub fn inputs(&self) -> impl Iterator<Item = (NodeId, &'static str)> + '_ {
        self.dag.nodes().flat_map(move |(id, node)| {
            node.kind()
                .parameters
                .iter()
                .filter(move |p| self.state[&id].external.contains(p.name))
                .map(move |p| (id, p.name))
        })
    }
    fn state_mut(&mut self, id: NodeId) -> Result<&mut State> {
        self.state.get_mut(&id).ok_or_else(|| GraphError::Graph("unknown node".into()))
    }
}
