//! Projects and their file format (DESIGN F1-F3, F5). A template is a graph
//! whose bound parameters form its inputs; a project is a template applied to
//! arguments. Both are saved in the same JSON format.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::eval::NodeError;
use crate::graph::{Graph, GraphError, Node, NodeId, Port};
use crate::node::{Migration, Registry};
use crate::param::ParamMap;

const FORMAT: &str = "drip";
const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Project {
    pub graph: Graph,
    /// Values for the graph's inputs, by input name. Private so every value is
    /// validated against the parameters bound to its input.
    arguments: ParamMap,
    /// Opaque frontend state such as the editor's viewport.
    pub ui: Json,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("malformed project file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not a drip project file")]
    NotDrip,
    #[error("project file format {0} is newer than this build supports")]
    NewerFormat(u32),
    #[error("input {0:?} has more than one connection")]
    DuplicateInput(Port),
    #[error("node id {0:?} is duplicated or out of range")]
    InvalidId(NodeId),
    #[error(transparent)]
    Graph(#[from] GraphError),
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    /// A list rather than a map keyed by id, so duplicate ids are detected
    /// instead of silently collapsed by the JSON parser.
    nodes: Vec<FileNode>,
    #[serde(default)]
    edges: Vec<Edge>,
    #[serde(default, skip_serializing_if = "ParamMap::is_empty")]
    arguments: ParamMap,
    #[serde(default, skip_serializing_if = "Json::is_null")]
    ui: Json,
}

#[derive(Serialize, Deserialize)]
struct FileNode {
    id: NodeId,
    #[serde(flatten)]
    node: Node,
}

#[derive(Serialize, Deserialize)]
struct Edge {
    from: Port,
    to: Port,
}

impl Project {
    /// The same graph without arguments, i.e. a function of its inputs.
    pub fn template(&self) -> Project {
        Project { arguments: ParamMap::new(), ..self.clone() }
    }

    pub fn arguments(&self) -> &ParamMap {
        &self.arguments
    }

    /// Makes parameter `name` of node `id` take its value from graph input `input`.
    pub fn bind(
        &mut self,
        registry: &Registry,
        id: NodeId,
        name: &str,
        input: &str,
    ) -> Result<(), GraphError> {
        let spec = self
            .graph
            .kind(registry, id)?
            .param(name)
            .ok_or_else(|| GraphError::UnknownParam(id, name.into()))?;
        if let Some(value) = self.arguments.get(input).filter(|value| !spec.kind.accepts(value)) {
            return Err(GraphError::InvalidParam { param: name.into(), value: value.clone() });
        }
        self.graph.node_mut(id)?.bindings.insert(name.into(), input.into());
        Ok(())
    }

    pub fn unbind(&mut self, id: NodeId, name: &str) -> Option<String> {
        self.graph.node_mut(id).ok()?.bindings.remove(name)
    }

    /// Sets graph input `input`, checked against every parameter bound to it.
    pub fn set_argument(
        &mut self,
        registry: &Registry,
        input: &str,
        value: Json,
    ) -> Result<(), GraphError> {
        if !self.graph.inputs().contains(input) {
            return Err(GraphError::UnknownInput(input.into()));
        }
        self.check_argument(registry, input, &value)?;
        self.arguments.insert(input.into(), value);
        Ok(())
    }

