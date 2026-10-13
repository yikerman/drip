//! One sequential queue per project: previews, details and node actions run
//! in order on one evaluator, so later demands reuse what it stored, and
//! decoded RAWs persist in its disk tier across sessions.
//!
//! A preview does not wait for a foreign job: views that need it keep what
//! they showed, the status names the node, and the preview runs again when
//! the job ends. A detail is the visible region of a magnified view's input
//! at full detail, evaluated after the preview and shown over it once both
//! belong to the same graph. The app replaces the worker with its project,
//! discarding queued work, late replies and the memory store.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Instant;

use crate::model::{Graph, NodeId};
use crate::node_ui::NodeAction;
use drip::Error as NodeError;
use drip::eval::{Evaluator, NodeTiming, Policy, Target, TargetValues};
use drip::lattice::Rect;
use drip::runtime::{GlobalContext, RuntimeContext};
use drip::store::Disk;
use serde_json::Value as Json;

use crate::render::node_views::{Drawable, Images};

pub type ViewResult = Result<Option<Arc<dyn Drawable>>, NodeError>;
type Snapshot = BTreeMap<NodeId, ViewResult>;

/// Forces part of one graph snapshot under one context. Shared ancestors run
/// once, and values the evaluator stored for an earlier demand are reused.
/// The effect then consumes the inputs of its nodes.
#[derive(Clone)]
struct Demand {
    graph: Graph,
    global: GlobalContext,
    effect: Effect,
}

#[derive(Clone)]
enum Effect {
    /// Prepare the targets' views. A newer generation makes the result stale.
    Preview { generation: u64, targets: Vec<NodeId> },
    /// Prepare `id`'s view from its input over `region` of its plane at full
    /// detail, for preview `generation`.
    Detail { generation: u64, id: NodeId, region: Rect },
    /// Run a node's action. Actions override the preview's context with full
    /// detail.
    Perform { id: NodeId, action: NodeAction },
}

enum Command {
    Force(Demand),
    Collect,
}

enum Event {
    /// `pending` maps each target that waits for a foreign job to the name of
    /// the job's node. Those targets have no entry in `views`.
    Evaluated {
        generation: u64,
        views: Snapshot,
        failures: BTreeMap<NodeId, Failure>,
        pending: BTreeMap<NodeId, String>,
    },
    Performed {
        id: NodeId,
        result: Result<Option<Json>, String>,
    },
    /// A detail's view, or `None` when it failed or waits for a foreign job.
    Detailed {
        generation: u64,
        id: NodeId,
        view: Option<Arc<dyn Drawable>>,
    },
    /// A foreign job ended, so a preview waiting for it can run again.
    Ready,
    Stopped,
}

pub enum Notice {
    Evaluated(Result<(), String>),
    /// A view's full-detail crop arrived.
    Detailed,
    /// An action finished; it may carry new parameters for its node.
    Performed {
        id: NodeId,
        result: Result<Option<Json>, String>,
    },
    Failed,
}

#[derive(Clone, PartialEq)]
struct Failure {
    kind: &'static str,
    /// The failed node's name in the evaluated snapshot, for the status line.
    name: String,
    error: NodeError,
}

impl Failure {
    /// The error without the node it is reported against.
    fn cause(&self) -> &NodeError {
        match &self.error {
            NodeError::Node { source, .. } => source,
            error => error,
        }
    }

    fn level(&self) -> log::Level {
        match self.cause() {
            NodeError::MissingInput { .. } | NodeError::Contract(_) => log::Level::Debug,
            _ => log::Level::Warn,
        }
    }
}

