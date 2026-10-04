//! Pull-based evaluation (DESIGN E1-E3, E6, U2). Frontends request target
//! nodes; only those and their ancestors are evaluated, in dependency order.
//! A node is recomputed only when its dependency stamp changes: a hash of
//! everything its result depends on, including the stamps of its sources.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::graph::{NodeId, Port};
use crate::node::{EvalContext, Evaluated, NodeKind, Registry};
use crate::param::{ParamMap, Params};
use crate::project::Project;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NodeError {
    #[error("unknown node kind `{0}`")]
    UnknownKind(String),
    #[error("saved by a newer version ({saved}) of `{kind}` than this build knows ({known})")]
    NewerVersion { kind: String, saved: u32, known: u32 },
    #[error("input `{0}` is not connected")]
    MissingInput(&'static str),
    #[error("no argument given for graph input `{0}`")]
    MissingArgument(String),
    #[error("upstream node {0:?} failed")]
    Upstream(NodeId),
    #[error("no action `{0}`")]
    UnknownAction(String),
    #[error("{0}")]
    Failed(String),
}

pub type NodeResult = Result<Evaluated, NodeError>;

/// Keeps one result per node between evaluations.
#[derive(Default)]
pub struct Evaluator {
    cache: HashMap<NodeId, Entry>,
}

struct Entry {
    stamp: u64,
    result: NodeResult,
}

impl Evaluator {
    /// Brings `targets` up to date and returns the nodes actually recomputed.
    pub fn evaluate(
        &mut self,
        project: &Project,
        registry: &Registry,
        ctx: EvalContext,
        targets: &[NodeId],
    ) -> Vec<NodeId> {
        self.cache.retain(|id, _| project.graph.node(*id).is_some());
        self.run(project, registry, ctx, targets, false)
    }

    pub fn result(&self, id: NodeId) -> Option<&NodeResult> {
        self.cache.get(&id).map(|entry| &entry.result)
    }

    /// With `release`, a result is dropped once its last consumer in this run
    /// has been computed (unless it is a target), bounding memory for one-off
    /// full-resolution runs.
    fn run(
        &mut self,
        project: &Project,
        registry: &Registry,
        ctx: EvalContext,
        targets: &[NodeId],
        release: bool,
    ) -> Vec<NodeId> {
        let graph = &project.graph;
        let order = graph.upstream_order(targets);
        let mut pending: HashMap<NodeId, usize> = HashMap::new();
        if release {
            for &id in &order {
                for source in sources(project, id) {
                    *pending.entry(source).or_default() += 1;
                }
            }
        }
        let mut computed = Vec::new();
        for &id in &order {
            let stamp = self.stamp(project, registry, ctx, id);
            if self.cache.get(&id).is_none_or(|entry| entry.stamp != stamp) {
                let result = self.compute(project, registry, &ctx, id);
                self.cache.insert(id, Entry { stamp, result });
                computed.push(id);
            }
            if release {
                for source in sources(project, id) {
                    let left = pending.get_mut(&source).expect("counted above");
                    *left -= 1;
                    if *left == 0 && !targets.contains(&source) {
                        self.cache.remove(&source);
                    }
                }
            }
        }
        computed
    }

    fn stamp(&self, project: &Project, registry: &Registry, ctx: EvalContext, id: NodeId) -> u64 {
        let node = project.graph.node(id).expect("in graph");
        let mut h = DefaultHasher::new();
        let kind = registry.get(&node.kind);
        (&node.kind, node.kind_version, kind.map(|kind| kind.version), ctx).hash(&mut h);
        // Debug output is canonical here: `ParamMap` is ordered by key.
        format!("{:?}", project.effective_params(id)).hash(&mut h);
        for spec in kind.map_or(&[][..], |kind| kind.inputs) {
            spec.name.hash(&mut h);
            if let Some(source) = project.graph.source(&Port(id, spec.name.into())) {
                (source, self.cache.get(&source.0).map(|entry| entry.stamp)).hash(&mut h);
            }
        }
        h.finish()
    }

    fn compute(
        &self,
        project: &Project,
        registry: &Registry,
        ctx: &EvalContext,
        id: NodeId,
    ) -> NodeResult {
        let (kind, params, inputs) = self.prepare(project, registry, id)?;
        (kind.eval)(Params(&params), &inputs, ctx).map_err(NodeError::Failed)
    }

    /// Resolves what evaluating or acting on node `id` needs.
    fn prepare(
        &self,
        project: &Project,
        registry: &Registry,
        id: NodeId,
    ) -> Result<(&'static NodeKind, ParamMap, Vec<Value>), NodeError> {
        let node = project.graph.node(id).expect("in graph");
        let kind =
            registry.get(&node.kind).ok_or_else(|| NodeError::UnknownKind(node.kind.clone()))?;
        if node.kind_version > kind.version {
            return Err(NodeError::NewerVersion {
                kind: node.kind.clone(),
                saved: node.kind_version,
                known: kind.version,
            });
        }
        let params = project.effective_params(id)?;
        let inputs = kind
            .inputs
            .iter()
            .map(|spec| {
                let source = project
                    .graph
                    .source(&Port(id, spec.name.into()))
                    .ok_or(NodeError::MissingInput(spec.name))?;
                let Ok(evaluated) = &self.cache[&source.0].result else {
                    return Err(NodeError::Upstream(source.0));
                };
                let source_kind = registry
                    .get(&project.graph.node(source.0).expect("in graph").kind)
                    .expect("evaluated");
                Ok(evaluated.outputs[source_kind.output_index(&source.1).expect("validated edge")]
                    .clone())
            })
            .collect::<Result<_, _>>()?;
        Ok((kind, params, inputs))
    }
}

fn sources(project: &Project, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    project.graph.edges().filter(move |(_, input)| input.0 == id).map(|(output, _)| output.0)
}

/// Runs action `name` of node `id`, evaluating its inputs at full resolution
/// without touching any interactive cache.
pub fn run_action(
    project: &Project,
    registry: &Registry,
    id: NodeId,
    name: &str,
) -> Result<(), NodeError> {
    let kind = &project.graph.node(id).expect("in graph").kind;
    let action = registry.get(kind).and_then(|kind| kind.action(name));
    let action = action.ok_or_else(|| NodeError::UnknownAction(name.into()))?;
    let mut evaluator = Evaluator::default();
    let targets: Vec<_> = sources(project, id).collect();
    evaluator.run(project, registry, EvalContext::FULL, &targets, true);
    let (_, params, inputs) = evaluator.prepare(project, registry, id)?;
    (action.run)(Params(&params), &inputs).map_err(NodeError::Failed)
}
