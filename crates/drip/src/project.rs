//! Persist node kinds, parameters and named edges; loading repeats connection checks.
use crate::{
    Error, Result,
    definition::Registry,
    graph::Dag,
    ports::{NodeId, PortId},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Default)]
pub struct Project {
    pub dag: Dag,
    /// Opaque frontend state. The headless library never interprets it.
    pub ui: Value,
    pub node_ui: BTreeMap<NodeId, Value>,
}
/// The project format follows the crate's major version: a format change is a
/// breaking change. Other versions are rejected without migration.
const VERSION: u32 = match u32::from_str_radix(env!("CARGO_PKG_VERSION_MAJOR"), 10) {
    Ok(version) => version,
    Err(_) => panic!("numeric crate major version"),
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    nodes: Vec<Record>,
    edges: Vec<(Endpoint, Endpoint)>,
    ui: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    kind: String,
    parameters: Value,
    ui: Value,
}
#[derive(Serialize, Deserialize)]
struct Endpoint {
    node: usize,
    port: String,
}
impl Project {
    pub fn to_json(&self) -> Result<String> {
        let nodes: Vec<_> = self.dag.nodes().collect();
        let ids: BTreeMap<_, _> = nodes.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
        let endpoint = |p: PortId, input: bool| -> Result<Endpoint> {
            if !input {
                self.dag.output(p)?;
            }
            let n = self.dag.node(p.node())?;
            let specs = if input { n.kind().inputs } else { n.kind().outputs };
            let spec = specs
                .get(p.index())
                .ok_or_else(|| Error::Graph("invalid saved endpoint".into()))?;
            Ok(Endpoint { node: ids[&p.node()], port: spec.name.into() })
        };
        let document = Document {
            version: VERSION,
            nodes: nodes
                .iter()
                .map(|(id, n)| {
                    Ok(Record {
                        kind: n.kind().id.into(),
                        parameters: n.parameters()?,
                        ui: self.node_ui.get(id).cloned().unwrap_or(Value::Null),
                    })
                })
                .collect::<Result<_>>()?,
            edges: self
                .dag
                .edges()
                .map(|(a, b)| Ok((endpoint(a, false)?, endpoint(b, true)?)))
                .collect::<Result<_>>()?,
            ui: self.ui.clone(),
        };
        serde_json::to_string_pretty(&document).map_err(|e| Error::Graph(e.to_string()))
    }
    /// Load a project whose node kinds are all in `registry`.
    pub fn from_json(text: &str, registry: &Registry) -> Result<Self> {
        let document = serde_json::from_str(text).map_err(|e| Error::Graph(e.to_string()))?;
        Self::load(document, registry)
    }

    fn load(document: Document, registry: &Registry) -> Result<Self> {
        if document.version != VERSION {
            return Err(Error::Graph(format!(
                "project version {} is not {VERSION}; projects are not migrated",
                document.version
            )));
        }
        let mut project = Self { ui: document.ui, ..Self::default() };
        let mut ids = Vec::new();
        for record in document.nodes {
            let id = project.dag.add(registry.build(&record.kind, record.parameters)?)?;
            ids.push(id);
            project.node_ui.insert(id, record.ui);
        }
        let endpoint = |e: Endpoint, input: bool| -> Result<PortId> {
            let id = *ids.get(e.node).ok_or_else(|| Error::Graph("unknown project node".into()))?;
            if input {
                project.dag.input_port(id, &e.port)
            } else {
                project.dag.output_port(id, &e.port)
            }
        };
        let edges = document
            .edges
            .into_iter()
            .map(|(a, b)| Ok((endpoint(a, false)?, endpoint(b, true)?)))
            .collect::<Result<Vec<_>>>()?;
        project.dag.edit(|edit| {
            for (a, b) in edges {
                edit.connect_ids(a, b)?;
            }
            Ok(())
        })?;
        Ok(project)
    }
}
