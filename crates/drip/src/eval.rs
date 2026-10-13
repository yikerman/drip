//! Evaluate the requested dependency subgraph, reusing what the store keeps.
//!
//! A call keys every output it may need, from the sources down, then walks
//! back from the demanded values: a value the evaluator's `Store` holds is a
//! hit, and a missing one schedules its node and what that node reads. The
//! scheduled nodes run in graph order. Map, warp and resample nodes publish
//! deferred images. A kernel node, the consumers of `evaluate_inputs` and the
//! target of `evaluate` materialize what they receive, so each segment runs as
//! one fused kernel at its first consumer that needs samples. A value enters
//! the store when the run stops reading it, if it was materialized by then.
use crate::{
    Error, Result,
    definition::{Kind, Typed},
    graph::{Dag, Entry},
    image::{Image, Raster},
    payload::Payload,
    ports::*,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
    store::{Key, Store},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Mutex, MutexGuard, PoisonError},
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
    /// Nodes run. A node runs when a value it produces is demanded and not
    /// stored.
    pub nodes: usize,
    /// Rasters copied from host to device; excludes constants and scratch.
    pub uploads: usize,
    /// Rasters copied from device to host.
    pub downloads: usize,
    /// Fused kernels launched, one per deferred image materialized, including
    /// the images a plan cuts at. A plan that neither moves nor maps samples
    /// launches none.
    pub fused: usize,
    /// Demanded values the store served.
    pub hits: usize,
    /// Representations the store dropped to stay within its budget.
    pub evictions: usize,
}
/// The target output's raster, where its node produced it. A deferred target
/// is materialized on the device.
pub struct Evaluation<P: Payload> {
    pub desc: P::Desc,
    pub data: Raster<P>,
    pub statistics: Statistics,
}
/// Evaluates graphs on one runtime and keeps their materialized values in a
/// store until the budget evicts them or the evaluator is dropped. Calls on
/// one evaluator share the store and run one at a time.
pub struct Evaluator {
    runtime: RuntimeContext,
    store: Mutex<Store>,
}
impl Evaluator {
    /// An evaluator with an empty store under `runtime`'s budget.
    pub fn new(runtime: RuntimeContext) -> Self {
        let store = Mutex::new(Store::new(runtime.budget()));
        Self { runtime, store }
    }

    /// The runtime that owns this evaluator's device rasters. External
    /// consumers read retained inputs and dispatch presentation kernels on it.
    pub fn runtime(&self) -> &RuntimeContext {
        &self.runtime
    }

