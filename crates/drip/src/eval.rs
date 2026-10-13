//! Evaluate the requested dependency subgraph, reusing what the store keeps.
//!
//! A demand names targets, a scale and, per target, an optional region. Each
//! needed node runs at the requested scale when its outputs depend on it and
//! at full detail otherwise, and a full-detail image on a plane read at a
//! coarser scale is box-reduced in its consumer's kernel. A run keys every
//! value, takes what the store holds, runs the other nodes in graph order and
//! materializes each deferred image at its first consumer that needs samples,
//! over a target's region only. Foreign nodes run as store jobs, waited for
//! under [`Policy::Wait`] and reported pending under [`Policy::Defer`].
use crate::{
    Error, Result,
    definition::{Kind, Typed},
    graph::{Dag, Entry},
    image::{Image, Raster},
    payload::Payload,
    ports::*,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
    shape::{Cache, Codec, Rect, Shape},
    store::{Disk, Key, Store},
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
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

/// Work done by one `evaluate` or `evaluate_batch` call, counted on the
/// evaluator's runtime while the call runs. Reading the result's other side afterwards is the caller's
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
    /// Fused kernels launched, one per deferred image or window materialized,
    /// including the images a plan cuts at. A plan that neither moves nor
    /// maps samples launches none.
    pub fused: usize,
    /// Output samples the fused kernels wrote.
    pub samples: usize,
    /// Demanded values the store served.
    pub hits: usize,
    /// Values among `hits` read from the disk tier.
    pub reads: usize,
    /// Representations the store dropped to stay within its budget.
    pub evictions: usize,
}
/// What an evaluation does when a foreign node's job is still running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Block until the job ends. Headless evaluation and actions wait.
    Wait,
    /// Fail the node and its consumers with [`Error::Pending`] and leave the
    /// job running. When it ends, the evaluator's wake callback runs, and
    /// the next demand of the same values finds them stored. Interactive
    /// previews defer.
    Defer,
}

/// A node whose inputs a frontend consumes, a view or an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub node: NodeId,
    /// A rectangle of the plane of the node's inputs, in full-detail
    /// pixels. Each input on a plane then holds only its samples whose cells
    /// meet the region, and fails when there are none. Other inputs, and
    /// every input without a region, are whole. A node with outputs takes
    /// no region.
    pub region: Option<Rect>,
}
impl From<NodeId> for Target {
    fn from(node: NodeId) -> Self {
        Self { node, region: None }
    }
}

/// What [`Evaluator::evaluate_batch`] returns.
pub struct Batch {
    /// Each target's inputs, or why they are missing.
    pub inputs: BTreeMap<NodeId, Result<InputValues>>,
    pub statistics: Statistics,
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
/// one evaluator share the store and run one at a time. Foreign jobs still
/// running when the evaluator is dropped finish on their threads and write
/// their disk entries.
pub struct Evaluator {
    runtime: RuntimeContext,
    store: Mutex<Store>,
}
impl Evaluator {
    /// An evaluator with an empty store under `runtime`'s budget, in memory
    /// only.
    pub fn new(runtime: RuntimeContext) -> Self {
        let store = Mutex::new(Store::new(runtime.budget()));
        Self { runtime, store }
    }

    /// This evaluator with a disk tier in `disk`, which keeps the outputs of
    /// `foreign = disk` nodes across evaluators and processes.
    pub fn with_disk(mut self, disk: Disk) -> Self {
        self.store_mut().disk = Some(Arc::new(disk));
        self
    }

    /// This evaluator calling `wake` on a job's thread whenever a foreign
    /// job ends, so a caller that deferred can demand again.
    pub fn with_wake(mut self, wake: impl Fn() + Send + Sync + 'static) -> Self {
        self.store_mut().wake = Some(Arc::new(wake));
        self
    }

    fn store_mut(&mut self) -> &mut Store {
        self.store.get_mut().unwrap_or_else(PoisonError::into_inner)
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
        let targets: Vec<Target> = targets.iter().map(|&node| node.into()).collect();
        self.evaluate_batch(dag, global, &targets, Policy::Wait, None).inputs
    }