#[derive(Clone)]
struct Notify {
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Notify {
    fn send(&self, event: Event) {
        let _ = self.events.send(event);
        (self.wake)();
    }
}

/// Reports a thread's end, by return or panic, so the UI stops showing
/// “Evaluating…”.
struct Stopping(Notify);

impl Drop for Stopping {
    fn drop(&mut self) {
        self.0.send(Event::Stopped);
    }
}

/// UI-side mailbox. At most one preview is sent and one latest request waits.
pub struct Worker {
    commands: mpsc::Sender<Command>,
    /// Set when dropped: each thread skips queued commands after its current one.
    stopped: Arc<AtomicBool>,
    events: mpsc::Receiver<Event>,
    generation: u64,
    in_flight: bool,
    pending: Option<Demand>,
    views: Snapshot,
    views_generation: Option<u64>,
    failures: BTreeMap<NodeId, Failure>,
    /// The latest preview request, sent again when a job it waits for ends.
    latest: Option<Demand>,
    /// The node whose foreign job the shown preview waits for.
    waiting: Option<String>,
    /// A foreign job ended since the last preview was sent.
    woken: bool,
    /// Per view node, the detail demand waiting to be sent, sent after any
    /// preview.
    details: BTreeMap<NodeId, Demand>,
    /// Per view node, the region and preview generation of the detail last
    /// asked for, so an unchanged window asks once.
    asked: BTreeMap<NodeId, (Rect, u64)>,
    /// Per view node, the latest detail evaluated and its preview generation.
    detailed: BTreeMap<NodeId, (u64, Arc<dyn Drawable>)>,
    collect: bool,
    failed: bool,
}

impl Worker {
    /// A worker whose evaluator keeps foreign outputs in `disk`, if given.
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>, disk: Option<Disk>) -> Self {
        let (events, replies) = mpsc::channel();
        let notify = Notify { events, wake };
        let stopped = Arc::new(AtomicBool::new(false));
        let mut evaluator = None;
        let mut images = Images::default();
        let commands =
            serve("drip-worker", &stopped, &notify, move |command, notify| match command {
                Command::Force(demand) => {
                    let evaluator = evaluator.get_or_insert_with(|| {
                        let ready = notify.clone();
                        RuntimeContext::from_env().map(|runtime| {
                            let evaluator =
                                Evaluator::new(runtime).with_wake(move || ready.send(Event::Ready));
                            match disk.clone() {
                                Some(disk) => evaluator.with_disk(disk),
                                None => evaluator,
                            }
                        })
                    });
                    notify.send(force(&demand, evaluator.as_ref(), &mut images));
                }
                Command::Collect => images.collect(),
            });
        Self {
            commands,
            stopped,
            events: replies,
            generation: 0,
            in_flight: false,
            pending: None,
            views: Snapshot::new(),
            views_generation: None,
            failures: BTreeMap::new(),
            latest: None,
            waiting: None,
            woken: false,
            details: BTreeMap::new(),
            asked: BTreeMap::new(),
            detailed: BTreeMap::new(),
            collect: false,
            failed: false,
        }
    }

    pub fn result(&self, id: NodeId) -> Option<&ViewResult> {
        self.views.get(&id)
    }

    /// Every displayed result belongs to one accepted request snapshot.
    pub fn showing_previous(&self) -> bool {
        self.views_generation.is_some_and(|generation| generation != self.generation)
    }

    /// A preview, detail or action is queued or running, or the shown
    /// preview waits for a foreign job.
    pub fn busy(&self) -> bool {
        self.in_flight
            || self.pending.is_some()
            || !self.details.is_empty()
            || self.waiting.is_some()
    }

    /// The full-detail view of the region `id`'s window last asked for, when
    /// it was evaluated for the preview the worker shows. Until then the
    /// window shows the preview alone.
    pub fn detail_view(&self, id: NodeId) -> Option<&Arc<dyn Drawable>> {
        let (generation, view) = self.detailed.get(&id)?;
        (Some(*generation) == self.views_generation).then_some(view)
    }

    /// Asks for `id`'s view from its input over `region` of its plane, in
    /// full-detail pixels, at full detail, or stops asking with `None`. The
    /// latest region per node is evaluated after any preview, once per
    /// region and preview generation.
    pub fn detail(
        &mut self,
        graph: &Graph,
        id: NodeId,
        region: Option<Rect>,
    ) -> Result<(), String> {
        let Some(region) = region else {
            self.details.remove(&id);
            self.asked.remove(&id);
            self.detailed.remove(&id);
            return Ok(());
        };
        if self.asked.insert(id, (region, self.generation)) == Some((region, self.generation)) {
            return Ok(());
        }
        let effect = Effect::Detail { generation: self.generation, id, region };
        let demand = Demand { graph: graph.clone(), global: Default::default(), effect };
        self.details.insert(id, demand);
        self.start()
    }

