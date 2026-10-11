//! Evaluate the requested dependency subgraph. Materializations live for one run.
use crate::{
    Error, Result,
    definition::{Kind, Outcome, Typed},
    graph::Dag,
    payload::Payload,
    ports::*,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
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

#[derive(Default, Debug)]
pub struct Statistics {
    pub nodes: usize,
    /// Port materializations from CPU to device; excludes algorithm-local constants.
    pub uploads: usize,
    /// Port materializations from device to CPU.
    pub downloads: usize,
}
pub struct Evaluation<P: Port> {
    pub desc: <P::Payload as Payload>::Desc,
    pub data: Arc<P::Storage>,
    pub statistics: Statistics,
}
pub struct Evaluator {
    runtime: RuntimeContext,
}
impl Evaluator {
    pub fn new(runtime: RuntimeContext) -> Self {
        Self { runtime }
    }

    /// The compute client that owns this evaluator's device inputs. External
    /// consumers can dispatch presentation kernels against retained inputs.
    pub fn client(&self) -> Result<&cubecl::prelude::Client> {
        self.runtime.client()
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
        self.evaluate_batch(dag, global, targets, targets)
    }

    /// Evaluate node status, retaining inputs only for the named observers.
    /// Status-only targets do not keep their intermediate buffers alive.
    pub fn evaluate_batch(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        targets: &[NodeId],
        observers: &[NodeId],
    ) -> BTreeMap<NodeId, Result<InputValues>> {
        self.evaluate_batch_with_timings(dag, global, targets, observers, None)
    }

    /// Optionally append host timings for attempted nodes in execution order.
    /// Shared ancestors appear once; nodes skipped after upstream failure do not.
    /// Recording adds no device waits. Existing readback waits are charged to the
    /// consuming node; the terminal device wait is outside these measurements.
    pub fn evaluate_batch_with_timings(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        targets: &[NodeId],
        observers: &[NodeId],
        timings: Option<&mut Vec<NodeTiming>>,
    ) -> BTreeMap<NodeId, Result<InputValues>> {
        if let Err(error) = global.validate() {
            return targets.iter().chain(observers).map(|&id| (id, Err(error.clone()))).collect();
        }
        let requests: BTreeMap<_, _> = targets
            .iter()
            .chain(observers)
            .map(|&id| {
                (
                    id,
                    input_request(dag, id).map(|mut request| {
                        if !observers.contains(&id) {
                            request.inputs.clear();
                        }
                        request
                    }),
                )
            })
            .collect();
        let needed: HashSet<_> = requests
            .values()
            .filter_map(|r| r.as_ref().ok())
            .flat_map(|r| r.needed.iter().copied())
            .collect();
        let mut run = Run::new(dag, &self.runtime, global, &needed);
        run.timings = timings;
        for request in requests.values().filter_map(|r| r.as_ref().ok()) {
            for (from, _) in &request.inputs {
                if let Some(from) = from {
                    run.retain(*from);
                }
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

    /// P selects the endpoint representation; node placements come from signatures.
    pub fn evaluate<P: Port>(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        target: Output<P::Payload>,
    ) -> Result<Evaluation<P>> {
        dag.output_of::<P::Payload>(target.id)?;
        global.validate()?;
        let needed = required_nodes(dag, target.node())?;
        let mut run = Run::new(dag, &self.runtime, global, &needed);
        run.retain(target.id);
        for &index in dag.order.iter().filter(|i| needed.contains(i)) {
            run.execute(dag.entries[index].as_ref().unwrap())?;
        }
        let (desc, data) = run.materialize(target.id, &PortSpec::of::<P, Any>("result"))?;
        // One terminal wait; host consumers have already waited at read boundaries.
        self.runtime.finish()?;
        Ok(Evaluation {
            desc: downcast::<<P::Payload as Payload>::Desc>(&desc).clone(),
            data: data
                .downcast()
                .map_err(|_| Error::Runtime("result representation mismatch".into()))?,
            statistics: run.statistics,
        })
    }
}

type DescribedValue = (Description, Data);

/// One logical output and its at-most-one transferred representation.
struct Value {
    desc: Description,
    /// The producer's transfers when it is a device port.
    transfers: Option<Transfers>,
    native: Data,
    transferred: Option<Data>,
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
        let e = dag.entry(id)?;
        for (from, port) in e.inputs.iter().zip(e.node.kind().inputs) {
            if from.is_none() && port.optional {
                continue;
            }
            let from =
                from.ok_or(Error::MissingInput { node: e.node.kind().id, port: port.name })?;
            stack.push(from.node);
        }
    }
    Ok(needed)
}

/// Per-evaluation ownership; never retained in Evaluator or Dag.
struct Run<'a, 't> {
    dag: &'a Dag,
    runtime: &'a RuntimeContext,
    global: &'a GlobalContext,
    values: HashMap<PortId, Value>,
    uses: HashMap<PortId, usize>,
    statistics: Statistics,
    timings: Option<&'t mut Vec<NodeTiming>>,
}
impl<'a, 't> Run<'a, 't> {
    fn new(
        dag: &'a Dag,
        runtime: &'a RuntimeContext,
        global: &'a GlobalContext,
        needed: &HashSet<usize>,
    ) -> Self {
        let mut run = Self {
            dag,
            runtime,
            global,
            values: HashMap::new(),
            uses: HashMap::new(),
            statistics: Statistics::default(),
            timings: None,
        };
        for &index in needed {
            for from in dag.entries[index].as_ref().unwrap().inputs.iter().flatten() {
                run.retain(*from);
            }
        }
        run
    }

    fn retain(&mut self, port: PortId) {
        *self.uses.entry(port).or_default() += 1;
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
            request
                .inputs
                .iter()
                .map(|(from, spec)| from.map(|from| self.materialize(from, spec)).transpose())
                .collect::<Result<Vec<_>>>()
                .map(|values| InputValues { kind, values })
        };
        for (from, _) in request.inputs {
            if let Some(from) = from {
                self.release(from);
            }
        }
        result
    }

    fn materialize(&mut self, from: PortId, spec: &PortSpec) -> Result<DescribedValue> {
        let value = self
            .values
            .get_mut(&from)
            .ok_or_else(|| Error::Contract("requested source value is unavailable".into()))?;
        // The device side of the edge supplies the transfer.
        let transfer = match (value.transfers, spec.transfers) {
            (None, None) | (Some(_), Some(_)) => {
                return Ok((value.desc.clone(), value.native.clone()));
            }
            (None, Some(consumer)) => consumer.upload,
            (Some(producer), None) => producer.download,
        };
        let data = match &value.transferred {
            Some(data) => data.clone(),
            None => {
                let data = transfer(&value.native, &value.desc, self.runtime)?;
                match spec.placement() {
                    Placement::Cpu => self.statistics.downloads += 1,
                    Placement::Device => self.statistics.uploads += 1,
                }
                value.transferred.insert(data).clone()
            }
        };
        Ok((value.desc.clone(), data))
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
            .zip(e.node.kind().inputs)
            .map(|(from, spec)| from.map(|from| self.materialize(from, spec)).transpose())
            .collect::<Result<Vec<_>>>()?;
        let descs: Vec<_> = inputs.iter().map(|v| v.as_ref().map(|(d, _)| d.clone())).collect();
        let outputs = match e.node.contract(self.global, &descs)? {
            Outcome::Known(outputs) => outputs,
            Outcome::Unknown(reason) => return Err(reason),
        };
        let specs = e.node.kind().outputs;
        if outputs.len() != specs.len() {
            return Err(Error::Runtime("node contract output arity mismatch".into()));
        }
        let context = KernelContext { runtime: self.runtime, global: self.global };
        let result = e.node.run(&context, &inputs, &outputs)?;
        if result.len() != specs.len() {
            return Err(Error::Runtime("node result arity mismatch".into()));
        }
        self.statistics.nodes += 1;
        for (port, ((data, desc), spec)) in result.into_iter().zip(outputs).zip(specs).enumerate() {
            let id = output_id(e.id, port);
            if self.uses.contains_key(&id) {
                let value =
                    Value { desc, transfers: spec.transfers, native: data, transferred: None };
                self.values.insert(id, value);
            }
        }
        Ok(())
    }

    fn release_inputs(&mut self, e: &crate::graph::Entry) {
        for from in e.inputs.iter().flatten() {
            self.release(*from);
        }
    }

    fn release(&mut self, port: PortId) {
        let remaining = self.uses.get_mut(&port).unwrap();
        *remaining -= 1;
        if *remaining == 0 {
            self.values.remove(&port);
        }
    }
}

/// A node's evaluated inputs, in its input order and declared placements.
pub struct InputValues {
    kind: &'static Kind,
    values: Vec<Option<DescribedValue>>,
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
        // Status-only batch targets do not retain their inputs.
        if self.values.len() != N::KIND.inputs.len() {
            return Err(Error::Contract(format!("inputs of {} were not retained", N::KIND.id)));
        }
        N::bind(&self.values)
    }
}
struct InputRequest {
    inputs: Vec<(Option<PortId>, PortSpec)>,
    needed: HashSet<usize>,
}
fn input_request(dag: &Dag, id: NodeId) -> Result<InputRequest> {
    let entry = dag.entry(id)?;
    let inputs: Vec<_> =
        entry.inputs.iter().copied().zip(entry.node.kind().inputs.iter().copied()).collect();
    let needed = required_nodes(dag, id)?;
    Ok(InputRequest { inputs, needed })
}
