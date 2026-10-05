//! Pull-based evaluation. Frontends request target
//! nodes; only those and their ancestors are evaluated, in dependency order.
//! A node is recomputed only when its dependency stamp changes: a hash of
//! its kind, parameters, level and the stamps of its sources. Files stay cached
//! until the frontend replaces the evaluator.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::graph::{Graph, NodeId, Port};
use crate::node::{EvalContext, Evaluated};
use crate::param::Params;
use crate::resource::Resources;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NodeError {
    #[error("input `{0}` is not connected")]
    MissingInput(&'static str),
    #[error("upstream node {0:?} failed")]
    Upstream(NodeId),
    #[error("no action `{0}`")]
    UnknownAction(String),
    #[error("{0}")]
    Failed(String),
}

pub type NodeResult = Result<Evaluated, NodeError>;

/// Keeps one result per node, and the files nodes have loaded, between
/// evaluations.
#[derive(Default)]
pub struct Evaluator {
    cache: Cache,
    resources: Resources,
}

#[derive(Default)]
struct Cache(HashMap<NodeId, Entry>);

struct Entry {
    stamp: u64,
    result: NodeResult,
}

impl Evaluator {
    /// A fresh node cache sharing loaded resources, including future first loads.
    pub fn fork(&self) -> Self {
        Self { cache: Cache::default(), resources: self.resources.clone() }
    }

    /// Brings `targets` up to date at downscale `level` and returns the nodes
    /// actually recomputed.
    pub fn evaluate(&mut self, graph: &Graph, level: u8, targets: &[NodeId]) -> Vec<NodeId> {
        self.cache.0.retain(|id, _| graph.node(*id).is_some());
        self.run(graph, level, targets, false)
    }

    pub fn result(&self, id: NodeId) -> Option<&NodeResult> {
        self.cache.0.get(&id).map(|entry| &entry.result)
    }

    /// Consumes this evaluator for a full-resolution action, releasing
    /// intermediates after their last consumer. Fork first to keep a preview cache.
    pub fn run_action(mut self, graph: &Graph, id: NodeId, name: &str) -> Result<(), NodeError> {
        let node = graph.node(id).expect("in graph");
        let action = node.kind.action(name).ok_or_else(|| NodeError::UnknownAction(name.into()))?;
        self.cache = Cache::default();
        let targets: Vec<_> = sources(graph, id).collect();
        self.run(graph, 0, &targets, true);
        let inputs = self.cache.inputs(graph, id)?;
        let ctx = EvalContext { level: 0, resources: &self.resources };
        (action.run)(Params(&node.params), &inputs, &ctx).map_err(NodeError::Failed)
    }

    /// With `release`, a result is dropped once its last consumer in this run
    /// has been computed (unless it is a target), bounding memory for one-off
    /// full-resolution runs.
    fn run(&mut self, graph: &Graph, level: u8, targets: &[NodeId], release: bool) -> Vec<NodeId> {
        let ctx = EvalContext { level, resources: &self.resources };
        let order = graph.upstream_order(targets);
        let mut pending: HashMap<NodeId, usize> = HashMap::new();
        if release {
            for &id in &order {
                for source in sources(graph, id) {
                    *pending.entry(source).or_default() += 1;
                }
            }
        }
        let mut computed = Vec::new();
        for &id in &order {
            let stamp = self.cache.stamp(graph, &ctx, id);
            if self.cache.0.get(&id).is_none_or(|entry| entry.stamp != stamp) {
                let start = std::time::Instant::now();
                let result = self.cache.compute(graph, &ctx, id);
                let kind = graph.node(id).expect("in graph").kind.name;
                log::debug!("evaluated {kind} {id:?} at level {level} in {:.1?}", start.elapsed());
                self.cache.0.insert(id, Entry { stamp, result });
                computed.push(id);
            }
            if release {
                for source in sources(graph, id) {
                    let left = pending.get_mut(&source).expect("counted above");
                    *left -= 1;
                    if *left == 0 && !targets.contains(&source) {
                        self.cache.0.remove(&source);
                    }
                }
            }
        }
        computed
    }
}

impl Cache {
    fn stamp(&self, graph: &Graph, ctx: &EvalContext, id: NodeId) -> u64 {
        let node = graph.node(id).expect("in graph");
        let mut h = DefaultHasher::new();
        (node.kind.name, ctx.level).hash(&mut h);
        serde_json::to_string(&node.params).expect("plain data serializes").hash(&mut h);
        for spec in node.kind.inputs {
            spec.name.hash(&mut h);
            if let Some(source) = graph.source(&Port(id, spec.name.into())) {
                (source, self.0.get(&source.0).map(|entry| entry.stamp)).hash(&mut h);
            }
        }
        h.finish()
    }

    fn compute(&self, graph: &Graph, ctx: &EvalContext, id: NodeId) -> NodeResult {
        let node = graph.node(id).expect("in graph");
        let inputs = self.inputs(graph, id)?;
        (node.kind.eval)(Params(&node.params), &inputs, ctx).map_err(NodeError::Failed)
    }

    /// The values on node `id`'s inputs, from its sources' cached results.
    fn inputs(&self, graph: &Graph, id: NodeId) -> Result<Vec<Value>, NodeError> {
        let kind = graph.node(id).expect("in graph").kind;
        kind.inputs
            .iter()
            .map(|spec| {
                let source = graph.source(&Port(id, spec.name.into()));
                let source = source.ok_or(NodeError::MissingInput(spec.name))?;
                let Ok(evaluated) = &self.0[&source.0].result else {
                    return Err(NodeError::Upstream(source.0));
                };
                let index = graph.node(source.0).expect("in graph").kind.output_index(&source.1);
                Ok(evaluated.outputs[index.expect("validated edge")].clone())
            })
            .collect()
    }
}

fn sources(graph: &Graph, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    graph.edges().filter(move |(_, input)| input.0 == id).map(|(output, _)| output.0)
}

/// Runs a standalone action with fresh resources. Interactive callers use
/// `Evaluator::fork().run_action(...)` to reuse their session's loaded files.
pub fn run_action(graph: &Graph, id: NodeId, name: &str) -> Result<(), NodeError> {
    Evaluator::default().run_action(graph, id, name)
}
