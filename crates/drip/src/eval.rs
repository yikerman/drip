//! Evaluate the requested dependency subgraph. Materializations live for one run.
use crate::{
    Error, Result,
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
        let entry = dag.output(target.id)?;
        if entry.node.outputs()[target.id.index].payload != std::any::TypeId::of::<P::Payload>() {
            return Err(wrong_handle());
        }
        let spec = PortSpec::of::<P>("result");
        let (mut values, statistics) =
            self.run(dag, global, target.node(), &[(target.id.index, spec)])?;
        let (desc, data) = values.pop().unwrap();
        Ok(Evaluation {
            desc: downcast::<<P::Payload as Payload>::Desc>(&desc).clone(),
            data: data
                .downcast()
                .map_err(|_| Error::Runtime("result representation mismatch".into()))?,
            statistics,
        })
    }

    /// Demand all native outputs of a node, including an input-only node's checks.
    pub fn evaluate_node(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        target: NodeId,
    ) -> Result<NodeEvaluation> {
        let specs = dag.entry(target)?.node.outputs();
        let endpoints: Vec<_> = specs.iter().cloned().enumerate().collect();
        let (values, statistics) = self.run(dag, global, target, &endpoints)?;
        let outputs = values
            .into_iter()
            .zip(specs)
            .map(|((desc, data), spec)| OutputValue { desc, data, spec })
            .collect();
        Ok(NodeEvaluation { outputs, statistics })
    }

    fn run(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        target: NodeId,
        endpoints: &[(usize, PortSpec)],
    ) -> Result<(Vec<DescribedValue>, Statistics)> {
        global.validate()?;
        let needed = required_nodes(dag, target)?;
        let mut run = Run::new(dag, &self.runtime, global, &needed);
        for (index, _) in endpoints {
            run.retain(output_id(target, *index));
        }
        for &index in dag.order.iter().filter(|i| needed.contains(i)) {
            run.execute(dag.entries[index].as_ref().unwrap())?;
        }
        let result = endpoints
            .iter()
            .map(|(index, spec)| run.materialize(output_id(target, *index), spec))
            .collect::<Result<_>>()?;
        // One terminal wait; host consumers have already waited at read boundaries.
        self.runtime.finish()?;
        Ok((result, run.statistics))
    }
}

type DescribedValue = (Description, Data);

/// One logical output and its at-most-one transferred representation.
struct Value {
    desc: Option<Description>,
    placement: Placement,
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
        for (from, port) in e.inputs.iter().zip(e.node.inputs()) {
            if from.is_none() && port.optional {
                continue;
            }
            let from =
                from.ok_or(Error::MissingInput { node: e.node.metadata().id, port: port.name })?;
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
                    kind: entry.node.metadata().id,
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
            request
                .inputs
                .iter()
                .map(|(from, spec)| {
                    from.map(|from| {
                        let (desc, data) = self.materialize(from, spec)?;
                        Ok(OutputValue { desc, data, spec: spec.clone() })
                    })
                    .transpose()
                })
                .collect::<Result<Vec<_>>>()
                .map(InputValues)
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
            .ok_or_else(|| Error::Contract("requested source description is unavailable".into()))?;
        let desc = value
            .desc
            .as_ref()
            .ok_or_else(|| Error::Contract("requested source description is unavailable".into()))?;
        if spec.placement == value.placement {
            return Ok((desc.clone(), value.native.clone()));
        }
        if value.transferred.is_none() {
            let data = (spec.transport)(&value.native, value.placement, desc, self.runtime)?;
            match spec.placement {
                Placement::Cpu => self.statistics.downloads += 1,
                Placement::Device => self.statistics.uploads += 1,
            }
            value.transferred = Some(data);
        }
        Ok((desc.clone(), value.transferred.as_ref().unwrap().clone()))
    }
    fn execute(&mut self, e: &crate::graph::Entry) -> Result<()> {
        let start = self.timings.as_ref().map(|_| Instant::now());
        let result = self.compute(e);
        self.release_inputs(e);
        if let (Some(start), Some(timings)) = (start, self.timings.as_mut()) {
            timings.push(NodeTiming {
                node: e.id,
                kind: e.node.metadata().id,
                host_elapsed: start.elapsed(),
                success: result.is_ok(),
            });
        }
        result
    }

    fn compute(&mut self, e: &crate::graph::Entry) -> Result<()> {
        let mut inputs = Vec::new();
        let mut descs = Vec::new();
        for (from, spec) in e.inputs.iter().zip(e.node.inputs()) {
            match from {
                Some(from) => {
                    let (desc, data) = self.materialize(*from, &spec)?;
                    descs.push(Some(desc));
                    inputs.push(Some(data));
                }
                None => {
                    descs.push(None);
                    inputs.push(None);
                }
            }
        }
        let output_descs = e.node.contract(self.global, &descs)?;
        let specs = e.node.outputs();
        if output_descs.len() != specs.len() {
            return Err(Error::Runtime("node contract output arity mismatch".into()));
        }
        let context = KernelContext { runtime: self.runtime, global: self.global };
        let result = e.node.run(&context, &descs, &inputs, &output_descs)?;
        if result.len() != specs.len() {
            return Err(Error::Runtime("node result arity mismatch".into()));
        }
        self.statistics.nodes += 1;
        for (port, (data, spec)) in result.into_iter().zip(specs).enumerate() {
            let id = output_id(e.id, port);
            if self.uses.contains_key(&id) {
                match data {
                    Some(data) => {
                        self.values.insert(
                            id,
                            Value {
                                desc: output_descs[port].clone(),
                                placement: spec.placement,
                                native: data,
                                transferred: None,
                            },
                        );
                    }
                    None if output_descs[port].is_none() => {}
                    None => return Err(Error::Runtime("node omitted a described output".into())),
                }
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

pub struct NodeEvaluation {
    pub outputs: Vec<OutputValue>,
    pub statistics: Statistics,
}
pub struct OutputValue {
    desc: Description,
    data: Data,
    spec: PortSpec,
}
impl OutputValue {
    pub fn shared<P: Port>(&self) -> Result<Arc<P::Storage>> {
        self.get::<P>()?;
        self.data.clone().downcast().map_err(|_| Error::Runtime("representation mismatch".into()))
    }
    pub fn spec(&self) -> &PortSpec {
        &self.spec
    }
    pub fn get<P: Port>(&self) -> Result<(&<P::Payload as Payload>::Desc, &P::Storage)> {
        if self.spec.payload != std::any::TypeId::of::<P::Payload>()
            || self.spec.placement != P::PLACEMENT
        {
            return Err(Error::Contract("output payload or placement mismatch".into()));
        }
        Ok((downcast(&self.desc), downcast(&self.data)))
    }
}

pub struct InputValues(Vec<Option<OutputValue>>);
impl InputValues {
    pub fn get(&self, name: &str) -> Option<&OutputValue> {
        self.0.iter().flatten().find(|v| v.spec.name == name)
    }
}
struct InputRequest {
    inputs: Vec<(Option<PortId>, PortSpec)>,
    needed: HashSet<usize>,
}
fn input_request(dag: &Dag, id: NodeId) -> Result<InputRequest> {
    let entry = dag.entry(id)?;
    let inputs: Vec<_> = entry.inputs.iter().copied().zip(entry.node.inputs()).collect();
    let needed = required_nodes(dag, id)?;
    Ok(InputRequest { inputs, needed })
}
