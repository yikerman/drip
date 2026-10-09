//! Persist node kinds, parameters and named edges; loading repeats connection checks.
use crate::{
    Error, Result, definition,
    graph::Dag,
    ports::{NodeId, PortId},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
pub struct Project {
    pub dag: Dag,
    pub target: Option<PortId>,
    /// Opaque frontend state. The headless library never interprets it.
    pub ui: Value,
    pub node_ui: BTreeMap<NodeId, Value>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    nodes: Vec<Record>,
    edges: Vec<(Endpoint, Endpoint)>,
    target: Option<Endpoint>,
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
            let specs = if input { n.inputs() } else { n.outputs() };
            let spec = specs
                .get(p.index())
                .ok_or_else(|| Error::Graph("invalid saved endpoint".into()))?;
            Ok(Endpoint { node: ids[&p.node()], port: spec.name.into() })
        };
        let document = Document {
            version: 2,
            nodes: nodes
                .iter()
                .map(|(id, n)| {
                    Ok(Record {
                        kind: n.metadata().id.into(),
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
            target: self.target.map(|p| endpoint(p, false)).transpose()?,
            ui: self.ui.clone(),
        };
        serde_json::to_string_pretty(&document).map_err(|e| Error::Graph(e.to_string()))
    }
    pub fn from_json(text: &str) -> Result<Self> {
        Self::load(serde_json::from_str(text).map_err(|e| Error::Graph(e.to_string()))?)
    }

    /// Instantiate only the saved target's dependency subgraph. Frontend node
    /// kinds outside that subgraph need not be registered by a batch frontend.
    pub fn target_from_json(text: &str) -> Result<Self> {
        let mut document: Document =
            serde_json::from_str(text).map_err(|e| Error::Graph(e.to_string()))?;
        let target = document
            .target
            .as_ref()
            .ok_or_else(|| Error::Graph("project has no unambiguous export target".into()))?;
        let mut needed = BTreeSet::new();
        let target_node = target.node;
        let mut pending = vec![target_node];
        while let Some(node) = pending.pop() {
            if node >= document.nodes.len() {
                return Err(Error::Graph("unknown project node".into()));
            }
            if needed.insert(node) {
                pending.extend(
                    document
                        .edges
                        .iter()
                        .filter(|(_, to)| to.node == node)
                        .map(|(from, _)| from.node),
                );
            }
        }
        let indices: BTreeMap<_, _> =
            needed.iter().enumerate().map(|(new, &old)| (old, new)).collect();
        document.nodes = document
            .nodes
            .into_iter()
            .enumerate()
            .filter_map(|(id, node)| needed.contains(&id).then_some(node))
            .collect();
        document.edges.retain(|(_, to)| needed.contains(&to.node));
        for (from, to) in &mut document.edges {
            from.node = indices[&from.node];
            to.node = indices[&to.node];
        }
        document.target.as_mut().unwrap().node = indices[&target_node];
        Self::load(document)
    }

    fn load(document: Document) -> Result<Self> {
        if document.version != 2 {
            return Err(Error::Graph(
                "unsupported project version; legacy projects require migration".into(),
            ));
        }
        let mut project = Self { ui: document.ui, ..Self::default() };
        let mut ids = Vec::new();
        for record in document.nodes {
            let id = project.dag.add(definition::registered(&record.kind, record.parameters)?)?;
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
        project.target = document.target.map(|e| endpoint(e, false)).transpose()?;
        project.dag.edit(|edit| {
            for (a, b) in edges {
                edit.connect_ids(a, b)?;
            }
            Ok(())
        })?;
        Ok(project)
    }
}