    /// The name of the node whose foreign job the shown preview waits for.
    pub fn waiting(&self) -> Option<&str> {
        self.waiting.as_deref()
    }

    pub fn request(
        &mut self,
        graph: &Graph,
        level: u8,
        targets: Vec<NodeId>,
    ) -> Result<(), String> {
        if level > crate::ui_state::MAX_LEVEL {
            return Err("invalid preview detail".into());
        }
        self.generation += 1;
        self.views.retain(|id, _| graph.kind(*id).is_some());
        self.failures.retain(|id, _| graph.kind(*id).is_some());
        self.detailed.retain(|id, _| graph.kind(*id).is_some());
        if self.pending.is_some() {
            log::trace!("coalesced pending preview into generation={}", self.generation);
        }
        self.collect = true;
        let demand = Demand {
            graph: graph.clone(),
            global: GlobalContext { scale: 1 << level },
            effect: Effect::Preview { generation: self.generation, targets },
        };
        self.latest = Some(demand.clone());
        self.pending = Some(demand);
        self.start()
    }

    /// Runs `action`, which belongs to node `id`'s kind.
    pub fn action(&self, graph: &Graph, id: NodeId, action: NodeAction) -> Result<(), String> {
        let effect = Effect::Perform { id, action };
        let demand = Demand { graph: graph.clone(), global: Default::default(), effect };
        self.send(&self.commands, Command::Force(demand))
    }

    pub fn poll(&mut self) -> Vec<Notice> {
        let mut notices = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Evaluated { generation, mut views, failures, pending } => {
                    self.in_flight = false;
                    self.collect = true;
                    if generation != self.generation {
                        log::trace!(
                            "discarded preview generation={generation} current={}",
                            self.generation
                        );
                        continue;
                    }
                    for (id, failure) in &failures {
                        if self.failures.get(id) != Some(failure) {
                            log::log!(
                                failure.level(),
                                "preview generation={generation} node={id:?} kind={}: {}",
                                failure.kind,
                                failure.error
                            );
                        }
                    }
                    let error = failures
                        .values()
                        .next()
                        .map(|failure| format!("{}: {}", failure.name, failure.cause()));
                    // A view that waits for a foreign job keeps what it showed.
                    for id in pending.keys() {
                        if let Some(view) = self.views.remove(id) {
                            views.insert(*id, view);
                        }
                    }
                    self.waiting = pending.into_values().next();
                    if let Some(name) = &self.waiting {
                        log::debug!("preview generation={generation} waits for {name}");
                    }
                    self.failures = failures;
                    self.views = views;
                    self.views_generation = Some(generation);
                    notices.push(Notice::Evaluated(error.map_or(Ok(()), Err)));
                }
                Event::Performed { id, result } => {
                    if !self.failed {
                        notices.push(Notice::Performed { id, result });
                    }
                }
                Event::Detailed { generation, id, view } => {
                    self.in_flight = false;
                    self.collect = true;
                    // A detail of an older graph never shows again; one of
                    // an older region shows until the newer one arrives.
                    if let Some(view) = view.filter(|_| generation == self.generation)
                        && self.asked.contains_key(&id)
                    {
                        self.detailed.insert(id, (generation, view));
                        notices.push(Notice::Detailed);
                    }
                }
                Event::Ready => self.woken = true,
                Event::Stopped => {
                    log::error!("worker stopped");
                    notices.push(self.fail());
                }
            }
        }
        if self.waiting.is_some() && self.woken && !self.in_flight && self.pending.is_none() {
            self.pending = self.latest.clone();
        }
        if self.start().is_err() && !self.failed {
            log::error!("worker disconnected");
            notices.push(self.fail());
        }
        notices
    }

    /// Called after rendering releases its previous callbacks/textures. The
    /// worker holds every prepared allocation until it has no other owners.
    pub fn after_frame(&mut self) {
        if std::mem::take(&mut self.collect) {
            let _ = self.send(&self.commands, Command::Collect);
        }
    }

    fn fail(&mut self) -> Notice {
        self.failed = true;
        self.in_flight = false;
        self.pending = None;
        self.details.clear();
        self.waiting = None;
        Notice::Failed
    }

    /// Sends the waiting preview, or else a waiting detail, unless a demand
    /// is in flight.
    fn start(&mut self) -> Result<(), String> {
        if self.in_flight {
            return Ok(());
        }
        let preview = self.pending.take();
        if preview.is_some() {
            self.woken = false;
        }
        let next = preview.or_else(|| self.details.pop_first().map(|(_, demand)| demand));
        if let Some(demand) = next {
            self.send(&self.commands, Command::Force(demand))?;
            self.in_flight = true;
        }
        Ok(())
    }

    fn send<C>(&self, queue: &mpsc::Sender<C>, command: C) -> Result<(), String> {
        if self.failed {
            return Err("the worker stopped".into());
        }
        queue.send(command).map_err(|_| "the worker stopped".into())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Do not join a decode or kernel from the UI thread; the thread ends
        // after its current command, and its replies have no receiver.
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// Starts a thread that handles commands in order until the worker is dropped.
fn serve<C: Send + 'static>(
    name: &str,
    stopped: &Arc<AtomicBool>,
    notify: &Notify,
    mut handle: impl FnMut(C, &Notify) + Send + 'static,
) -> mpsc::Sender<C> {
    let (queue, commands) = mpsc::channel();
    let stopped = stopped.clone();
    let stopping = Stopping(notify.clone());
    thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            while let Ok(command) = commands.recv() {
                if stopped.load(Ordering::Relaxed) {
                    break;
                }
                handle(command, &stopping.0);
            }
        })
        .expect("start worker");
    queue
}

