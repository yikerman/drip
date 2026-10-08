//! Request-driven evaluation with transient, shared placement adaptation.
//!
//! A request executes each needed node once. It retains requested outputs and
//! consumer inputs, never a cross-request node cache. Each output port owns its
//! temporary CPU/GPU representations until the last requesting edge consumes it.
//! GPU completion/lifetime belongs to the compute queue, not to graph topology.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::compute::Compute;
use crate::graph::{Graph, NodeId, Port};
use crate::node::{EvalContext, KernelError, NodeDeclaration, NodeKind, TypedNode, check_inputs};
use crate::param::{ParamMap, Parameters, Params};
use crate::ports::InputTuple;
use crate::resource::Resources;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NodeError {
    #[error("input `{0}` is not connected")]
    MissingInput(&'static str),
    #[error("upstream node {0:?} failed")]
    Upstream(NodeId),
    #[error("{0}")]
    Incomplete(&'static str),
    #[error("node {node:?} ({kind}): {source}")]
    AtNode { node: NodeId, kind: &'static str, source: Box<NodeError> },
    #[error("no action `{0}`")]
    UnknownAction(String),
    #[error("{0}")]
    Failed(String),
    #[error("input `{input}`: {mismatch}")]
    Contract { input: &'static str, mismatch: crate::ports::TypeMismatch },
}
impl NodeError {
    fn from_kernel(kind: &NodeKind, error: KernelError) -> Self {
        match error {
            KernelError::Incomplete(message) => Self::Incomplete(message),
            KernelError::Failed(message) => Self::Failed(message),
            KernelError::Contract { index, mismatch } => {
                Self::Contract { input: kind.inputs().nth(index).map_or("?", |p| p.name), mismatch }
            }
        }
    }
}
pub type NodeResult = Result<Vec<Value>, NodeError>;

/// Outputs and external consumers are distinct requests. An input request does
/// not execute the consumer's kernel or action; it supplies its declared inputs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Request {
    Output(Port),
    Inputs(NodeId),
}
impl Request {
    fn node(&self) -> NodeId {
        match self {
            Self::Output(port) => port.0,
            Self::Inputs(id) => *id,
        }
    }
}

struct Prepared {
    kind: &'static NodeKind,
    params: ParamMap,
    values: Result<Vec<Option<Value>>, NodeError>,
}

/// A completed scheduling pass. GPU work may still be queued; reading a CPU
/// consumer has already performed its necessary synchronization. Keeping this
/// object retains only requested values, not every intermediate in the graph.
pub struct Evaluation {
    resources: Resources,
    level: u8,
    outputs: BTreeMap<Port, Value>,
    consumers: BTreeMap<NodeId, Prepared>,
    results: BTreeMap<NodeId, NodeResult>,
    // Request validity is independent of whether a node computes successfully.
    // A misspelled output must not poison another requested branch through it.
    request_errors: BTreeMap<Request, NodeError>,
    pub executed: Vec<NodeId>,
    /// Placement conversions, excluding small kernel coefficient uploads.
    pub transfers: usize,
}
impl Evaluation {
    fn new(resources: Resources, level: u8) -> Self {
        Self {
            resources,
            level,
            outputs: BTreeMap::new(),
            consumers: BTreeMap::new(),
            results: BTreeMap::new(),
            request_errors: BTreeMap::new(),
            executed: Vec::new(),
            transfers: 0,
        }
    }
    pub fn output(&self, port: &Port) -> Result<&Value, NodeError> {
        if let Some(error) = self.request_errors.get(&Request::Output(port.clone())) {
            return Err(error.clone());
        }
        if let Some(Err(error)) = self.results.get(&port.0) {
            return Err(error.clone());
        }
        self.outputs
            .get(port)
            .ok_or_else(|| NodeError::Failed(format!("output {port:?} was not requested")))
    }
    /// Computational results, independent of invalid requests on other ports.
    pub fn result(&self, id: NodeId) -> Option<&NodeResult> {
        self.results.get(&id)
    }
    /// Invalid target/port requests, retained separately from kernel failures.
    pub fn request_errors(&self) -> impl Iterator<Item = (&Request, &NodeError)> {
        self.request_errors.iter()
    }
    pub fn failures(&self) -> impl Iterator<Item = (NodeId, &NodeError)> {
        self.results
            .iter()
            .filter_map(|(&id, result)| {
                result
                    .as_ref()
                    .err()
                    .filter(|e| !matches!(e, NodeError::Upstream(_)))
                    .map(|e| (id, e))
            })
            .chain(self.request_errors.iter().map(|(request, error)| (request.node(), error)))
    }
    /// Borrow exactly the declared inputs from this immutable request snapshot.
    /// No graph is consulted and no computation is repeated during presentation.
    pub fn with_inputs<N: NodeDeclaration, R>(
        &self,
        id: NodeId,
        declaration: &TypedNode<N>,
        consume: impl FnOnce(
            N::Parameters,
            <N::Inputs as InputTuple>::Borrowed<'_>,
            &EvalContext<'_>,
        ) -> Result<R, KernelError>,
    ) -> Result<R, NodeError> {
        if let Some(error) = self.request_errors.get(&Request::Inputs(id)) {
            return Err(error.clone());
        }
        let prepared = self
            .consumers
            .get(&id)
            .ok_or_else(|| NodeError::Failed("consumer inputs were not requested".into()))?;
        if !std::ptr::eq(prepared.kind, declaration.kind()) {
            return Err(NodeError::Failed("consumer bound to a different node declaration".into()));
        }
        let inputs = prepared.values.as_ref().map_err(Clone::clone)?;
        let ctx = EvalContext::new(self.level, &self.resources)
            .map_err(|e| NodeError::from_kernel(prepared.kind, e))?;
        let params = Params::validated(&prepared.params);
        check_inputs::<N>(params, inputs, &ctx)
            .map_err(|e| NodeError::from_kernel(prepared.kind, e))?;
        consume(N::Parameters::read(params), N::Inputs::read(inputs), &ctx)
            .map_err(|e| NodeError::from_kernel(prepared.kind, e))
    }
}

/// Reusable resources and the latest requested result. No computed image is
/// reused by a later evaluation. Forks share decoded sources and the device only.
#[derive(Default)]
pub struct Evaluator {
    resources: Resources,
    last: Option<Evaluation>,
}
impl Evaluator {
    pub fn with_compute(compute: Arc<Compute>) -> Self {
        Self { resources: Resources::with_compute(compute), ..Self::default() }
    }
    pub fn fork(&self) -> Self {
        Self { resources: self.resources.clone(), last: None }
    }
    pub fn resources(&self) -> &Resources {
        &self.resources
    }
    /// Evaluate an explicit batch. The caller owns the requested values and can
    /// prepare several frontend consumers without recomputing shared ancestors.
    pub fn request(&self, graph: &Graph, level: u8, requests: &[Request]) -> Evaluation {
        execute(graph, level, requests, self.resources.clone())
    }
    /// Convenience for requesting every output of a node, or its inputs when it
    /// is an outputless consumer. Previous requested values are released first.
    pub fn evaluate(&mut self, graph: &Graph, level: u8, targets: &[NodeId]) -> Vec<NodeId> {
        self.last = None;
        let mut requests = Vec::new();
        for &id in targets {
            match graph.node(id) {
                Some(node) if node.kind.outputs().len() > 0 => {
                    requests.extend(
                        node.kind.outputs().map(|p| Request::Output(Port(id, p.name.into()))),
                    );
                }
                _ => requests.push(Request::Inputs(id)),
            }
        }
        let report = self.request(graph, level, &requests);
        let executed = report.executed.clone();
        self.last = Some(report);
        executed
    }
    pub fn result(&self, id: NodeId) -> Option<&NodeResult> {
        self.last.as_ref()?.result(id)
    }
    pub fn evaluation(&self) -> Option<&Evaluation> {
        self.last.as_ref()
    }
    pub fn failures(&self) -> impl Iterator<Item = (NodeId, &NodeError)> {
        self.last.iter().flat_map(Evaluation::failures)
    }
    /// Standalone typed consumer convenience. Batch consumers should instead use
    /// `request` followed by `Evaluation::with_inputs` on the same snapshot.
    pub fn with_inputs<N: NodeDeclaration, R>(
        &mut self,
        graph: &Graph,
        id: NodeId,
        level: u8,
        declaration: &TypedNode<N>,
        consume: impl FnOnce(
            N::Parameters,
            <N::Inputs as InputTuple>::Borrowed<'_>,
            &EvalContext<'_>,
        ) -> Result<R, KernelError>,
    ) -> Result<R, NodeError> {
        self.last = None;
        let report = self.request(graph, level, &[Request::Inputs(id)]);
        report.with_inputs(id, declaration, consume)
    }
    /// External effects are explicit and run at full resolution. All upstream
    /// kernels remain pure; their transient results retire in the same way as previews.
    pub fn run_action(mut self, graph: &Graph, id: NodeId, name: &str) -> Result<(), NodeError> {
        // An action owns this evaluator; previous preview outputs are not inputs
        // to the action and must not occupy its full-resolution working budget.
        self.last = None;
        let node = graph.node(id).ok_or_else(|| NodeError::Failed("node does not exist".into()))?;
        let action = node.kind.action(name).ok_or_else(|| NodeError::UnknownAction(name.into()))?;
        let report = self.request(graph, 0, &[Request::Inputs(id)]);
        if let Some((failed, error)) = report.failures().find(|(failed, _)| *failed != id) {
            return Err(NodeError::AtNode {
                node: failed,
                kind: graph.node(failed).map_or("unknown", |n| n.kind.id),
                source: Box::new(error.clone()),
            });
        }
        let prepared = report
            .consumers
            .get(&id)
            .ok_or_else(|| NodeError::Failed("missing action inputs".into()))?;
        let inputs = prepared.values.as_ref().map_err(Clone::clone)?;
        let ctx = EvalContext::new(0, &self.resources)
            .map_err(|e| NodeError::from_kernel(node.kind, e))?;
        action
            .run(Params::validated(&prepared.params), inputs, &ctx)
            .map_err(|e| NodeError::from_kernel(node.kind, e))
    }
}

struct Representations {
    original: Value,
    adapted: HashMap<TypeId, Value>,
}

fn execute(graph: &Graph, level: u8, requests: &[Request], resources: Resources) -> Evaluation {
    let mut report = Evaluation::new(resources.clone(), level);
    let mut output_ports = BTreeSet::new();
    let mut consumers = BTreeSet::new();
    let mut roots = BTreeSet::new();
    let mut execution_roots = BTreeSet::new();
    for request in requests {
        let id = request.node();
        let Some(node) = graph.node(id) else {
            report
                .request_errors
                .insert(request.clone(), NodeError::Failed(format!("no node {id:?}")));
            continue;
        };
        match request {
            Request::Output(port) => {
                if node.kind.output_index(&port.1).is_none() {
                    report.request_errors.insert(
                        request.clone(),
                        NodeError::Failed(format!("unknown output {port:?}")),
                    );
                    continue;
                }
                output_ports.insert(port.clone());
                execution_roots.insert(id);
            }
            Request::Inputs(_) => {
                consumers.insert(id);
                execution_roots.extend(sources(graph, id).map(|p| p.0));
            }
        }
        roots.insert(id);
    }
    let ctx = match EvalContext::new(level, &resources) {
        Ok(ctx) => ctx,
        Err(error) => {
            for id in roots {
                report.results.insert(id, Err(NodeError::Failed(error.to_string())));
            }
            return report;
        }
    };
    let order = graph.upstream_order(&roots.into_iter().collect::<Vec<_>>());
    let execute: BTreeSet<_> = graph
        .upstream_order(&execution_roots.into_iter().collect::<Vec<_>>())
        .into_iter()
        .collect();
    let mut remaining: HashMap<Port, usize> = HashMap::new();
    for &id in &order {
        for source in sources(graph, id) {
            *remaining.entry(source.clone()).or_default() += 1;
        }
    }
    let mut live: HashMap<Port, Representations> = HashMap::new();
    for id in order {
        let node = graph.node(id).expect("validated request ancestors");
        let inputs =
            collect_inputs(graph, id, &ctx, &mut live, &report.results, &mut report.transfers);
        if consumers.contains(&id) {
            report.consumers.insert(
                id,
                Prepared { kind: node.kind, params: node.params.clone(), values: inputs.clone() },
            );
        }
        let result = match &inputs {
            Err(error) => Err(error.clone()),
            Ok(inputs) if execute.contains(&id) => {
                report.executed.push(id);
                (node.kind.eval)(Params::validated(&node.params), inputs, &ctx)
                    .map_err(|error| NodeError::from_kernel(node.kind, error))
            }
            Ok(_) => Ok(Vec::new()),
        };
        // Drop this call's input handles before retiring exhausted source ports.
        drop(inputs);
        for source in sources(graph, id) {
            let count = remaining.get_mut(source).expect("counted input edge");
            *count -= 1;
            if *count == 0 {
                live.remove(source);
            }
        }
        match result {
            Err(error) => {
                report.results.insert(id, Err(error));
            }
            Ok(values) => {
                if consumers.contains(&id) {
                    report.results.entry(id).or_insert_with(|| Ok(Vec::new()));
                }
                let mut requested = Vec::new();
                for (spec, value) in node.kind.outputs().zip(values) {
                    let port = Port(id, spec.name.into());
                    if output_ports.contains(&port) {
                        report.outputs.insert(port.clone(), value.clone());
                        requested.push(value.clone());
                    }
                    if remaining.get(&port).is_some_and(|&n| n > 0) {
                        live.insert(
                            port,
                            Representations { original: value, adapted: HashMap::new() },
                        );
                    }
                }
                if !requested.is_empty() {
                    report.results.insert(id, Ok(requested));
                }
            }
        }
    }
    report
}

fn collect_inputs(
    graph: &Graph,
    id: NodeId,
    ctx: &EvalContext<'_>,
    live: &mut HashMap<Port, Representations>,
    results: &BTreeMap<NodeId, NodeResult>,
    transfers: &mut usize,
) -> Result<Vec<Option<Value>>, NodeError> {
    let node = graph.node(id).expect("planned node");
    node.kind
        .inputs()
        .map(|spec| {
            let Some(source) = graph.source(&Port(id, spec.name.into())) else {
                return if spec.requirement.optional {
                    Ok(None)
                } else {
                    Err(NodeError::MissingInput(spec.name))
                };
            };
            if results.get(&source.0).is_some_and(Result::is_err) {
                return Err(NodeError::Upstream(source.0));
            }
            let replicas = live
                .get_mut(source)
                .ok_or_else(|| NodeError::Failed(format!("source {source:?} produced no value")))?;
            let descriptor =
                spec.requirement.target(replicas.original.descriptor()).ok_or_else(|| {
                    NodeError::Contract {
                        input: spec.name,
                        mismatch: crate::ports::TypeMismatch {
                            expected: spec.requirement.name(),
                            actual: replicas.original.descriptor().name,
                        },
                    }
                })?;
            if descriptor == *replicas.original.descriptor() {
                return Ok(Some(replicas.original.clone()));
            }
            let key = descriptor.type_id();
            if let Some(value) = replicas.adapted.get(&key) {
                return Ok(Some(value.clone()));
            }
            let compute =
                ctx.compute().map_err(|error| NodeError::from_kernel(node.kind, error))?;
            let value =
                descriptor.materialize(&replicas.original, compute).map_err(NodeError::Failed)?;
            *transfers += 1;
            replicas.adapted.insert(key, value.clone());
            Ok(Some(value))
        })
        .collect()
}
fn sources(graph: &Graph, id: NodeId) -> impl Iterator<Item = &Port> {
    graph.edges().filter(move |(_, input)| input.0 == id).map(|(output, _)| output)
}

pub fn run_action(graph: &Graph, id: NodeId, name: &str) -> Result<(), NodeError> {
    Evaluator::default().run_action(graph, id, name)
}
