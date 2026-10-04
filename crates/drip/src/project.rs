//! Projects (DESIGN F5). A template is a graph whose bound parameters form its
//! inputs; a project is a template applied to arguments.

use serde_json::Value as Json;

use crate::eval::NodeError;
use crate::graph::{Graph, GraphError, NodeId};
use crate::node::Registry;
use crate::param::ParamMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Project {
    pub graph: Graph,
    /// Values for the graph's inputs, by input name. Private so every value is
    /// validated against the parameters bound to its input.
    arguments: ParamMap,
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
}