fn force(demand: &Demand, evaluator: Result<&Evaluator, &NodeError>, images: &mut Images) -> Event {
    let start = Instant::now();
    let trace = log::log_enabled!(log::Level::Trace);
    let mut timings = Vec::new();
    let ctx = crate::node_ui::data::PrepareContext {
        global: demand.global,
        runtime: evaluator.map_or_else(|_| RuntimeContext::host(), |e| e.runtime().clone()),
    };
    let targets = demand.targets();
    // A view shows what is ready, and an action needs every input.
    let policy = match demand.effect {
        Effect::Preview { .. } | Effect::Detail { .. } => Policy::Defer,
        Effect::Perform { .. } => Policy::Wait,
    };
    let mut values = match evaluator {
        Ok(evaluator) => {
            evaluator
                .evaluate_batch(
                    &demand.graph.dag,
                    &demand.global,
                    &targets,
                    policy,
                    trace.then_some(&mut timings),
                )
                .values
        }
        Err(error) => targets.iter().map(|target| (target.node, Err(error.clone()))).collect(),
    };
    demand.log_timings("evaluate", &timings);
    match demand.effect {
        Effect::Preview { generation, .. } => {
            let mut timings = Vec::new();
            let mut failures = BTreeMap::new();
            let mut pending = BTreeMap::new();
            let mut views: Snapshot = targets
                .iter()
                .filter_map(|&Target { node: id, .. }| {
                    let node = demand.graph.dag.node(id).ok()?;
                    let result = match values.remove(&id) {
                        Some(target) => target.and_then(|target| {
                            prepare_view(
                                id,
                                node,
                                &target,
                                &ctx,
                                images,
                                trace.then_some(&mut timings),
                            )
                        }),
                        None => Ok(None),
                    };
                    if let Err(error) = &result {
                        let (failed, kind) = match error {
                            NodeError::Node { node, kind, .. } => (*node, *kind),
                            _ => (id, node.kind().id),
                        };
                        let name = demand.graph.name(failed).unwrap_or(kind).to_owned();
                        if matches!(error, NodeError::Node { source, .. } if **source == NodeError::Pending)
                        {
                            pending.insert(id, name);
                            return None;
                        }
                        failures.insert(failed, Failure { kind, name, error: error.clone() });
                    }
                    Some((id, result))
                })
                .collect();
            // A failed ancestor shows its own error; nodes nothing forced show none.
            for (&id, failure) in &failures {
                views.entry(id).or_insert_with(|| Err(failure.error.clone()));
            }
            demand.log_timings("prepare", &timings);
            log::debug!(
                "preview generation={generation} scale={} elapsed={:.1?} failures={} pending={}",
                demand.global.scale,
                start.elapsed(),
                failures.len(),
                pending.len()
            );
            Event::Evaluated { generation, views, failures, pending }
        }
        Effect::Detail { generation, id, region } => {
            let node = demand.graph.dag.node(id);
            let target = values.remove(&id).expect("demanded node");
            let view = node
                .map_err(|e| e.clone())
                .and_then(|node| prepare_view(id, node, &target?, &ctx, images, None));
            log::debug!(
                "detail generation={generation} node={id:?} region={region:?} elapsed={:.1?} {}",
                start.elapsed(),
                view.as_ref().map_or_else(ToString::to_string, |_| "ok".into())
            );
            Event::Detailed { generation, id, view: view.ok().flatten() }
        }
        Effect::Perform { id, action } => {
            let result = demand
                .graph
                .dag
                .node(id)
                .map_err(|_| "action node no longer exists".to_owned())
                .and_then(|node| {
                    values
                        .remove(&id)
                        .expect("demanded node")
                        .and_then(|target| action.run(node, target.inputs()?, &ctx))
                        .map_err(|e| e.to_string())
                });
            log::debug!("action {} node={id:?} elapsed={:.1?}", action.name, start.elapsed());
            Event::Performed { id, result }
        }
    }
}

