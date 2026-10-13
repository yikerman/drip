//! Evaluate the requested dependency subgraph. Values live for one run, in
//! the run's `Store`.
//!
//! Nodes run in graph order. Map, warp and resample nodes publish deferred
//! images. A kernel node, the consumers of `evaluate_inputs` and the target of
//! `evaluate` materialize what they receive, so each segment runs as one fused
//! kernel at its first consumer that needs samples.
use crate::{
    Error, Result,
    definition::{Kind, Typed},
    graph::Dag,
    image::{Image, Raster},
    payload::Payload,
    ports::*,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
    store::Store,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::{Duration, Instant},
};

/// Host wall time around one attempted node, including input transfers, contracts
/// and allocation. Device dispatch is asynchronous; this is not GPU kernel time.
#[derive(Debug)]
pub struct NodeTiming {
    pub node: NodeId,
    pub kind: &'static str,
    pub host_elapsed: Duration,
    pub success: bool,
}

/// Work done by one `evaluate` call, counted on the evaluator's runtime while
/// the call runs. Reading the result's other side afterwards is the caller's
/// transfer.
#[derive(Default, Debug)]
pub struct Statistics {
    pub nodes: usize,
    /// Rasters copied from host to device; excludes constants and scratch.
    pub uploads: usize,
    /// Rasters copied from device to host.
    pub downloads: usize,
    /// Fused kernels launched, one per deferred image materialized, including
    /// the images a plan cuts at. A plan that neither moves nor maps samples
    /// launches none.
    pub fused: usize,
}
/// The target output's raster, where its node produced it. A deferred target
/// is materialized on the device.
pub struct Evaluation<P: Payload> {
    pub desc: P::Desc,
    pub data: Raster<P>,
    pub statistics: Statistics,
}
pub struct Evaluator {
    runtime: RuntimeContext,
}
impl Evaluator {
    pub fn new(runtime: RuntimeContext) -> Self {
        Self { runtime }
    }

    /// The runtime that owns this evaluator's device rasters. External
    /// consumers read retained inputs and dispatch presentation kernels on it.
    pub fn runtime(&self) -> &RuntimeContext {
        &self.runtime
    }

    /// Prepare declared inputs for several external consumers in one run.
    /// Shared ancestors execute once; an incomplete branch does not prevent
    /// unrelated consumers from receiving their inputs. No values survive here.
    pub fn evaluate_inputs(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        targets: &[NodeId],
    ) -> BTreeMap<NodeId, Result<InputValues>> {
        self.evaluate_batch(dag, global, targets, None)
    }

    /// `evaluate_inputs`, recording host timings.
    ///
    /// `timings` receives host timings for attempted nodes in execution order.
    /// Shared ancestors appear once; nodes skipped after upstream failure do not.
    /// Recording adds no device waits. Existing readback waits are charged to the
    /// consuming node; the terminal device wait is outside these measurements.
    pub fn evaluate_batch(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        targets: &[NodeId],
        timings: Option<&mut Vec<NodeTiming>>,
    ) -> BTreeMap<NodeId, Result<InputValues>> {
        if let Err(error) = global.validate() {
            return targets.iter().map(|&id| (id, Err(error.clone()))).collect();
        }
        let requests: BTreeMap<_, _> =
            targets.iter().map(|&id| (id, input_request(dag, id))).collect();
        let needed: HashSet<_> = requests
            .values()
            .filter_map(|r| r.as_ref().ok())
            .flat_map(|r| r.needed.iter().copied())
            .collect();
        let mut run = Run::new(dag, &self.runtime, global, &needed);
        run.timings = timings;
        for request in requests.values().filter_map(|r| r.as_ref().ok()) {
            for &from in request.inputs.iter().flatten() {
                run.store.retain(from);
            }
        }
        let failures = run.execute_batch(&needed);
        let mut results: BTreeMap<_, _> = requests
            .into_iter()
            .map(|(id, request)| {
                let values = request.and_then(|request| run.collect_inputs(id, request, &failures));
                (id, values)
            })
            .collect();
        if let Err(error) = self.runtime.finish() {
            for value in results.values_mut().filter(|v| v.is_ok()) {
                *value = Err(error.clone());
            }
        }
        results
    }

    /// Evaluate `target` and the nodes it depends on.
    pub fn evaluate<P: Payload>(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        target: Output<P>,
    ) -> Result<Evaluation<P>> {
        dag.output_of::<P>(target.id)?;
        global.validate()?;
        let (uploads, downloads) = self.runtime.transfers();
        let fused = self.runtime.fused();
        let needed = required_nodes(dag, target.node())?;
        let mut run = Run::new(dag, &self.runtime, global, &needed);
        run.store.retain(target.id);
        for &index in dag.order.iter().filter(|i| needed.contains(i)) {
            run.execute(dag.entries[index].as_ref().unwrap())?;
        }
        let image = run.value(target.id)?;
        image.materialize(&run.kernels())?;
        let data = image.downcast::<P>().expect("output payload is checked").clone();
        // One terminal wait; host consumers have already waited at read boundaries.
        self.runtime.finish()?;
        let (uploaded, downloaded) = self.runtime.transfers();
        let statistics = Statistics {
            nodes: run.nodes,
            uploads: uploaded - uploads,
            downloads: downloaded - downloads,
            fused: self.runtime.fused() - fused,
        };
        Ok(Evaluation { desc: data.desc().clone(), data, statistics })
    }
}

