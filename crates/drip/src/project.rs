//! Projects and their file format. A template is a
//! graph whose external parameters are its inputs; a project is the same
//! graph with those parameters filled in. Both are saved in the same format,
//! defined here by private types so the in-memory model can change freely.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::graph::{Graph, GraphError, Node, NodeId, Port};
use crate::node::Registry;
use crate::param::ParamMap;

const FORMAT: &str = "drip";

/// Files carry the major version of the crate that wrote them, and only that
/// major version reads them. The prototype keeps no compatibility otherwise:
/// a file must match exactly what this build writes.
const VERSION: u32 = match u32::from_str_radix(env!("CARGO_PKG_VERSION_MAJOR"), 10) {
    Ok(major) => major,
    Err(_) => panic!("the crate's major version is a number"),
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Project {
    pub graph: Graph,
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
    #[error("unknown node kind `{0}`")]
    UnknownKind(String),
    #[error("node {0:?} has no value for parameter `{1}`")]
    MissingParam(NodeId, &'static str),
    #[error("input {0:?} has more than one connection")]
    DuplicateInput(Port),
    #[error("node id {0} is duplicated or out of range")]
    InvalidId(u64),
    #[error(transparent)]
    Graph(#[from] GraphError),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: String,
    version: u32,
    /// A list rather than a map keyed by id, so duplicate ids are detected
    /// instead of silently collapsed by the JSON parser.
    nodes: Vec<FileNode>,
    edges: Vec<FileEdge>,
    #[serde(default, skip_serializing_if = "Json::is_null")]
    ui: Json,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileNode {
    id: u64,
    label: String,
    kind: String,
    params: ParamMap,
    external: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Json::is_null")]
    ui: Json,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEdge {
    from: (u64, String),
    to: (u64, String),
}

impl Project {
    /// The same graph with every external parameter back at its default:
    /// a function of its inputs.
    pub fn template(&self) -> Project {
        let mut template = self.clone();
        for (id, param) in self.graph.inputs() {
            let spec = self.graph.node(id).expect("listed").kind.param(param).expect("listed");
            template
                .graph
                .set_param(id, param, spec.kind.default_value())
                .expect("defaults are valid");
        }
        template
    }

    pub fn to_json(&self) -> String {
        let nodes = self.graph.nodes().map(|(id, node)| FileNode {
            id: id.0,
            label: node.name.clone(),
            kind: node.kind.id.into(),
            params: node.params.clone(),
            external: node.external.iter().map(|p| p.to_string()).collect(),
            ui: node.ui.clone(),
        });
        let edges = self.graph.edges().map(|(from, to)| FileEdge {
            from: (from.0.0, from.1.clone()),
            to: (to.0.0, to.1.clone()),
        });
        let file = File {
            format: FORMAT.into(),
            version: VERSION,
            nodes: nodes.collect(),
            edges: edges.collect(),
            ui: self.ui.clone(),
        };
        serde_json::to_string_pretty(&file).expect("plain data serializes")
    }

    /// Loads a project, validating everything as editing would.
    pub fn from_json(text: &str, registry: &Registry) -> Result<Project, LoadError> {
        let file: File = serde_json::from_str(text)?;
        if file.format != FORMAT {
            return Err(LoadError::NotDrip);
        }
        if file.version != VERSION {
            return Err(LoadError::Version(file.version));
        }
        let mut graph = Graph::default();
        for n in file.nodes {
            let id = NodeId(n.id);
            if n.id == u64::MAX || graph.node(id).is_some() {
                return Err(LoadError::InvalidId(n.id));
            }
            let kind =
                registry.get(&n.kind).ok_or_else(|| LoadError::UnknownKind(n.kind.clone()))?;
            let unknown =
                n.params.keys().chain(&n.external).find(|name| kind.param(name).is_none());
            if let Some(name) = unknown {
                return Err(GraphError::UnknownParam(id, name.clone()).into());
            }
            for spec in kind.params {
                let value =
                    n.params.get(spec.name).ok_or(LoadError::MissingParam(id, spec.name))?;
                if !spec.kind.accepts(value) {
                    return Err(GraphError::InvalidParam {
                        param: spec.name.into(),
                        value: value.clone(),
                    }
                    .into());
                }
            }
            let external = kind
                .params
                .iter()
                .filter(|p| n.external.contains(p.name))
                .map(|p| p.name)
                .collect();
            if n.label.is_empty() || graph.find(&n.label).is_some() {
                return Err(GraphError::InvalidName(n.label).into());
            }
            graph.insert(id, Node { name: n.label, kind, params: n.params, external, ui: n.ui });
        }
        for FileEdge { from, to } in file.edges {
            let (from, to) = (Port(NodeId(from.0), from.1), Port(NodeId(to.0), to.1));
            if graph.source(&to).is_some() {
                return Err(LoadError::DuplicateInput(to));
            }
            graph.connect(from, to)?;
        }
        Ok(Project { graph, ui: file.ui })
    }
}
