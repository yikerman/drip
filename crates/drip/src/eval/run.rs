//! One call's evaluation: the scale and key of each needed node, the
//! values on live edges and the order nodes run in.
use super::{
    Evaluator, NodeTiming, Policy,
    meta::Expansion,
    values::{InputValues, cut},
};
use crate::{
    Error, Result,
    definition::Node,
    graph::{Dag, Entry},
    image::{Description, Image},
    lattice::Rect,
    ports::*,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
    shape::{Cache, Codec, Shape},
    store::{Key, Store},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{MutexGuard, PoisonError},
    time::Instant,
};

pub(super) fn output_id(node: NodeId, index: usize) -> PortId {
    PortId { node, index, direction: Direction::Output }
}

/// A value a run holds for its consumers: a node's output at the scale the
/// node ran at, reduced to the scale it is read at where its lattice is
/// finer, and cut to a target's region. Each step derives from the value
/// before it and has a store key of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Slot {
    pub port: PortId,
    pub scale: u32,
    pub reduce: Option<u32>,
    pub region: Option<Rect>,
}
/// How a slot's value derives from its parent's.
#[derive(Clone, Copy)]
enum Derivation {
    /// Reduced to this scale where the parent's lattice is finer.
    Reduce(u32),
    /// The parent's samples whose cells meet a target's region.
    Cut(Rect),
}
impl Slot {
    /// Output `port` as its node publishes it at `scale`.
    pub fn output(port: PortId, scale: u32) -> Self {
        Self { port, scale, reduce: None, region: None }
    }

    /// The value this one derives from, and how.
    fn parent(self) -> Option<(Slot, Derivation)> {
        match self {
            Slot { region: Some(region), .. } => {
                Some((Slot { region: None, ..self }, Derivation::Cut(region)))
            }
            Slot { reduce: Some(scale), .. } => {
                Some((Slot { reduce: None, ..self }, Derivation::Reduce(scale)))
            }
            _ => None,
        }
    }
}

/// One call's evaluation: the scale each needed node runs at, the keys of
/// the values it may read, the nodes it runs and the values on its live
/// edges. It holds the evaluator's store for its duration. While a meta
/// node runs, the nodes of its sub-graph are nodes of the run too.
pub(super) struct Run<'a, 't> {
    dag: &'a Dag,
    runtime: &'a RuntimeContext,
    scale: u32,
    pub store: MutexGuard<'a, Store>,
    policy: Policy,
    /// The nodes whose outputs at the requested scale differ from their
    /// outputs at full detail. The others run at scale 1.
    scaled: HashSet<NodeId>,
    regions: HashMap<NodeId, Rect>,
    /// Per node and the scale it runs at, the key its output keys derive
    /// from, or why the node or an input has none.
    pub keys: HashMap<(NodeId, u32), Result<Key>>,
    /// Demanded values, with the consumers still to read each.
    live: HashMap<Slot, Live>,
    /// The nodes this run executes, at the scale each runs at.
    pub running: HashSet<(NodeId, u32)>,
    /// The sub-graph of the meta node running now.
    pub expansion: Option<Expansion>,
    pub nodes: usize,
    pub hits: usize,
    /// The store's read and eviction counts when the run began.
    reads: usize,
    evictions: usize,
    pub timings: Option<&'t mut Vec<NodeTiming>>,
}

/// A demanded value and the consumers still to read it.
#[derive(Default)]
struct Live {
    state: State,
    uses: usize,
}
/// Where a demanded value stands.
#[derive(Default)]
enum State {
    /// Its node has not published it yet.
    #[default]
    Pending,
    /// It derives from its parent slot on first read, and holds one use of
    /// the parent until then.
    Deriving,
    /// The value. `shared` when it is another slot's value, which that
    /// slot's key keeps.
    Ready { image: Image, shared: bool },
}

/// What a run did beside the runtime's work.
pub(super) struct Ran {
    pub nodes: usize,
    pub hits: usize,
    pub reads: usize,
    pub evictions: usize,
}

pub(super) const FULL: GlobalContext = GlobalContext { scale: 1 };