    /// `evaluate_inputs` for targets with regions, under `policy`, recording
    /// host timings. Targets name distinct nodes. With [`Policy::Defer`], a
    /// target that depends on a running foreign job receives `Error::Pending`
    /// inside the `Error::Node` of that job's node.
    ///
    /// `timings` receives host timings for attempted nodes in execution order.
    /// Shared ancestors appear once per scale they run at. Nodes skipped after
    /// upstream failure, and nodes whose values the store held, do not appear.
    /// Recording adds no device waits. Existing readback waits are charged to
    /// the consuming node, and the terminal device wait is outside these
    /// measurements.
    pub fn evaluate_batch(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        targets: &[Target],
        policy: Policy,
        timings: Option<&mut Vec<NodeTiming>>,
    ) -> Batch {
        let counts = Counts::of(&self.runtime);
        let mut inputs = BTreeMap::new();
        let mut valid = Vec::new();
        for target in targets {
            match target_error(dag, global, target) {
                Some(error) => drop(inputs.insert(target.node, Err(error))),
                None => valid.push(*target),
            }
        }
        let regions = valid.iter().filter_map(|t| Some((t.node, t.region?))).collect();
        let mut run = Run::new(self, dag, global, policy, valid.iter().map(|t| t.node), regions);
        run.timings = timings;
        for target in &valid {
            let at = run.scale_of(target.node.index, global.scale);
            run.schedule(target.node.index, at);
            // Collecting the target's inputs reads each once more.
            for slot in run.inputs(target.node, at).into_iter().flatten() {
                run.demand(slot);
            }
        }
        let failures = run.execute_batch();
        for target in &valid {
            inputs.insert(target.node, run.collect(target.node, &failures));
        }
        let statistics = counts.since(&self.runtime, run.finish());
        if let Err(error) = self.runtime.finish() {
            for value in inputs.values_mut().filter(|v| v.is_ok()) {
                *value = Err(error.clone());
            }
        }
        Batch { inputs, statistics }
    }

    /// Evaluate `target` and the nodes it depends on, waiting for foreign
    /// jobs.
    pub fn evaluate<P: Payload>(
        &self,
        dag: &Dag,
        global: &GlobalContext,
        target: Output<P>,
    ) -> Result<Evaluation<P>> {
        dag.output_of::<P>(target.id)?;
        global.validate()?;
        let counts = Counts::of(&self.runtime);
        let mut run = Run::new(self, dag, global, Policy::Wait, [target.node()], HashMap::new());
        let slot = run.edge(None, global.scale, target.id);
        run.demand(slot);
        for (index, scale) in run.order() {
            run.execute(index, scale)?;
        }
        let image = run.resolve(slot)?;
        image.materialize(&run.kernels())?;
        run.release(slot);
        let statistics = counts.since(&self.runtime, run.finish());
        let data = image.downcast::<P>().expect("output payload is checked").clone();
        // One terminal wait; host consumers have already waited at read boundaries.
        self.runtime.finish()?;
        Ok(Evaluation { desc: data.desc().clone(), data, statistics })
    }
}

/// Why `target` cannot be evaluated, if it cannot.
fn target_error(dag: &Dag, global: &GlobalContext, target: &Target) -> Option<Error> {
    let entry = match dag.entry(target.node) {
        Ok(entry) => entry,
        Err(error) => return Some(error),
    };
    if let Err(error) = global.validate() {
        return Some(error);
    }
    let region = target.region?;
    if region.extent.contains(&0) {
        return Some(Error::Contract("the region is empty".into()));
    }
    // A node computed from part of its inputs would publish part of its outputs.
    (!entry.node.kind().outputs.is_empty())
        .then(|| Error::Contract("only a node without outputs takes a region".into()))
}

/// The runtime's counts when a call began.
struct Counts {
    transfers: (usize, usize),
    fused: (usize, usize),
}
impl Counts {
    fn of(runtime: &RuntimeContext) -> Self {
        Self { transfers: runtime.transfers(), fused: runtime.fused() }
    }