fn output_id(node: NodeId, index: usize) -> PortId {
    PortId { node, index, direction: Direction::Output }
}
fn required_nodes(dag: &Dag, target: NodeId) -> Result<HashSet<usize>> {
    let mut needed = HashSet::new();
    let mut stack = vec![target];
    while let Some(id) = stack.pop() {
        if !needed.insert(id.index) {
            continue;
        }
        // An unconnected required input is the node's own contract failure.
        stack.extend(dag.entry(id)?.inputs.iter().flatten().map(|from| from.node));
    }
    Ok(needed)
}

/// Per-evaluation ownership; never retained in Evaluator or Dag.
struct Run<'a, 't> {
    dag: &'a Dag,
    runtime: &'a RuntimeContext,
    global: &'a GlobalContext,
    store: Store,
    nodes: usize,
    timings: Option<&'t mut Vec<NodeTiming>>,
}
impl<'a, 't> Run<'a, 't> {
    fn new(
        dag: &'a Dag,
        runtime: &'a RuntimeContext,
        global: &'a GlobalContext,
        needed: &HashSet<usize>,
    ) -> Self {
        let mut run =
            Self { dag, runtime, global, store: Store::default(), nodes: 0, timings: None };
        for &index in needed {
            for from in dag.entries[index].as_ref().unwrap().inputs.iter().flatten() {
                run.store.retain(*from);
            }
        }
        run
    }

    fn execute_batch(&mut self, needed: &HashSet<usize>) -> HashMap<NodeId, Error> {
        let mut failures = HashMap::new();
        for &index in self.dag.order.iter().filter(|i| needed.contains(i)) {
            let entry = self.dag.entries[index].as_ref().unwrap();
            let upstream =
                entry.inputs.iter().flatten().find_map(|p| failures.get(&p.node).cloned());
            let result = if let Some(error) = upstream {
                self.release_inputs(entry);
                Err(error)
            } else {
                self.execute(entry).map_err(|source| Error::Node {
                    node: entry.id,
                    kind: entry.node.kind().id,
                    source: Box::new(source),
                })
            };
            if let Err(error) = result {
                failures.insert(entry.id, error);
            }
        }
        failures
    }

    fn collect_inputs(
        &mut self,
        id: NodeId,
        request: InputRequest,
        failures: &HashMap<NodeId, Error>,
    ) -> Result<InputValues> {
        let result = if let Some(error) = failures.get(&id) {
            Err(error.clone())
        } else {
            let kind = self.dag.entry(id)?.node.kind();
            let kernels = self.kernels();
            let value = |from| {
                let image = self.value(from)?;
                image.materialize(&kernels)?;
                Ok(image.clone())
            };
            request
                .inputs
                .iter()
                .map(|from| from.map(value).transpose())
                .collect::<Result<Vec<_>>>()
                .map(|values| InputValues { kind, values })
        };
        for from in request.inputs.into_iter().flatten() {
            self.store.release(from);
        }
        result
    }

    fn kernels(&self) -> KernelContext<'a> {
        KernelContext { runtime: self.runtime, global: self.global }
    }

    fn value(&self, from: PortId) -> Result<&Image> {
        self.store
            .get(from)
            .ok_or_else(|| Error::Contract("requested source value is unavailable".into()))
    }
    fn execute(&mut self, e: &crate::graph::Entry) -> Result<()> {
        let start = self.timings.as_ref().map(|_| Instant::now());
        let result = self.compute(e);
        self.release_inputs(e);
        if let (Some(start), Some(timings)) = (start, self.timings.as_mut()) {
            timings.push(NodeTiming {
                node: e.id,
                kind: e.node.kind().id,
                host_elapsed: start.elapsed(),
                success: result.is_ok(),
            });
        }
        result
    }

    fn compute(&mut self, e: &crate::graph::Entry) -> Result<()> {
        let inputs = e
            .inputs
            .iter()
            .map(|from| from.map(|from| self.value(from).cloned()).transpose())
            .collect::<Result<Vec<_>>>()?;
        let descs: Vec<_> = inputs.iter().map(|v| v.as_ref().map(|i| i.desc().clone())).collect();
        let outputs = e.node.contract(self.global, &descs)?;
        let specs = e.node.kind().outputs;
        if outputs.len() != specs.len() {
            return Err(Error::Runtime("node contract output arity mismatch".into()));
        }
        let result = e.node.run(&self.kernels(), &inputs, &outputs)?;
        if result.len() != specs.len() {
            return Err(Error::Runtime("node result arity mismatch".into()));
        }
        self.nodes += 1;
        for (port, image) in result.into_iter().enumerate() {
            self.store.insert(output_id(e.id, port), image);
        }
        Ok(())
    }

    fn release_inputs(&mut self, e: &crate::graph::Entry) {
        for from in e.inputs.iter().flatten() {
            self.store.release(*from);
        }
    }
}

/// A node's evaluated inputs, in its input order, materialized.
pub struct InputValues {
    kind: &'static Kind,
    values: Vec<Option<Image>>,
}
impl InputValues {
    /// Typed views as declared by node type `N`; these must be `N`'s inputs.
    pub fn bind<N: Typed>(&self) -> Result<N::Inputs<'_>> {
        if !std::ptr::eq(self.kind, N::KIND) {
            return Err(Error::Contract(format!(
                "inputs of {} cannot bind as {}",
                self.kind.id,
                N::KIND.id
            )));
        }
        N::bind(&self.values)
    }
}
struct InputRequest {
    inputs: Vec<Option<PortId>>,
    needed: HashSet<usize>,
}
fn input_request(dag: &Dag, id: NodeId) -> Result<InputRequest> {
    let inputs = dag.entry(id)?.inputs.clone();
    let needed = required_nodes(dag, id)?;
    Ok(InputRequest { inputs, needed })
}
