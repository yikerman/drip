//! Projects and their file format (DESIGN F1, F2, F5, F6). A template is a
//! graph whose bound parameters form its inputs; a project is a template
//! applied to arguments. Both are saved in the same JSON format.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::eval::NodeError;
use crate::graph::{Graph, GraphError, Node, NodeId, Port};
use crate::node::Registry;
use crate::param::{ParamKind, ParamMap};

const FORMAT: &str = "drip";

/// Files carry the major version of the crate that wrote them. Within a major
/// version, files stay compatible (semantic versioning, DESIGN F6); across
/// major versions they are rejected rather than migrated.
const VERSION: u32 = match u32::from_str_radix(env!("CARGO_PKG_VERSION_MAJOR"), 10) {
    Ok(major) => major,
    Err(_) => panic!("the crate's major version is a number"),
};

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
    #[error("written by Drip {0}.x; this is Drip {VERSION}.x")]
    Version(u32),
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

    /// The kind of value graph input `input` takes: that of the parameters
    /// bound to it, which all share one kind.
    pub fn input_kind(&self, registry: &Registry, input: &str) -> Option<ParamKind> {
        self.graph.nodes().find_map(|(id, node)| {
            let (param, _) = node.bindings.iter().find(|(_, bound)| *bound == input)?;
            let kind = self.graph.kind(registry, id).expect("node exists");
            Some(kind.param(param).expect("bindings name parameters").kind)
        })
    }

    /// Makes parameter `name` of node `id` take its value from graph input
    /// `input`, which must not already take a different kind of value.
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
        if let Some(kind) = self.input_kind(registry, input)
            && std::mem::discriminant(&kind) != std::mem::discriminant(&spec.kind)
        {
            return Err(GraphError::IncompatibleInput { input: input.into(), param: name.into() });
        }
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
        if !self.graph.inputs().contains(input) {
            return Err(GraphError::UnknownInput(input.into()));
        }
        for (id, node) in self.graph.nodes() {
            let kind = self.graph.kind(registry, id).expect("node exists");
            for (param, _) in node.bindings.iter().filter(|(_, bound)| *bound == input) {
                if !kind.param(param).expect("bindings name parameters").kind.accepts(value) {
                    let param = param.clone();
                    return Err(GraphError::InvalidParam { param, value: value.clone() });
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

    /// Loads a project written by this major version, validating everything
    /// as editing would. Parameters missing from the file take their defaults,
    /// so a minor version may add parameters without breaking older files.
    pub fn from_json(text: &str, registry: &Registry) -> Result<Project, LoadError> {
        let file: File = serde_json::from_str(text)?;
        if file.format != FORMAT {
            return Err(LoadError::NotDrip);
        }
        if file.version != VERSION {
            return Err(LoadError::Version(file.version));
        }
        let mut graph = Graph::default();
        let mut bindings = Vec::new();
        for FileNode { id, mut node } in file.nodes {
            graph.next_id = id.0.checked_add(1).ok_or(LoadError::InvalidId(id))?.max(graph.next_id);
            if graph.node(id).is_some() {
                return Err(LoadError::InvalidId(id));
            }
            let kind = registry
                .get(&node.kind)
                .ok_or_else(|| GraphError::UnknownKind(node.kind.clone()))?;
            let unknown = node
                .params
                .keys()
                .chain(node.bindings.keys())
                .find(|name| kind.param(name).is_none());
            if let Some(name) = unknown {
                return Err(GraphError::UnknownParam(id, name.clone()).into());
            }
            for spec in kind.params {
                let value =
                    node.params.entry(spec.name).or_insert_with(|| spec.kind.default_value());
                if !spec.kind.accepts(value) {
                    let param = spec.name.into();
                    return Err(GraphError::InvalidParam { param, value: value.clone() }.into());
                }
            }
            if node.label.is_empty() || graph.find(&node.label).is_some() {
                return Err(GraphError::InvalidLabel(node.label).into());
            }
            // Re-applied below through `bind`, which checks their inputs agree.
            bindings
                .extend(std::mem::take(&mut node.bindings).into_iter().map(|(p, i)| (id, p, i)));
            graph.nodes.insert(id, node);
        }
        for Edge { from, to } in file.edges {
            if graph.edges.contains_key(&to) {
                return Err(LoadError::DuplicateInput(to));
            }
            graph.connect(registry, from, to)?;
        }
        let mut project = Project { graph, arguments: ParamMap::new(), ui: file.ui };
        for (id, param, input) in bindings {
            project.bind(registry, id, &param, &input)?;
        }
        for (input, value) in file.arguments {
            project.set_argument(registry, &input, value)?;
        }
        Ok(project)
    }
}