    /// Prepare declared inputs for several external consumers in one run.
    /// Shared ancestors execute once; an incomplete branch does not prevent
    /// unrelated consumers from receiving their inputs. Each target node runs,
    /// so its contract checks the inputs it receives.
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
    /// Shared ancestors appear once. Nodes skipped after upstream failure, and
    /// nodes whose values the store held, do not appear. Recording adds no
    /// device waits. Existing readback waits are charged to the consuming
    /// node, and the terminal device wait is outside these measurements.
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
        let mut run = Run::new(self, dag, global, &needed);
        run.timings = timings;
        for (&id, request) in requests.iter().filter_map(|(id, r)| Some((id, r.as_ref().ok()?))) {
            run.schedule(id);
            for &from in request.inputs.iter().flatten() {
                run.demand(from);
            }
        }
        let failures = run.execute_batch();
        let mut results: BTreeMap<_, _> = requests
            .into_iter()
            .map(|(id, request)| {
                let values = request.and_then(|request| run.collect_inputs(id, request, &failures));
                (id, values)
            })
            .collect();
        run.finish();
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
        let mut run = Run::new(self, dag, global, &needed);
        run.demand(target.id);
        for index in &dag.order {
            if run.running.contains(index) {
                run.execute(dag.entries[*index].as_ref().unwrap())?;
            }
        }
        let image = run.value(target.id)?.clone();
        image.materialize(&run.kernels())?;
        run.release(target.id);
        let (nodes, hits, evictions) = run.finish();
        let data = image.downcast::<P>().expect("output payload is checked").clone();
        // One terminal wait; host consumers have already waited at read boundaries.
        self.runtime.finish()?;
        let (uploaded, downloaded) = self.runtime.transfers();
        let statistics = Statistics {
            nodes,
            uploads: uploaded - uploads,
            downloads: downloaded - downloads,
            fused: self.runtime.fused() - fused,
            hits,
            evictions,
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

/// One call's evaluation: the keys of the needed nodes, the nodes it runs and
/// the values on its live edges. It holds the evaluator's store for its
/// duration.
struct Run<'a, 't> {
    dag: &'a Dag,
    runtime: &'a RuntimeContext,
    global: &'a GlobalContext,
    store: MutexGuard<'a, Store>,
    /// Per needed node, the key its output keys derive from, or `None` when
    /// the node or an input cannot be keyed.
    keys: HashMap<NodeId, Option<Key>>,
    /// Values of demanded ports, with the consumers still to read each.
    live: HashMap<PortId, Live>,
    /// Indices of the nodes this run executes.
    running: HashSet<usize>,
    nodes: usize,
    hits: usize,
    /// The store's eviction count when the run began.
    evictions: usize,
    timings: Option<&'t mut Vec<NodeTiming>>,
}
/// A demanded value, set when its node publishes it or the store serves it.
#[derive(Default)]
struct Live {
    image: Option<Image>,
    uses: usize,
}
impl<'a, 't> Run<'a, 't> {
    /// Key the `needed` nodes in graph order. A source loads here, since its
    /// key includes the files it read.
    fn new(
        evaluator: &'a Evaluator,
        dag: &'a Dag,
        global: &'a GlobalContext,
        needed: &HashSet<usize>,
    ) -> Self {
        let store = evaluator.store.lock().unwrap_or_else(PoisonError::into_inner);
        let evictions = store.evictions();
        let mut keys: HashMap<NodeId, Option<Key>> = HashMap::new();
        for entry in dag.order.iter().filter(|i| needed.contains(i)) {
            let entry = dag.entries[*entry].as_ref().unwrap();
            // A connected input without a key leaves the node without one.
            let inputs: Option<Vec<Option<Key>>> = entry
                .inputs
                .iter()
                .map(|from| match from {
                    None => Some(None),
                    Some(from) => keys[&from.node].map(|key| Some(key.output(from.index))),
                })
                .collect();
            let key = inputs
                .zip(entry.node.identity().ok())
                .map(|(inputs, identity)| Key::node(entry.node.kind(), &identity, global, &inputs));
            keys.insert(entry.id, key);
        }
        Self {
            dag,
            runtime: &evaluator.runtime,
            global,
            store,
            keys,
            live: HashMap::new(),
            running: HashSet::new(),
            nodes: 0,
            hits: 0,
            evictions,
            timings: None,
        }
    }

    fn key(&self, port: PortId) -> Option<Key> {
        self.keys.get(&port.node).copied().flatten().map(|key| key.output(port.index))
    }

    /// Count one more consumer of `port`. The first one takes the value from
    /// the store or schedules its node.
    fn demand(&mut self, port: PortId) {
        let live = self.live.entry(port).or_default();
        live.uses += 1;
        if live.uses > 1 {
            return;
        }
        match self.key(port).and_then(|key| self.store.get(key)) {
            Some(image) => {
                self.live.get_mut(&port).unwrap().image = Some(image);
                self.hits += 1;
            }
            None => self.schedule(port.node),
        }
    }

    /// Run `node` in this call, demanding each of its connected inputs.
    fn schedule(&mut self, node: NodeId) {
        if self.running.insert(node.index) {
            let entry = self.dag.entries[node.index].as_ref().unwrap();
            for &from in entry.inputs.iter().flatten() {
                self.demand(from);
            }
        }
    }

    fn execute_batch(&mut self) -> HashMap<NodeId, Error> {
        let mut failures = HashMap::new();
        let dag = self.dag;
        let order: Vec<_> = dag.order.iter().filter(|i| self.running.contains(i)).collect();
        for index in order {
            let entry = dag.entries[*index].as_ref().unwrap();
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
            self.release(from);
        }
        result
    }

    fn kernels(&self) -> KernelContext<'a> {
        KernelContext { runtime: self.runtime, global: self.global }
    }

    fn value(&self, from: PortId) -> Result<&Image> {
        self.live
            .get(&from)
            .and_then(|live| live.image.as_ref())
            .ok_or_else(|| Error::Contract("requested source value is unavailable".into()))
    }

    fn execute(&mut self, e: &Entry) -> Result<()> {
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

    fn compute(&mut self, e: &Entry) -> Result<()> {
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
            let port = output_id(e.id, port);
            match self.live.get_mut(&port) {
                Some(live) => live.image = Some(image),
                None => self.keep(port, image),
            }
        }
        Ok(())
    }

    fn release_inputs(&mut self, e: &Entry) {
        for &from in e.inputs.iter().flatten() {
            self.release(from);
        }
    }

    /// One consumer of `port` is done. After the last one the run drops the
    /// value and offers it to the store. `port` must be demanded.
    fn release(&mut self, port: PortId) {
        let live = self.live.get_mut(&port).expect("released port is demanded");
        live.uses -= 1;
        if live.uses == 0
            && let Some(image) = self.live.remove(&port).and_then(|live| live.image)
        {
            self.keep(port, image);
        }
    }

    /// Store `image`, the value of `port`, when it has a key and samples.
    /// Rebuilding a deferred image no consumer materialized runs host
    /// functions only, so the store keeps rasters alone and never holds a
    /// plan's input outside its budget.
    fn keep(&mut self, port: PortId, image: Image) {
        if let Some(key) = self.key(port).filter(|_| image.footprint().is_some()) {
            let kind = self.dag.entries[port.node.index].as_ref().unwrap().node.kind();
            self.store.insert(key, image, kind.durability());
        }
    }

    /// Trim the store, whose entries consumers may have uploaded or
    /// downloaded during the run, and return the nodes run, the hits and the
    /// evictions of this call.
    fn finish(&mut self) -> (usize, usize, usize) {
        self.store.trim();
        (self.nodes, self.hits, self.store.evictions() - self.evictions)
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