impl<'a, 't> Run<'a, 't> {
    /// Decide the scale of each node needed by `roots` at `global`'s scale,
    /// then key them in graph order. A foreign node stamps its files here,
    /// since its key includes them.
    pub(super) fn new(
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
            expansion: None,
            nodes: 0,
            hits: 0,
            reads,
            evictions,
            timings: None,
        };
        // The scales each node is read at: the request's from the roots, and
        // full detail from a foreign node up.
        let mut requested = HashSet::new();
        let mut stack: Vec<_> = roots.into_iter().map(|id| (id, global.scale)).collect();
        while let Some((id, scale)) = stack.pop() {
            if requested.insert((id, scale)) {
                let entry = run.entry(id);
                let read = if entry.node.kind().shape.reads_full_detail() { 1 } else { scale };
                stack.extend(entry.inputs.iter().flatten().map(|from| (from.node, read)));
            }
        }
        for entry in dag.order.iter().filter_map(|&index| dag.entries[index].as_ref()) {
            let id = entry.id;
            if requested.contains(&(id, run.scale)) && run.depends(entry) {
                run.scaled.insert(id);
            }
            for read in [1, run.scale] {
                let scale = run.scale_of(id, read);
                if requested.contains(&(id, read)) && !run.keys.contains_key(&(id, scale)) {
                    let key = run.key(id, scale);
                    run.keys.insert((id, scale), key);
                }
            }
        }
        run
    }

    /// Node `id` of the graph, or of the sub-graph of the meta node running
    /// now.
    fn entry(&self, id: NodeId) -> &Entry {
        let dag = match &self.expansion {
            Some(expansion) if expansion.holds(id) => &expansion.graph.dag,
            _ => self.dag,
        };
        dag.entries[id.index].as_ref().expect("needed nodes exist")
    }

    pub(super) fn reduces(&self, id: NodeId) -> bool {
        self.entry(id).node.kind().shape.reduces()
    }

    /// Whether `port` carries images on a plane, which evaluation reduces
    /// and cuts.
    fn planar(&self, port: PortId) -> bool {
        self.entry(port.node).node.kind().outputs[port.index].planar
    }

    /// Whether `entry`'s outputs at the requested scale differ from its
    /// outputs at full detail: its shape reads the scale, or it reads an
    /// image on a plane, which arrives reduced or from a node that depends
    /// on the scale. A foreign node reads full detail. Its inputs come
    /// first in graph order.
    fn depends(&self, entry: &Entry) -> bool {
        let shape = entry.node.kind().shape;
        self.scale > 1
            && !shape.reads_full_detail()
            && (shape.scaled()
                || entry
                    .inputs
                    .iter()
                    .flatten()
                    .any(|from| self.scaled.contains(&from.node) || self.planar(*from)))
    }

    /// The scale node `id` runs at when read at `scale`.
    pub(super) fn scale_of(&self, id: NodeId, scale: u32) -> u32 {
        if scale > 1 && self.scaled.contains(&id) { scale } else { 1 }
    }

    /// The key of node `id` running at `scale`: its identity, the scale for
    /// a scaled shape, and the keys of the values it reads.
    pub(super) fn key(&self, id: NodeId, scale: u32) -> Result<Key> {
        // A connected input without a key leaves the node without one.
        let inputs = self
            .inputs(id, scale)
            .into_iter()
            .map(|slot| slot.map(|slot| self.slot_key(slot).ok_or_else(|| self.key_error(slot))))
            .map(Option::transpose)
            .collect::<Result<Vec<_>>>()?;
        let node = &self.entry(id).node;
        Ok(Key::node(node.kind(), &node.identity()?, scale, &inputs))
    }

    fn key_error(&self, slot: Slot) -> Error {
        match self.keys.get(&(slot.port.node, slot.scale)) {
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
    pub(super) fn edge(&self, consumer: Option<&Entry>, scale: u32, port: PortId) -> Slot {
        let full_detail = consumer.is_some_and(|entry| entry.node.kind().shape.reads_full_detail());
        let read = if full_detail { 1 } else { scale };
        let at = self.scale_of(port.node, read);
        let planar = self.planar(port);
        let claimed = self.entry(port.node).node.kind().shape.produces_at_scale();
        let scaled = consumer.is_some_and(|entry| entry.node.kind().shape.scaled());
        // A warp's output can be finer than the scale: Orientation keeps the
        // sampling of a full-detail input.
        let reduce = (read > 1 && planar && !claimed && !scaled).then_some(read);
        let region = consumer.and_then(|entry| self.regions.get(&entry.id)).filter(|_| planar);
        Slot { port, scale: at, reduce, region: region.copied() }
    }

    /// The values node `id`, running at `scale`, reads, in input order.
    fn inputs(&self, id: NodeId, scale: u32) -> Vec<Option<Slot>> {
        let entry = self.entry(id);
        match &self.expansion {
            Some(expansion) if expansion.holds(id) => expansion.inputs(entry, scale),
            _ => entry
                .inputs
                .iter()
                .map(|from| from.map(|from| self.edge(Some(entry), scale, from)))
                .collect(),
        }
    }

    /// The values a frontend consumes of target `node` running at `scale`:
    /// a reduction's outputs, any other node's inputs.
    pub(super) fn consumed(&self, node: NodeId, scale: u32) -> Vec<Option<Slot>> {
        if !self.reduces(node) {
            return self.inputs(node, scale);
        }
        let outputs = self.entry(node).node.kind().outputs.len();
        (0..outputs).map(|port| Some(Slot::output(output_id(node, port), scale))).collect()
    }

    pub(super) fn slot_key(&self, slot: Slot) -> Option<Key> {
        let key = self.keys.get(&(slot.port.node, slot.scale))?.as_ref().ok()?;
        let key = key.output(slot.port.index);
        let key = slot.reduce.map_or(key, |scale| key.reduced(scale));
        Some(slot.region.map_or(key, |region| key.region(region)))
    }

    /// How the disk tier stores `slot`, when it is an output of a `foreign =
    /// disk` node.
    fn codec(&self, slot: Slot) -> Option<&'static Codec> {
        let codecs = self.entry(slot.port.node).node.kind().shape.codecs();
        codecs.get(slot.port.index).filter(|_| slot.parent().is_none())
    }

    /// Count one more consumer of `slot`. The first one takes the value from
    /// the store, in memory or on disk, or demands what it derives from, or
    /// schedules its node.
    pub(super) fn demand(&mut self, slot: Slot) {
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
                live.state = State::Ready { image, shared: false };
                self.hits += 1;
            }
            (None, Some((parent, _))) => {
                live.state = State::Deriving;
                self.demand(parent);
            }
            (None, None) => self.schedule(slot.port.node, slot.scale),
        }
    }

    /// Run node `id` at `scale` in this call, demanding what it reads.
    pub(super) fn schedule(&mut self, id: NodeId, scale: u32) {
        if self.running.insert((id, scale)) {
            for slot in self.inputs(id, scale).into_iter().flatten() {
                self.demand(slot);
            }
        }
    }

    /// The scheduled nodes of `dag` in its order, each at its scale.
    pub(super) fn order(&self, dag: &Dag) -> Vec<(NodeId, u32)> {
        let scales = if self.scale > 1 { vec![1, self.scale] } else { vec![1] };
        dag.order
            .iter()
            .filter_map(|&index| dag.entries[index].as_ref())
            .flat_map(|entry| scales.iter().map(|&scale| (entry.id, scale)))
            .filter(|at| self.running.contains(at))
            .collect()
    }

    /// Execute the scheduled nodes `order` in that order. A node whose
    /// input failed does not run and fails with that input's error, so each
    /// failure names the node it happened on.
    pub(super) fn execute_all(
        &mut self,
        order: Vec<(NodeId, u32)>,
    ) -> HashMap<(NodeId, u32), Error> {
        let mut failures = HashMap::new();
        for (id, scale) in order {
            let upstream = self
                .inputs(id, scale)
                .into_iter()
                .flatten()
                .find_map(|slot| failures.get(&(slot.port.node, slot.scale)).cloned());
            let result = if let Some(error) = upstream {
                self.release_inputs(id, scale);
                Err(error)
            } else {
                let kind = self.entry(id).node.kind().id;
                self.execute(id, scale).map_err(|source| Error::Node {
                    node: id,
                    kind,
                    source: Box::new(source),
                })
            };
            if let Err(error) = result {
                failures.insert((id, scale), error);
            }
        }
        failures
    }

    /// The values target `node` hands its frontend, or why they are missing,
    /// then release them. Values are materialized here, so the store keeps
    /// them, except an image on a plane larger than the device budget, which
    /// the store would not keep.
    pub(super) fn collect(
        &mut self,
        node: NodeId,
        failures: &HashMap<(NodeId, u32), Error>,
    ) -> Result<InputValues> {
        let scale = self.scale_of(node, self.scale);
        let slots = self.consumed(node, scale);
        let kind = self.entry(node).node.kind();
        let budget = self.runtime.budget().device;
        let result = match failures.get(&(node, scale)) {
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
                .map(|values| {
                    let (values, outputs) = if self.reduces(node) {
                        (vec![None; kind.inputs.len()], values.into_iter().flatten().collect())
                    } else {
                        (values, Vec::new())
                    };
                    InputValues { kind, values, outputs, runtime: self.runtime.clone() }
                }),
        };
        for slot in slots.into_iter().flatten() {
            self.release(slot);
        }
        result
    }

    pub(super) fn kernels(&self) -> KernelContext<'a> {
        KernelContext { runtime: self.runtime, global: &FULL }
    }

    /// The value of `slot`, deriving it from its parent on first read: a
    /// reduction to the requested scale, or the samples of a target's region,
    /// materialized.
    pub(super) fn resolve(&mut self, slot: Slot) -> Result<Image> {
        let live = self.live.get(&slot).expect("resolved slot is demanded");
        if let State::Ready { image, .. } = &live.state {
            return Ok(image.clone());
        }
        let (parent, derivation) = slot
            .parent()
            .ok_or_else(|| Error::Contract("requested source value is unavailable".into()))?;
        let source = self.resolve(parent)?;
        let image = match derivation {
            Derivation::Cut(region) => cut(&source, region, &self.kernels())?,
            Derivation::Reduce(scale) => source.reduce(scale)?,
        };
        let shared = image.is(&source);
        let live = self.live.get_mut(&slot).expect("resolved slot is demanded");
        live.state = State::Ready { image: image.clone(), shared };
        self.release(parent);
        Ok(image)
    }

    /// The value of `slot` for a consumer that publishes it under a key of
    /// its own, as a meta node publishes its sub-graph's outputs, then
    /// release it without storing it under its own.
    pub(super) fn take(&mut self, slot: Slot) -> Result<Image> {
        let image = self.resolve(slot);
        if let Some(Live { state: State::Ready { shared, .. }, .. }) = self.live.get_mut(&slot) {
            *shared = true;
        }
        self.release(slot);
        image
    }

    pub(super) fn execute(&mut self, id: NodeId, scale: u32) -> Result<()> {
        let e = self.entry(id).clone();
        let start = self.timings.as_ref().map(|_| Instant::now());
        let result = self.compute(&e, scale);
        self.release_inputs(id, scale);
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
        let slots = self.inputs(e.id, scale);
        let mut inputs = Vec::with_capacity(e.inputs.len());
        for slot in &slots {
            inputs.push(slot.map(|slot| self.resolve(slot)).transpose()?);
        }
        let shape = e.node.kind().shape;
        // A reduction computes a deferred input per pixel without storing
        // it. When another consumer of the run reads the input too,
        // computing it once for all of them costs less, and the store keeps
        // it.
        if shape.reduces() {
            for (slot, image) in slots.iter().zip(&inputs) {
                if let (Some(slot), Some(image)) = (slot, image)
                    && self.live[slot].uses > 1
                {
                    image.materialize(&self.kernels())?;
                }
            }
        }
        let specs = e.node.kind().outputs;
        let result = match shape {
            // A foreign node describes its outputs when it publishes them.
            Shape::Foreign(cache) => {
                e.node.contract(&FULL, &descriptions(&inputs))?;
                self.run_foreign(e, cache, inputs)?
            }
            Shape::Meta => self.expand(e, scale, &slots, &inputs)?,
            _ => self.run(e.node.as_ref(), scale, &inputs)?,
        };
        if result.len() != specs.len() {
            return Err(Error::Runtime("node result arity mismatch".into()));
        }
        self.nodes += 1;
        for (port, image) in result.into_iter().enumerate() {
            let slot = Slot::output(output_id(e.id, port), scale);
            match self.live.get_mut(&slot) {
                Some(live) => live.state = State::Ready { image, shared: false },
                None => self.keep(slot, image),
            }
        }
        Ok(())
    }

    /// Run `node`'s contract on its inputs' descriptions, then `node`, at
    /// `scale` when its shape is scaled and at scale 1 otherwise.
    fn run(&self, node: &dyn Node, scale: u32, inputs: &[Option<Image>]) -> Result<Vec<Image>> {
        let shape = node.kind().shape;
        // Only a scaled shape reads the scale, which keys it.
        let global = GlobalContext { scale: if shape.scaled() { scale } else { 1 } };
        let outputs = node.contract(&global, &descriptions(inputs))?;
        if outputs.len() != node.kind().outputs.len() {
            return Err(Error::Runtime("node contract output arity mismatch".into()));
        }
        let images = node.run(&KernelContext::new(&global, self.runtime), inputs, &outputs)?;
        if images.len() != outputs.len() {
            return Err(Error::Runtime("node result arity mismatch".into()));
        }
        Ok(images)
    }

    /// Materialize `inputs` here, then take the outputs of the store's job
    /// for `e`'s key, which runs the node at full detail on host samples.
    fn run_foreign(
        &mut self,
        e: &Entry,
        cache: Cache,
        inputs: Vec<Option<Image>>,
    ) -> Result<Vec<Image>> {
        let key = self.keys[&(e.id, 1)].clone()?;
        let kernels = self.kernels();
        for image in inputs.iter().flatten() {
            image.materialize(&kernels)?;
        }
        let (node, runtime) = (e.node.clone(), self.runtime.clone());
        let compute = move || node.run(&KernelContext::new(&FULL, &runtime), &inputs, &[]);
        let wait = self.policy == Policy::Wait;
        self.store.foreign(key, cache, wait, compute).unwrap_or(Err(Error::Pending))
    }

    fn release_inputs(&mut self, id: NodeId, scale: u32) {
        for slot in self.inputs(id, scale).into_iter().flatten() {
            self.release(slot);
        }
    }

    /// One consumer of `slot` is done. After the last one the run drops the
    /// value and offers it to the store, and releases the parent of a value
    /// it never derived. `slot` must be demanded.
    pub(super) fn release(&mut self, slot: Slot) {
        let live = self.live.get_mut(&slot).expect("released slot is demanded");
        live.uses -= 1;
        if live.uses > 0 {
            return;
        }
        match self.live.remove(&slot).expect("present above").state {
            State::Deriving => {
                let (parent, _) = slot.parent().expect("a deriving slot has a parent");
                self.release(parent);
            }
            State::Ready { image, shared: false } => self.keep(slot, image),
            State::Pending | State::Ready { shared: true, .. } => {}
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
            let kind = self.entry(slot.port.node).node.kind();
            self.store.insert(key, image, kind.durability());
        }
    }

    /// Trim the store, whose entries consumers may have uploaded or
    /// downloaded during the run, and return what the run did.
    pub(super) fn finish(&mut self) -> Ran {
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

/// The descriptions of `inputs`, as contracts receive them.
pub(super) fn descriptions(inputs: &[Option<Image>]) -> Vec<Option<Description>> {
    inputs.iter().map(|input| input.as_ref().map(|image| image.desc().clone())).collect()
}