    /// The call's statistics from the run's own counts.
    fn since(&self, runtime: &RuntimeContext, run: Ran) -> Statistics {
        let ((uploads, downloads), (fused, samples)) = (runtime.transfers(), runtime.fused());
        Statistics {
            nodes: run.nodes,
            uploads: uploads - self.transfers.0,
            downloads: downloads - self.transfers.1,
            fused: fused - self.fused.0,
            samples: samples - self.fused.1,
            hits: run.hits,
            reads: run.reads,
            evictions: run.evictions,
        }
    }
}
/// What a run did beside the runtime's work.
struct Ran {
    nodes: usize,
    hits: usize,
    reads: usize,
    evictions: usize,
}

fn output_id(node: NodeId, index: usize) -> PortId {
    PortId { node, index, direction: Direction::Output }
}

/// A value a run holds for its consumers: a node's output at the scale the
/// node ran at, reduced to the scale it is read at where its lattice is
/// finer, and cut to a target's region. Each step derives from the value
/// before it and has a store key of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Slot {
    port: PortId,
    scale: u32,
    reduce: Option<u32>,
    region: Option<Rect>,
}
impl Slot {
    /// The value this one derives from.
    fn parent(self) -> Option<Slot> {
        match self {
            Slot { region: Some(_), .. } => Some(Slot { region: None, ..self }),
            Slot { reduce: Some(_), .. } => Some(Slot { reduce: None, ..self }),
            _ => None,
        }
    }
}

/// One call's evaluation: the scale each needed node runs at, the keys of
/// the values it may read, the nodes it runs and the values on its live
/// edges. It holds the evaluator's store for its duration.
struct Run<'a, 't> {
    dag: &'a Dag,
    runtime: &'a RuntimeContext,
    /// The requested scale.
    scale: u32,
    store: MutexGuard<'a, Store>,
    policy: Policy,
    /// Indices of the nodes whose outputs at the requested scale differ from
    /// their outputs at full detail. The others run at scale 1.
    scaled: HashSet<usize>,
    /// Regions of the targets that have one.
    regions: HashMap<NodeId, Rect>,
    /// Per node index and the scale it runs at, the key its output keys
    /// derive from, or why the node or an input has none.
    keys: HashMap<(usize, u32), Result<Key>>,
    /// Demanded values, with the consumers still to read each.
    live: HashMap<Slot, Live>,
    /// The nodes this run executes, by index and scale.
    running: HashSet<(usize, u32)>,
    nodes: usize,
    hits: usize,
    /// The store's read and eviction counts when the run began.
    reads: usize,
    evictions: usize,
    timings: Option<&'t mut Vec<NodeTiming>>,
}
/// A demanded value, set when its node publishes it, the store serves it,
/// or a consumer first reads a value derived from another.
#[derive(Default)]
struct Live {
    image: Option<Image>,
    uses: usize,
    /// The value derives from its parent slot and holds one use of it until
    /// it is derived.
    parent: bool,
    /// The derived value is its parent's, which the parent's key keeps.
    alias: bool,
}

const FULL: GlobalContext = GlobalContext { scale: 1 };

impl<'a, 't> Run<'a, 't> {
    /// Decide the scale of each node needed by `roots` at `global`'s scale,
    /// then key them in graph order. A foreign node stamps its files here,
    /// since its key includes them.
    fn new(
        evaluator: &'a Evaluator,
        dag: &'a Dag,
        global: &GlobalContext,
        policy: Policy,
        roots: impl IntoIterator<Item = NodeId>,
        regions: HashMap<NodeId, Rect>,
    ) -> Self {
        let mut store = evaluator.store.lock().unwrap_or_else(PoisonError::into_inner);
        store.collect();
        let (reads, evictions) = (store.reads(), store.evictions());
        let mut run = Self {
            dag,
            runtime: &evaluator.runtime,
            scale: global.scale,
            store,
            policy,
            scaled: HashSet::new(),
            regions,
            keys: HashMap::new(),
            live: HashMap::new(),
            running: HashSet::new(),
            nodes: 0,
            hits: 0,
            reads,
            evictions,
            timings: None,
        };
        // The scales each node is read at: the request's from the roots, and
        // full detail from a foreign node up.
        let mut requested = HashSet::new();
        let mut stack: Vec<_> = roots.into_iter().map(|id| (id.index, global.scale)).collect();
        while let Some((index, scale)) = stack.pop() {
            if requested.insert((index, scale)) {
                let entry = run.entry(index);
                let read = if run.is_foreign(entry) { 1 } else { scale };
                stack.extend(entry.inputs.iter().flatten().map(|from| (from.node.index, read)));
            }
        }
        for &index in &dag.order {
            let entry = run.entry(index);
            if requested.contains(&(index, run.scale)) && run.depends(entry) {
                run.scaled.insert(index);
            }
            for read in [1, run.scale] {
                let scale = run.scale_of(index, read);
                if requested.contains(&(index, read)) && !run.keys.contains_key(&(index, scale)) {
                    let key = run.key(entry, scale);
                    run.keys.insert((index, scale), key);
                }
            }
        }
        run
    }

