//! Frontend labels and layout are serialized through the core's opaque UI state.
use super::{Graph, Registry, State};
use drip::Result;
use serde_json::Value;

#[derive(Clone, Default)]
pub struct Project {
    pub graph: Graph,
    pub ui: Value,
}

impl Project {
    pub fn template(&self) -> Result<Self> {
        let mut template = self.clone();
        for (id, param) in self.graph.inputs() {
            let node = self.graph.node(id).unwrap();
            template.graph.set_param(
                id,
                param,
                node.kind.param(param).unwrap().kind.default_value(),
            )?;
        }
        Ok(template)
    }
    pub fn to_json(&self) -> Result<String> {
        let mut project = drip::project::Project {
            dag: self.graph.dag.clone(),
            ui: self.ui.clone(),
            ..Default::default()
        };
        // A single explicit export image is an unambiguous batch target.
        // Multiple exports remain a frontend choice rather than an inferred one.
        let targets: Vec<_> = self
            .graph
            .nodes()
            .filter(|(_, n)| n.kind.id == "output.tiff")
            .filter_map(|(id, _)| self.graph.source(&super::Port(id, "image".into())))
            .map(|p| self.graph.dag.output_port(p.0, &p.1))
            .collect::<Result<_>>()?;
        if let [target] = targets.as_slice() {
            project.target = Some(*target);
        }
        for (id, state) in &self.graph.state {
            project.node_ui.insert(
                *id,
                serde_json::to_value(state).map_err(|e| drip::Error::Graph(e.to_string()))?,
            );
        }
        project.to_json()
    }
    pub fn from_json(text: &str, registry: &Registry) -> Result<Self> {
        let core = drip::project::Project::from_json(text)?;
        let mut graph = Graph { dag: core.dag, ..Default::default() };
        let mut missing = Vec::new();
        for (id, node) in graph.dag.nodes() {
            let kind = registry
                .get(node.metadata().id)
                .ok_or_else(|| drip::Error::Graph("unknown editor node".into()))?;
            let state: State = match core.node_ui.get(&id).filter(|v| !v.is_null()) {
                Some(value) => serde_json::from_value(value.clone())
                    .map_err(|e| drip::Error::Graph(e.to_string()))?,
                None => {
                    missing.push((id, kind));
                    continue;
                }
            };
            if state.name.trim().is_empty()
                || graph.state.values().any(|s| s.name == state.name)
                || state.external.iter().any(|p| kind.param(p).is_none())
            {
                return Err(drip::Error::Graph("invalid node label or template input".into()));
            }
            graph.state.insert(id, state);
        }
        for (id, kind) in missing {
            graph.initialize_state(id, kind);
        }
        Ok(Self { graph, ui: core.ui })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_names_use_kind_suffixes_and_respect_saved_names() {
        let mut core = drip::project::Project::default();
        drip::node::exposure::add(&mut core.dag, Default::default()).unwrap();
        let b = drip::node::exposure::add(&mut core.dag, Default::default()).unwrap();
        core.node_ui
            .insert(b.node, serde_json::json!({"name":"Exposure", "external":[], "ui":null}));
        let mut project = Project::from_json(&core.to_json().unwrap(), &Registry).unwrap();
        let names: Vec<_> = project.graph.nodes().map(|(_, node)| node.name).collect();
        assert_eq!(names, ["Exposure 2", "Exposure"]);
        let added = project.graph.add_node(Registry.get("tone.exposure").unwrap()).unwrap();
        assert_eq!(project.graph.node(added).unwrap().name, "Exposure 3");
    }
}
