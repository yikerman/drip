//! Pull-based evaluation (DESIGN E1-E3, E6, U2). Frontends request target
//! nodes; only those and their ancestors are evaluated, in dependency order.
//! A node is recomputed only when its dependency stamp changes: a hash of
//! everything its result depends on, including the stamps of its sources and
//! the revisions of the files it reads.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::graph::{NodeId, Port};
use crate::node::{EvalContext, Evaluated, NodeKind, Registry};
use crate::param::{ParamKind, ParamMap, Params};
use crate::project::Project;
use crate::resource::Resources;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NodeError {
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
    /// Brings `targets` up to date and returns the nodes actually recomputed.
    pub fn evaluate(
        &mut self,
        project: &Project,
        registry: &Registry,
        level: u8,
        targets: &[NodeId],
    ) -> Vec<NodeId> {
        self.cache.0.retain(|id, _| project.graph.node(*id).is_some());
        self.run(project, registry, level, targets, false)
    }

    pub fn result(&self, id: NodeId) -> Option<&NodeResult> {
        self.cache.0.get(&id).map(|entry| &entry.result)
    }

    /// Forgets what was loaded from `path`; results depending on it are
    /// recomputed on the next evaluation.
    pub fn reload(&mut self, path: &std::path::Path) {
        self.resources.reload(path);
    }

    /// With `release`, a result is dropped once its last consumer in this run
    /// has been computed (unless it is a target), bounding memory for one-off
    /// full-resolution runs.
    fn run(
        &mut self,
        project: &Project,
        registry: &Registry,
        level: u8,
        targets: &[NodeId],
        release: bool,
    ) -> Vec<NodeId> {
        let ctx = EvalContext { level, resources: &self.resources };
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
            let stamp = self.cache.stamp(project, registry, &ctx, id);
            if self.cache.0.get(&id).is_none_or(|entry| entry.stamp != stamp) {
                let start = std::time::Instant::now();
                let result = self.cache.compute(project, registry, &ctx, id);
                let kind = &graph.node(id).expect("in graph").kind;
                log::debug!("evaluated {kind} {id:?} at level {level} in {:.1?}", start.elapsed());
                self.cache.0.insert(id, Entry { stamp, result });
                computed.push(id);
            }
            if release {
                for source in sources(project, id) {
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
    fn stamp(&self, project: &Project, registry: &Registry, ctx: &EvalContext, id: NodeId) -> u64 {
        let node = project.graph.node(id).expect("in graph");
        let kind = registry.get(&node.kind).expect("graphs hold only registered kinds");
        let mut h = DefaultHasher::new();
        (&node.kind, ctx.level).hash(&mut h);
        let params = project.effective_params(id);
        // Debug output is canonical here: `ParamMap` is ordered by key.
        format!("{params:?}").hash(&mut h);
        if let Ok(params) = &params {
            let paths =
                kind.params.iter().filter(|spec| matches!(spec.kind, ParamKind::Path { .. }));
            for spec in paths {
                Params(params)
                    .path(spec.name)
                    .map(|path| ctx.resources.revision(path))
                    .hash(&mut h);
            }
        }
        for spec in kind.inputs {
            spec.name.hash(&mut h);
            if let Some(source) = project.graph.source(&Port(id, spec.name.into())) {
                (source, self.0.get(&source.0).map(|entry| entry.stamp)).hash(&mut h);
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
        let kind = registry.get(&node.kind).expect("graphs hold only registered kinds");
        let params = project.effective_params(id)?;
        let inputs = kind
            .inputs
            .iter()
            .map(|spec| {
                let source = project
                    .graph
                    .source(&Port(id, spec.name.into()))
                    .ok_or(NodeError::MissingInput(spec.name))?;
                let Ok(evaluated) = &self.0[&source.0].result else {
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
    evaluator.run(project, registry, 0, &targets, true);
    let (_, params, inputs) = evaluator.cache.prepare(project, registry, id)?;
    (action.run)(Params(&params), &inputs).map_err(NodeError::Failed)
}