    fn entry(&self, index: usize) -> &'a Entry {
        self.dag.entries[index].as_ref().expect("needed nodes exist")
    }

    fn is_foreign(&self, entry: &Entry) -> bool {
        matches!(entry.node.kind().shape, Shape::Foreign(_))
    }

    /// Whether `port` carries images on a plane, which evaluation reduces
    /// and cuts.
    fn planar(&self, port: PortId) -> bool {
        self.entry(port.node.index).node.kind().outputs[port.index].planar
    }

    /// Whether `entry`'s outputs at the requested scale differ from its
    /// outputs at full detail: its shape reads the scale, or it reads an
    /// image on a plane, which arrives reduced or from a node that depends
    /// on the scale. A foreign node reads full detail. Its inputs come
    /// first in graph order.
    fn depends(&self, entry: &Entry) -> bool {
        let shape = entry.node.kind().shape;
        self.scale > 1
            && !self.is_foreign(entry)
            && (shape.scaled()
                || entry
                    .inputs
                    .iter()
                    .flatten()
                    .any(|from| self.scaled.contains(&from.node.index) || self.planar(*from)))
    }

    /// The scale node `index` runs at when read at `scale`.
    fn scale_of(&self, index: usize, scale: u32) -> u32 {
        if scale > 1 && self.scaled.contains(&index) { scale } else { 1 }
    }

    /// The key of `entry` running at `scale`: its identity, the scale for a
    /// scaled shape, and the keys of the values it reads.
    fn key(&self, entry: &Entry, scale: u32) -> Result<Key> {
        // A connected input without a key leaves the node without one.
        let inputs = entry
            .inputs
            .iter()
            .map(|from| {
                from.map(|from| {
                    let slot = self.edge(Some(entry), scale, from);
                    self.slot_key(slot).ok_or_else(|| self.key_error(slot))
                })
                .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Key::node(entry.node.kind(), &entry.node.identity()?, scale, &inputs))
    }

    fn key_error(&self, slot: Slot) -> Error {
        match self.keys.get(&(slot.port.node.index, slot.scale)) {
            Some(Err(error)) => error.clone(),
            _ => Error::Runtime("an input was not keyed".into()),
        }
    }

    /// The value `consumer`, running at `scale`, reads from `port`, or a
    /// caller of `evaluate` reads at `scale` without a consumer. An image on
    /// a plane read at a scale coarser than 1 is reduced where its lattice is
    /// finer, unless a `scaled` kernel produced it or the consumer has a
    /// scaled shape, which reaches the scale itself. A target's region cuts
    /// the images on a plane it reads.
    fn edge(&self, consumer: Option<&Entry>, scale: u32, port: PortId) -> Slot {
        let read = if consumer.is_some_and(|entry| self.is_foreign(entry)) { 1 } else { scale };
        let at = self.scale_of(port.node.index, read);
        let planar = self.planar(port);
        let claimed =
            matches!(self.entry(port.node.index).node.kind().shape, Shape::Kernel { scaled: true });
        let scaled = consumer.is_some_and(|entry| entry.node.kind().shape.scaled());
        // A warp's output can be finer than the scale: Orientation keeps the
        // sampling of a full-detail input.
        let reduce = (read > 1 && planar && !claimed && !scaled).then_some(read);
        let region = consumer.and_then(|entry| self.regions.get(&entry.id)).filter(|_| planar);
        Slot { port, scale: at, reduce, region: region.copied() }
    }

    /// The values `node`, running at `scale`, reads, in input order.
    fn inputs(&self, node: NodeId, scale: u32) -> Vec<Option<Slot>> {
        let entry = self.entry(node.index);
        entry
            .inputs
            .iter()
            .map(|from| from.map(|from| self.edge(Some(entry), scale, from)))
            .collect()
    }

    fn slot_key(&self, slot: Slot) -> Option<Key> {
        let key = self.keys.get(&(slot.port.node.index, slot.scale))?.as_ref().ok()?;
        let key = key.output(slot.port.index);
        let key = slot.reduce.map_or(key, |scale| key.reduced(scale));
        Some(slot.region.map_or(key, |region| key.region(region)))
    }

    /// How the disk tier stores `slot`, when it is an output of a `foreign =
    /// disk` node.
    fn codec(&self, slot: Slot) -> Option<&'static Codec> {
        match self.entry(slot.port.node.index).node.kind().shape {
            Shape::Foreign(Cache::Disk(codecs)) if slot.parent().is_none() => {
                codecs.get(slot.port.index)
            }
            _ => None,
        }
    }

    /// Count one more consumer of `slot`. The first one takes the value from
    /// the store, in memory or on disk, or demands what it derives from, or
    /// schedules its node.
    fn demand(&mut self, slot: Slot) {
        let live = self.live.entry(slot).or_default();
        live.uses += 1;
        if live.uses > 1 {
            return;
        }
        let codec = self.codec(slot);
        let stored = self.slot_key(slot).and_then(|key| {
            self.store.get(key).or_else(|| codec.and_then(|codec| self.store.read(key, codec)))
        });
        let live = self.live.get_mut(&slot).expect("demanded above");
        match (stored, slot.parent()) {
            (Some(image), _) => {
                live.image = Some(image);
                self.hits += 1;
            }
            (None, Some(parent)) => {
                live.parent = true;
                self.demand(parent);
            }
            (None, None) => self.schedule(slot.port.node.index, slot.scale),
        }
    }

    /// Run node `index` at `scale` in this call, demanding what it reads.
    fn schedule(&mut self, index: usize, scale: u32) {
        if self.running.insert((index, scale)) {
            for slot in self.inputs(self.entry(index).id, scale).into_iter().flatten() {
                self.demand(slot);
            }
        }
    }

    /// The scheduled nodes in graph order, each at its scale.
    fn order(&self) -> Vec<(usize, u32)> {
        let scales = if self.scale > 1 { vec![1, self.scale] } else { vec![1] };
        let dag = self.dag;
        dag.order
            .iter()
            .flat_map(|&index| scales.iter().map(move |&scale| (index, scale)))
            .filter(|at| self.running.contains(at))
            .collect()
    }

    fn execute_batch(&mut self) -> HashMap<(usize, u32), Error> {
        let mut failures = HashMap::new();
        for (index, scale) in self.order() {
            let entry = self.entry(index);
            let upstream = self
                .inputs(entry.id, scale)
                .into_iter()
                .flatten()
                .find_map(|slot| failures.get(&(slot.port.node.index, slot.scale)).cloned());
            let result = if let Some(error) = upstream {
                self.release_inputs(index, scale);
                Err(error)
            } else {
                self.execute(index, scale).map_err(|source| Error::Node {
                    node: entry.id,
                    kind: entry.node.kind().id,
                    source: Box::new(source),
                })
            };
            if let Err(error) = result {
                failures.insert((index, scale), error);
            }
        }
        failures
    }

    /// The inputs of target `node`, or why they are missing, then release
    /// them. Inputs are materialized here, so the store keeps them, except an
    /// image on a plane larger than the device budget, which the store would
    /// not keep.
    fn collect(
        &mut self,
        node: NodeId,
        failures: &HashMap<(usize, u32), Error>,
    ) -> Result<InputValues> {
        let scale = self.scale_of(node.index, self.scale);
        let slots = self.inputs(node, scale);
        let budget = self.runtime.budget().device;
        let result = match failures.get(&(node.index, scale)) {
            Some(error) => Err(error.clone()),
            None => slots
                .iter()
                .map(|slot| {
                    slot.map(|slot| {
                        let image = self.resolve(slot)?;
                        if image.byte_len().is_none_or(|bytes| bytes as u64 <= budget) {
                            image.materialize(&self.kernels())?;
                        }
                        Ok(image)
                    })
                    .transpose()
                })
                .collect::<Result<Vec<_>>>()
                .map(|values| InputValues {
                    kind: self.entry(node.index).node.kind(),
                    values,
                    runtime: self.runtime.clone(),
                }),
        };
        for slot in slots.into_iter().flatten() {
            self.release(slot);
        }
        result
    }

    fn kernels(&self) -> KernelContext<'a> {
        KernelContext { runtime: self.runtime, global: &FULL }
    }

    /// The value of `slot`, deriving it from its parent on first read: a
    /// reduction to the requested scale, or the samples of a target's region,
    /// materialized.
    fn resolve(&mut self, slot: Slot) -> Result<Image> {
        let live = self.live.get(&slot).expect("resolved slot is demanded");
        if let Some(image) = &live.image {
            return Ok(image.clone());
        }
        let parent = slot
            .parent()
            .ok_or_else(|| Error::Contract("requested source value is unavailable".into()))?;
        let source = self.resolve(parent)?;
        let image = match (slot.region, slot.reduce) {
            (Some(region), _) => cut(&source, region, &self.kernels())?,
            (None, Some(scale)) => source.reduce(scale)?,
            (None, None) => unreachable!("a slot with a parent derives from it"),
        };
        let live = self.live.get_mut(&slot).expect("resolved slot is demanded");
        live.alias = image.is(&source);
        live.image = Some(image.clone());
        live.parent = false;
        self.release(parent);
        Ok(image)
    }

    fn execute(&mut self, index: usize, scale: u32) -> Result<()> {
        let e = self.entry(index);
        let start = self.timings.as_ref().map(|_| Instant::now());
        let result = self.compute(e, scale);
        self.release_inputs(index, scale);
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

    fn compute(&mut self, e: &Entry, scale: u32) -> Result<()> {
        let mut inputs = Vec::with_capacity(e.inputs.len());
        for slot in self.inputs(e.id, scale) {
            inputs.push(slot.map(|slot| self.resolve(slot)).transpose()?);
        }
        let descs: Vec<_> = inputs.iter().map(|v| v.as_ref().map(|i| i.desc().clone())).collect();
        let shape = e.node.kind().shape;
        // Only a scaled shape reads the scale, which keys it.
        let global = GlobalContext { scale: if shape.scaled() { scale } else { 1 } };
        let outputs = e.node.contract(&global, &descs)?;
        let specs = e.node.kind().outputs;
        let result = match shape {
            // A foreign node describes its outputs when it publishes them.
            Shape::Foreign(cache) => self.run_foreign(e, cache, inputs)?,
            _ if outputs.len() != specs.len() => {
                return Err(Error::Runtime("node contract output arity mismatch".into()));
            }
            _ => e.node.run(&KernelContext::new(&global, self.runtime), &inputs, &outputs)?,
        };
        if result.len() != specs.len() {
            return Err(Error::Runtime("node result arity mismatch".into()));
        }
        self.nodes += 1;
        for (port, image) in result.into_iter().enumerate() {
            let slot = Slot { port: output_id(e.id, port), scale, reduce: None, region: None };
            match self.live.get_mut(&slot) {
                Some(live) => live.image = Some(image),
                None => self.keep(slot, image),
            }
        }
        Ok(())
    }

    /// Materialize `inputs` here, then take the outputs of the store's job
    /// for `e`'s key, which runs the node at full detail on host samples.
    fn run_foreign(
        &mut self,
        e: &Entry,
        cache: Cache,
        inputs: Vec<Option<Image>>,
    ) -> Result<Vec<Image>> {
        let key = self.keys[&(e.id.index, 1)].clone()?;
        let kernels = self.kernels();
        for image in inputs.iter().flatten() {
            image.materialize(&kernels)?;
        }
        let (node, runtime) = (e.node.clone(), self.runtime.clone());
        let compute = move || node.run(&KernelContext::new(&FULL, &runtime), &inputs, &[]);
        let wait = self.policy == Policy::Wait;
        self.store.foreign(key, cache, wait, compute).unwrap_or(Err(Error::Pending))
    }

    fn release_inputs(&mut self, index: usize, scale: u32) {
        for slot in self.inputs(self.entry(index).id, scale).into_iter().flatten() {
            self.release(slot);
        }
    }

    /// One consumer of `slot` is done. After the last one the run drops the
    /// value and offers it to the store, and releases the parent of a value
    /// it never derived. `slot` must be demanded.
    fn release(&mut self, slot: Slot) {
        let live = self.live.get_mut(&slot).expect("released slot is demanded");
        live.uses -= 1;
        if live.uses > 0 {
            return;
        }
        let live = self.live.remove(&slot).expect("present above");
        if let (true, Some(parent)) = (live.parent, slot.parent()) {
            self.release(parent);
        }
        if let Some(image) = live.image.filter(|_| !live.alias) {
            self.keep(slot, image);
        }
    }

    /// Store `image`, the value of `slot`, when it has a key and samples.
    /// Rebuilding a deferred image no consumer materialized runs host
    /// functions only, so the store keeps rasters alone and never holds a
    /// plan's input outside its budget.
    fn keep(&mut self, slot: Slot, image: Image) {
        if image.footprint().is_none() {
            return;
        }
        if let Some(key) = self.slot_key(slot) {
            let kind = self.entry(slot.port.node.index).node.kind();
            self.store.insert(key, image, kind.durability());
        }
    }

    /// Trim the store, whose entries consumers may have uploaded or
    /// downloaded during the run, and return what the run did.
    fn finish(&mut self) -> Ran {
        self.store.trim();
        let (reads, evictions) = (self.store.reads(), self.store.evictions());
        Ran {
            nodes: self.nodes,
            hits: self.hits,
            reads: reads - self.reads,
            evictions: evictions - self.evictions,
        }
    }
}