impl Demand {
    /// The nodes the effect forces and whose inputs it consumes.
    fn targets(&self) -> Vec<Target> {
        match &self.effect {
            Effect::Preview { targets, .. } => targets.iter().map(|&id| id.into()).collect(),
            Effect::Detail { id, region, .. } => vec![Target { node: *id, region: Some(*region) }],
            Effect::Perform { id, .. } => vec![(*id).into()],
        }
    }

    fn log_timings(&self, phase: &str, timings: &[NodeTiming]) {
        let demand = match self.effect {
            Effect::Preview { generation, .. } => format!("preview generation={generation}"),
            Effect::Detail { generation, .. } => format!("detail generation={generation}"),
            Effect::Perform { action, .. } => format!("action {}", action.name),
        };
        for timing in timings {
            log::trace!(
                "node timing {demand} scale={} node={:?} kind={} phase={} host_elapsed={:.3?} success={}",
                self.global.scale,
                timing.node,
                timing.kind,
                phase,
                timing.host_elapsed,
                timing.success,
            );
        }
    }
}

fn prepare_view(
    id: NodeId,
    node: &dyn drip::definition::Node,
    values: &TargetValues,
    ctx: &crate::node_ui::data::PrepareContext,
    images: &mut Images,
    timings: Option<&mut Vec<NodeTiming>>,
) -> ViewResult {
    let kind = node.kind();
    let Some(binding) = crate::registry::binding(kind).filter(|b| b.has_preparation()) else {
        return Ok(None);
    };
    let start = timings.as_ref().map(|_| Instant::now());
    let result = binding.prepare(node, values, ctx, images);
    if let (Some(start), Some(timings)) = (start, timings) {
        timings.push(NodeTiming {
            node: id,
            kind: kind.id,
            host_elapsed: start.elapsed(),
            success: result.is_ok(),
        });
    }
    result
}

#[cfg(test)]
pub(crate) fn view<T: 'static>(worker: &Worker, id: NodeId) -> &T {
    worker.result(id).unwrap().as_ref().unwrap().as_ref().unwrap().downcast_ref().unwrap()
}

#[cfg(test)]
mod tests;