    fn check_argument(
        &self,
        registry: &Registry,
        input: &str,
        value: &Json,
    ) -> Result<(), GraphError> {
        for (id, node) in self.graph.nodes() {
            let Ok(kind) = self.graph.kind(registry, id) else {
                continue;
            };
            for (param, _) in node.bindings.iter().filter(|(_, bound)| *bound == input) {
                if !kind.param(param).expect("validated binding").kind.accepts(value) {
                    return Err(GraphError::InvalidParam {
                        param: param.clone(),
                        value: value.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Node `id`'s literal params with bound ones replaced by arguments.
    pub(crate) fn effective_params(&self, id: NodeId) -> Result<ParamMap, NodeError> {
        let node = self.graph.node(id).expect("in graph");
        let mut params = node.params.clone();
        for (param, input) in &node.bindings {
            let value = self
                .arguments
                .get(input)
                .ok_or_else(|| NodeError::MissingArgument(input.clone()))?;
            params.insert(param.clone(), value.clone());
        }
        Ok(params)
    }

    pub fn to_json(&self) -> String {
        let file = File {
            format: FORMAT.into(),
            version: VERSION,
            nodes: self
                .graph
                .nodes()
                .map(|(id, node)| FileNode { id, node: node.clone() })
                .collect(),
            edges: self
                .graph
                .edges()
                .map(|(from, to)| Edge { from: from.clone(), to: to.clone() })
                .collect(),
            arguments: self.arguments.clone(),
            ui: self.ui.clone(),
        };
        serde_json::to_string_pretty(&file).expect("plain data serializes")
    }

    /// Loads a project, migrating nodes saved by older kind versions. Nodes of
    /// unknown or newer kinds are kept as they are; the returned warnings say
    /// which, along with unknown parameters and unused arguments.
    pub fn from_json(text: &str, registry: &Registry) -> Result<(Project, Vec<String>), LoadError> {
        let file: File = serde_json::from_str(text)?;
        if file.format != FORMAT {
            return Err(LoadError::NotDrip);
        }
        if file.version > VERSION {
            return Err(LoadError::NewerFormat(file.version));
        }
        let mut warnings = Vec::new();
        let mut edges: Vec<_> = file.edges.into_iter().map(|edge| (edge.from, edge.to)).collect();
        let mut graph = Graph::default();
        for FileNode { id, mut node } in file.nodes {
            graph.next_id = id.0.checked_add(1).ok_or(LoadError::InvalidId(id))?.max(graph.next_id);
            if graph.node(id).is_some() {
                return Err(LoadError::InvalidId(id));
            }
            match registry.get(&node.kind) {
                Some(kind) if node.kind_version <= kind.version => {
                    if node.kind_version < kind.version {
                        let mut migration = Migration {
                            params: &mut node.params,
                            bindings: &mut node.bindings,
                            renamed_inputs: vec![],
                            renamed_outputs: vec![],
                        };
                        if let Some(migrate) = kind.migrate {
                            migrate(node.kind_version, &mut migration);
                        }
                        rename_ports(
                            &mut edges,
                            id,
                            migration.renamed_inputs,
                            migration.renamed_outputs,
                        );
                        node.kind_version = kind.version;
                    }
                    for spec in kind.params {
                        let value = node
                            .params
                            .entry(spec.name)
                            .or_insert_with(|| spec.kind.default_value());
                        if !spec.kind.accepts(value) {
                            return Err(GraphError::InvalidParam {
                                param: spec.name.into(),
                                value: value.clone(),
                            }
                            .into());
                        }
                    }
                    for name in node.params.keys().filter(|name| kind.param(name).is_none()) {
                        warnings.push(format!("`{}`: unknown parameter `{name}` kept", node.label));
                    }
                    if let Some(name) = node.bindings.keys().find(|name| kind.param(name).is_none())
                    {
                        return Err(GraphError::UnknownParam(id, name.clone()).into());
                    }
                }
                Some(_) => warnings
                    .push(format!("`{}`: saved by a newer version of `{}`", node.label, node.kind)),
                None => {
                    warnings.push(format!("`{}`: unknown node kind `{}`", node.label, node.kind))
                }
            }
            if node.label.is_empty() || graph.find(&node.label).is_some() {
                return Err(GraphError::InvalidLabel(node.label).into());
            }
            graph.nodes.insert(id, node);
        }
        for (from, to) in edges {
            for id in [from.0, to.0] {
                graph.node(id).ok_or(GraphError::UnknownNode(id))?;
            }
            if graph.edges.contains_key(&to) {
                return Err(LoadError::DuplicateInput(to));
            }
            let (source, sink) =
                (graph.kind(registry, from.0).ok(), graph.kind(registry, to.0).ok());
            if let Some(kind) = source
                && kind.output_index(&from.1).is_none()
            {
                return Err(GraphError::UnknownPort(from.0, "output", from.1).into());
            }
            if let Some(kind) = sink
                && kind.input(&to.1).is_none()
            {
                return Err(GraphError::UnknownPort(to.0, "input", to.1).into());
            }
            if source.is_some() && sink.is_some() {
                graph.connect(registry, from, to)?;
            } else if graph.reaches(to.0, from.0) {
                return Err(GraphError::Cycle.into());
            } else {
                graph.edges.insert(to, from);
            }
        }
        let project = Project { graph, arguments: ParamMap::new(), ui: file.ui };
        for (input, value) in &file.arguments {
            if !project.graph.inputs().contains(input.as_str()) {
                warnings.push(format!("argument `{input}` is not used by any node"));
            }
            project.check_argument(registry, input, value)?;
        }
        Ok((Project { arguments: file.arguments, ..project }, warnings))
    }
}

fn rename_ports(
    edges: &mut [(Port, Port)],
    id: NodeId,
    inputs: Vec<(String, String)>,
    outputs: Vec<(String, String)>,
) {
    for (from, to) in edges {
        for (port, renames) in [(to, &inputs), (from, &outputs)] {
            if let Some((_, new)) = renames.iter().find(|(old, _)| port.0 == id && port.1 == *old) {
                port.1 = new.clone();
            }
        }
    }
}