/// The samples of `image`, which is on a plane, whose cells meet `region`,
/// materialized. A region that covers the image yields the image itself.
fn cut(image: &Image, region: Rect, kernels: &KernelContext<'_>) -> Result<Image> {
    let lattice = image.lattice().expect("regions cut images on a plane");
    let window = lattice
        .cover(region)
        .ok_or_else(|| Error::Contract("the region lies outside the image".into()))?;
    if lattice.with(window).is_whole() {
        image.materialize(kernels)?;
        return Ok(image.clone());
    }
    image.crop(window, kernels)
}

/// A node's evaluated inputs, in its input order. An input on a plane
/// larger than the device budget stays deferred until it is bound or cut, so
/// an action can compute it in bands.
pub struct InputValues {
    kind: &'static Kind,
    values: Vec<Option<Image>>,
    /// The runtime that materializes deferred inputs.
    runtime: RuntimeContext,
}
impl InputValues {
    /// Typed views as declared by node type `N`; these must be `N`'s inputs.
    /// Deferred inputs are materialized whole first.
    pub fn bind<N: Typed>(&self) -> Result<N::Inputs<'_>> {
        if !std::ptr::eq(self.kind, N::KIND) {
            return Err(Error::Contract(format!(
                "inputs of {} cannot bind as {}",
                self.kind.id,
                N::KIND.id
            )));
        }
        let kernels = KernelContext::new(&FULL, &self.runtime);
        for image in self.values.iter().flatten() {
            image.materialize(&kernels)?;
        }
        N::bind(&self.values)
    }

    /// The device budget of the evaluator's store, which bounds what
    /// [`bands`](Self::bands) holds.
    pub fn budget(&self) -> u64 {
        self.runtime.budget().device
    }

    /// The same inputs with each input on a plane holding only its samples
    /// whose cells meet `region` of its plane, in full-detail pixels, as
    /// [`Target::region`] does. Fails where none do.
    pub fn region(&self, region: Rect) -> Result<InputValues> {
        let kernels = KernelContext::new(&FULL, &self.runtime);
        let cut = |image: &Image| match image.lattice() {
            Some(_) => cut(image, region, &kernels),
            None => Ok(image.clone()),
        };
        let values =
            self.values.iter().map(|v| v.as_ref().map(cut).transpose()).collect::<Result<_>>()?;
        Ok(InputValues { values, ..self.clone() })
    }

    /// Regions of whole rows of the inputs' plane, top to bottom, that cut
    /// each input on a plane into parts of at most `bytes`. One region covers
    /// the plane when every input fits, or when one stores other than one
    /// sample per full-detail pixel. Inputs must share their plane.
    pub fn bands(&self, bytes: u64) -> Vec<Rect> {
        let planes: Vec<_> = self
            .values
            .iter()
            .flatten()
            .filter_map(|image| Some((image.lattice()?, image.byte_len()? as u64)))
            .collect();
        let Some((lattice, _)) = planes.first() else { return Vec::new() };
        let [width, height] = lattice.full;
        let full = planes.iter().all(|(l, _)| l.extent == l.full && l.is_whole());
        let largest = planes.iter().map(|(_, bytes)| *bytes).max().unwrap_or(0);
        let rows = match full && largest > bytes {
            true => (bytes / largest.div_ceil(u64::from(height))).clamp(1, height.into()) as u32,
            false => height,
        };
        (0..height)
            .step_by(rows as usize)
            .map(|y| Rect { origin: [0, y], extent: [width, rows.min(height - y)] })
            .collect()
    }
}
impl Clone for InputValues {
    fn clone(&self) -> Self {
        Self { kind: self.kind, values: self.values.clone(), runtime: self.runtime.clone() }
    }
}
