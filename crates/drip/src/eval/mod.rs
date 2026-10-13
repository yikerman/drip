//! Demand-driven evaluation: run what a demand needs, reuse what the store
//! keeps.
//!
//! Nothing runs until a caller forces it. A demand is a graph, a scale in
//! [`GlobalContext`] and targets, each with an optional region. Contracts,
//! missing inputs and file reads fail here, on the node that forced them,
//! since they read descriptions that exist only once data is loaded.
//!
//! ```text
//! targets ─► scale per node ─► key every node ─► walk back from the demanded values
//!                                                  ├─ stored: hit
//!                                                  └─ missing: schedule its node
//!         ─► run scheduled nodes in graph order ─► materialize for consumers ─► store
//! ```
//!
//! # Rules
//!
//! - **Scale.** From the targets, every node is read at the requested scale,
//!   and a foreign node reads its inputs at scale 1. A node read at the
//!   requested scale runs at it when its shape is scaled or it reads an image
//!   on a plane. Every other node runs at scale 1 and is keyed without the
//!   scale, so a RAW and its gains are stored once for every preview level.
//! - **Reduction.** An image on a plane read at scale `s > 1` whose lattice
//!   is finer than `div_ceil(full, s)` is reduced with
//!   [`BOX`](crate::body::BOX) as a stage of its consumer's plan, unless a
//!   `scaled` kernel produced it or the consumer's shape is scaled. The
//!   reduction is a preview approximation and never applies at scale 1.
//! - **Keys and hits.** A run keys every needed node in `Dag::order`, which
//!   stamps foreign nodes' files, then walks back from the demanded values. A
//!   stored value, in memory or on disk for a `foreign = disk` node, is a
//!   hit. A missing one demands the value it derives from (a reduction or a
//!   region) or schedules its node. Target nodes always run, so their
//!   contracts check what they receive, except reduce and bins targets,
//!   whose outputs are demanded instead.
//! - **Lifetimes.** A run counts the consumers of each value and drops it
//!   after the last, when the store keeps it if it has a key and samples. A
//!   target's inputs are materialized before the run ends, except an input
//!   on a plane larger than the device budget, which [`InputValues`]
//!   computes by region.
//! - **Regions.** Only a node without outputs takes a region, since a node
//!   computed from part of its inputs would publish part of its outputs under
//!   a whole-image key. Kernels, foreign nodes and sources run whole, so a
//!   region bounds the segments after the last of them. Stencils grow it by
//!   their reach.
//! - **Reductions.** A reduce or bins node computes a deferred input per
//!   pixel when it is the input's only consumer in the run. Otherwise the
//!   input materializes once for every consumer.
//! - **Meta nodes.** A meta node expands when it runs, once its inputs'
//!   descriptions exist. Its sub-graph's nodes are keyed from the meta node's
//!   input keys as if they were in the graph, served from the store walking
//!   back from the outputs, and run at the meta node's scale.
//! - **Foreign nodes.** The run materializes a foreign node's inputs and
//!   takes the store's job for its key. [`Policy::Wait`] blocks on a running
//!   job. [`Policy::Defer`] fails the node and its consumers with
//!   [`Error::Pending`] and leaves the job running.
//! - **Errors.** `evaluate` stops at the first error. `evaluate_batch`
//!   reports `Error::Node` on the failed node and repeats it on the nodes
//!   downstream, and targets whose ancestors succeeded still receive their
//!   inputs. Each call ends with one device wait, and a device error there
//!   fails every result.
use crate::{
    Error, Result,
    graph::Dag,
    image::Raster,
    lattice::Rect,
    payload::Payload,
    ports::*,
    runtime::{GlobalContext, RuntimeContext},
    store::{Disk, Store},
};
use run::{Ran, Run};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

mod meta;
mod run;
mod values;

pub use values::InputValues;

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
    /// stored. A meta node counts once, and each node of its sub-graph that
    /// runs once more.
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
    /// Reduction passes over a plan's pixels, one per
    /// [`Image::fold`](crate::image::Image::fold) or
    /// [`Image::count`](crate::image::Image::count), by reduce and bins
    /// nodes or frontends. A pass over a deferred image computes its pixels
    /// without a fused launch.
    pub reductions: usize,
    /// Stencil launches and passes whose radius spans less than one sample
    /// of a lattice coarser than full detail. The stencil then reads one
    /// sample on that axis, and the preview shows less of its effect than
    /// full detail does.
    pub approximated: usize,
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

/// A node whose values a frontend consumes: the inputs of a view, an
/// action or any other node, or the outputs of a reduce or bins node.
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
    /// so its contract checks the inputs it receives, except a reduce or bins
    /// node, whose outputs the store may hold.
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
            // A reduction's outputs are demanded like any value, so a stored
            // one runs nothing.
            if !run.reduces(target.node.index) {
                run.schedule(target.node.index, at);
            }
            // Collecting the target's values reads each once more.
            for slot in run.consumed(target.node, at).into_iter().flatten() {
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
    reductions: usize,
    approximated: usize,
}
impl Counts {
    fn of(runtime: &RuntimeContext) -> Self {
        Self {
            transfers: runtime.transfers(),
            fused: runtime.fused(),
            reductions: runtime.reductions(),
            approximated: runtime.approximated(),
        }
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
            reductions: runtime.reductions() - self.reductions,
            approximated: runtime.approximated() - self.approximated,
            hits: run.hits,
            reads: run.reads,
            evictions: run.evictions,
        }
    }
}
