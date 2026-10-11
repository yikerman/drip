//! One sequential queue per project: previews and node actions run in order
//! with one evaluator, which also reads a node's files when first forced. The
//! app replaces the worker with its project, discarding queued work and late
//! replies.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Instant;

use crate::model::{Graph, NodeId};
use crate::node_ui::NodeAction;
use drip::Error as NodeError;
use drip::eval::{Evaluator, InputValues, NodeTiming};
use drip::runtime::GlobalContext;
use serde_json::Value as Json;

use crate::render::node_views::{Drawable, Images};

pub type ViewResult = Result<Option<Arc<dyn Drawable>>, NodeError>;
type Snapshot = BTreeMap<NodeId, ViewResult>;

/// Forces part of one graph snapshot under one context. Shared ancestors run
/// once and each output lives until its last consumer is done; the effect then
/// consumes the inputs of its nodes.
struct Demand {
    graph: Graph,
    global: GlobalContext,
    effect: Effect,
}

enum Effect {
    /// Prepare the targets' views. A newer generation makes the result stale.
    Preview { generation: u64, targets: Vec<NodeId> },
    /// Run a node's action. Actions override the preview's context with full
    /// detail.
    Perform { id: NodeId, action: NodeAction },
}

enum Command {
    Force(Demand),
    Collect,
}

enum Event {
    Evaluated { generation: u64, views: Snapshot, failures: BTreeMap<NodeId, Failure> },
    Performed { id: NodeId, result: Result<Option<Json>, String> },
    Stopped,
}

pub enum Notice {
    Evaluated(Result<(), String>),
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
    collect: bool,
    failed: bool,
}

impl Worker {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (events, replies) = mpsc::channel();
        let notify = Notify { events, wake };
        let stopped = Arc::new(AtomicBool::new(false));
        let mut evaluator = None;
        let mut images = Images::default();
        let commands =
            serve("drip-worker", &stopped, &notify, move |command, notify| match command {
                Command::Force(demand) => {
                    let evaluator = evaluator.get_or_insert_with(|| {
                        drip::runtime::RuntimeContext::from_env().map(Evaluator::new)
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

    pub fn busy(&self) -> bool {
        self.in_flight || self.pending.is_some()
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
        if self.pending.is_some() {
            log::trace!("coalesced pending preview into generation={}", self.generation);
        }
        self.collect = true;
        self.pending = Some(Demand {
            graph: graph.clone(),
            global: GlobalContext { scale: 1 << level },
            effect: Effect::Preview { generation: self.generation, targets },
        });
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
                Event::Evaluated { generation, views, failures } => {
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
                Event::Stopped => {
                    log::error!("worker stopped");
                    notices.push(self.fail());
                }
            }
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
        Notice::Failed
    }

    fn start(&mut self) -> Result<(), String> {
        if !self.in_flight
            && let Some(request) = self.pending.take()
        {
            self.send(&self.commands, Command::Force(request))?;
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
        client: evaluator.ok().and_then(|e| e.client().ok()).cloned(),
    };
    let targets = demand.targets();
    let mut inputs = match evaluator {
        Ok(evaluator) => evaluator.evaluate_batch(
            &demand.graph.dag,
            &demand.global,
            &targets,
            trace.then_some(&mut timings),
        ),
        Err(error) => targets.iter().map(|&id| (id, Err(error.clone()))).collect(),
    };
    demand.log_timings("evaluate", &timings);
    match demand.effect {
        Effect::Preview { generation, .. } => {
            let mut timings = Vec::new();
            let mut failures = BTreeMap::new();
            let mut views: Snapshot = targets
                .iter()
                .filter_map(|&id| {
                    let node = demand.graph.dag.node(id).ok()?;
                    let result = match inputs.remove(&id) {
                        Some(inputs) => inputs.and_then(|inputs| {
                            prepare_view(
                                id,
                                node,
                                &inputs,
                                &ctx,
                                images,
                                trace.then_some(&mut timings),
                            )
                        }),
                        None => Ok(None),
                    };
                    if let Err(error) = &result {
                        let (id, kind) = match error {
                            NodeError::Node { node, kind, .. } => (*node, *kind),
                            _ => (id, node.kind().id),
                        };
                        let name = demand.graph.name(id).unwrap_or(kind).to_owned();
                        failures.insert(id, Failure { kind, name, error: error.clone() });
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
                "preview generation={generation} scale={} elapsed={:.1?} failures={}",
                demand.global.scale,
                start.elapsed(),
                failures.len()
            );
            Event::Evaluated { generation, views, failures }
        }
        Effect::Perform { id, action } => {
            let result = demand
                .graph
                .dag
                .node(id)
                .map_err(|_| "action node no longer exists".to_owned())
                .and_then(|node| {
                    let inputs = inputs.remove(&id).expect("demanded node");
                    inputs
                        .and_then(|inputs| action.run(node, &inputs, &ctx))
                        .map_err(|e| e.to_string())
                });
            log::debug!("action {} node={id:?} elapsed={:.1?}", action.name, start.elapsed());
            Event::Performed { id, result }
        }
    }
}

impl Demand {
    /// The nodes the effect forces and whose inputs it consumes.
    fn targets(&self) -> Vec<NodeId> {
        match &self.effect {
            Effect::Preview { targets, .. } => targets.clone(),
            Effect::Perform { id, .. } => vec![*id],
        }
    }

    fn log_timings(&self, phase: &str, timings: &[NodeTiming]) {
        let demand = match self.effect {
            Effect::Preview { generation, .. } => format!("preview generation={generation}"),
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
    inputs: &InputValues,
    ctx: &crate::node_ui::data::PrepareContext,
    images: &mut Images,
    timings: Option<&mut Vec<NodeTiming>>,
) -> ViewResult {
    let kind = node.kind();
    let Some(binding) = crate::node_ui::binding(kind).filter(|b| b.has_preparation()) else {
        return Ok(None);
    };
    let start = timings.as_ref().map(|_| Instant::now());
    let result = binding.prepare(node, inputs, ctx, images);
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
mod tests {
    use drip::{
        Error as KernelError,
        node::data::{Coordinates, Extent, REC2020_D65, Rgb, RgbDesc},
        ports::{Cpu, Write},
        runtime::KernelContext,
    };
    use serde::{Deserialize, Serialize};
    fn image_contract<P>(_: &drip::runtime::GlobalContext, _: &P) -> drip::Result<(RgbDesc,)> {
        Ok((RgbDesc {
            extent: Extent { width: 1, height: 1 },
            coordinates: Coordinates::Space(REC2020_D65),
        },))
    }
    fn empty_contract<P>(_: &drip::runtime::GlobalContext, _: &P) -> drip::Result<()> {
        Ok(())
    }

    use drip::ports::Read;

    use super::*;
    use crate::model::Kind;
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(5);
    // The compute client is shared with full-RAW GUI tests. Allow their
    // queued work and cold kernel compilation; channel gates stay short.
    const COMPUTE_TIMEOUT: Duration = Duration::from_secs(90);

    fn preview(generation: u64, graph: Graph, level: u8, targets: Vec<NodeId>) -> Demand {
        let global = GlobalContext { scale: 1 << level };
        Demand { graph, global, effect: Effect::Preview { generation, targets } }
    }

    #[test]
    fn backend_configuration_errors_reach_preview_results() {
        let mut graph = Graph::default();
        let id = graph.add_node(drip::definition::kind("view.preview").unwrap()).unwrap();
        let demand = preview(7, graph, 2, vec![id]);
        let error = NodeError::Runtime("DRIP_BACKEND=cpu requires --features drip/cpu".into());
        let Event::Evaluated { generation, views, failures } =
            force(&demand, Err(&error), &mut Images::default())
        else {
            panic!("expected preview completion");
        };
        assert_eq!(generation, 7);
        assert_eq!(views[&id].as_ref().err(), Some(&error));
        assert_eq!(failures[&id].error, error);
    }

    fn graph(source: &'static Kind) -> (Graph, NodeId, NodeId, NodeId) {
        let mut graph = Graph::default();
        let source = graph.add_node(source).unwrap();
        let preview = graph.add_node(drip::definition::kind("view.preview").unwrap()).unwrap();
        let histogram = graph.add_node(drip::definition::kind("view.histogram").unwrap()).unwrap();
        for target in [preview, histogram] {
            graph
                .connect(
                    graph.dag.output_port(source, "image").unwrap(),
                    graph.dag.input_port(target, "image").unwrap(),
                )
                .unwrap();
        }
        (graph, source, preview, histogram)
    }

    fn wait(worker: &mut Worker) -> Vec<Notice> {
        let deadline = Instant::now() + COMPUTE_TIMEOUT;
        let mut notices = Vec::new();
        loop {
            notices.extend(worker.poll());
            if !worker.busy() {
                return notices;
            }
            assert!(Instant::now() < deadline, "worker timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn action(graph: &Graph, id: NodeId) -> NodeAction {
        crate::node_ui::action(graph.kind(id).unwrap()).unwrap()
    }

    fn perform(worker: &mut Worker, graph: &Graph, id: NodeId) {
        worker.action(graph, id, action(graph, id)).unwrap();
        let deadline = Instant::now() + COMPUTE_TIMEOUT;
        loop {
            for notice in worker.poll() {
                if let Notice::Performed { id: performed, result } = notice {
                    assert_eq!(performed, id);
                    result.unwrap();
                    return;
                }
            }
            assert!(Instant::now() < deadline, "action did not finish");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn pixel(worker: &Worker, id: NodeId) -> [f32; 4] {
        let image = &view::<crate::node_ui::preview::ImageView>(worker, id).image;
        std::array::from_fn(|c| {
            half::f16::from_bits(u16::from_ne_bytes([image.texels[c * 2], image.texels[c * 2 + 1]]))
                .to_f32()
        })
    }

    mod preview_logs_only_changed_root_failures_at_their_severity_fixture {
        use super::*;
        #[derive(Clone, drip::Choice)]
        enum State {
            #[default]
            #[choice("incomplete")]
            Incomplete,
            #[choice("failed")]
            Failed,
            #[choice("ok")]
            Ok,
        }
        #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
        struct Source {
            #[param]
            state: State,
        }
        #[drip::node(id="test.diagnostics", category="test", name="diagnostics", contract=image_contract)]
        fn diagnostics(
            _: &KernelContext<'_>,
            p: &Source,
            image: Write<'_, Cpu<Rgb>>,
        ) -> drip::Result<()> {
            match p.state {
                State::Incomplete => Err(KernelError::Contract("choose a file".into())),
                State::Failed => Err("decode failed".into()),
                State::Ok => {
                    image.data[0] = [1., 1., 0.];
                    Ok(())
                }
            }
        }

        #[test]
        fn preview_logs_failures_and_passive_node_timings() {
            use std::cell::{Cell, RefCell};
            thread_local! {
                static RECORDS: RefCell<Vec<(log::Level, String)>> = const { RefCell::new(Vec::new()) };
                static TIMINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
                static TRACE: Cell<bool> = const { Cell::new(true) };
            }
            struct Capture;
            impl log::Log for Capture {
                fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
                    metadata.level() != log::Level::Trace || TRACE.get()
                }
                fn log(&self, record: &log::Record<'_>) {
                    let text = record.args().to_string();
                    if text.starts_with("preview generation=") {
                        RECORDS.with_borrow_mut(|records| records.push((record.level(), text)));
                    } else if text.starts_with("node timing ") {
                        TIMINGS.with_borrow_mut(|records| records.push(text));
                    }
                }
                fn flush(&self) {}
            }
            log::set_logger(&Capture).unwrap();
            log::set_max_level(log::LevelFilter::Trace);

            let source_kind = drip::definition::kind("test.diagnostics").unwrap();
            let (mut graph, source, preview, histogram) = graph(source_kind);
            let mut worker = Worker::new(Arc::new(|| {}));
            let mut evaluate = |graph: &Graph| {
                worker.request(graph, 1, vec![preview, histogram]).unwrap();
                let notices = wait(&mut worker);
                let logs = RECORDS.with_borrow_mut(std::mem::take);
                (notices, logs)
            };
            for (state, expected) in [
                ("incomplete", Some(log::Level::Debug)),
                ("incomplete", None),
                ("failed", Some(log::Level::Warn)),
                ("failed", None),
                ("ok", None),
                ("failed", Some(log::Level::Warn)),
            ] {
                graph.set_param(source, "state", json!(state)).unwrap();
                let (notices, logs) = evaluate(&graph);
                if let Some(expected) = expected {
                    assert_eq!(logs.len(), 1, "{logs:?}");
                    assert_eq!(logs[0].0, expected);
                    assert!(logs[0].1.contains(&format!("node={source:?} kind=test.diagnostics")));
                    assert!(logs[0].1.contains("preview generation="));
                } else {
                    assert!(logs.is_empty(), "{logs:?}");
                }
                if state != "ok" {
                    assert!(notices.iter().any(|notice| matches!(notice,
                    Notice::Evaluated(Err(error)) if error.starts_with("diagnostics: ")
                        && !error.contains("upstream"))));
                }
            }
            // Views name the failed source, which no view forces directly but
            // which still shows its own error.
            for id in [source, preview, histogram] {
                assert!(matches!(worker.result(id),
                    Some(Err(NodeError::Node { node, .. })) if *node == source));
            }
            worker = Worker::new(Arc::new(|| {}));
            worker.request(&graph, 1, vec![preview]).unwrap();
            wait(&mut worker);
            assert_eq!(
                RECORDS.with_borrow_mut(std::mem::take).len(),
                1,
                "a new worker has no failure history"
            );

            graph.set_param(source, "state", json!("ok")).unwrap();
            graph.disconnect(graph.dag.input_port(preview, "image").unwrap()).unwrap();
            worker.request(&graph, 1, vec![preview, histogram]).unwrap();
            wait(&mut worker);
            let logs = RECORDS.with_borrow_mut(std::mem::take);
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].0, log::Level::Debug);
            assert!(logs[0].1.contains("missing input"));

            graph
                .connect(
                    graph.dag.output_port(source, "image").unwrap(),
                    graph.dag.input_port(preview, "image").unwrap(),
                )
                .unwrap();
            let demand = super::preview(42, graph, 1, vec![preview, histogram]);
            let evaluator = Evaluator::new(drip::runtime::RuntimeContext::from_env().unwrap());
            for enabled in [true, false] {
                TRACE.set(enabled);
                let Event::Evaluated { failures, .. } =
                    super::super::force(&demand, Ok(&evaluator), &mut Images::default())
                else {
                    panic!("expected preview completion")
                };
                assert!(failures.is_empty());
                let logs = TIMINGS.with_borrow_mut(std::mem::take);
                assert_eq!(logs.len(), if enabled { 5 } else { 0 });
                if enabled {
                    assert_eq!(
                        logs.iter().filter(|line| line.contains("phase=evaluate")).count(),
                        3
                    );
                    assert_eq!(
                        logs.iter().filter(|line| line.contains("phase=prepare")).count(),
                        2
                    );
                    assert!(logs.iter().all(|line| line.contains("preview generation=42 scale=2")
                        && line.contains("host_elapsed=")
                        && line.contains("success=true")));
                }
            }
            TRACE.set(true);
        }
    }

    mod latest_targets_win_and_a_replaced_worker_skips_queued_work_fixture {
        use super::*;

        static GATE: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
        static CALLS: Mutex<Vec<f32>> = Mutex::new(Vec::new());
        #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
        struct SourceKernel {
            #[param(default = 1.0, range = 0.0..=10.0)]
            value: f32,
            #[param(default = false)]
            block: bool,
        }
        #[drip::node(id="test.blocking", category="test", name="blocking", contract=image_contract)]
        fn blocking(
            _: &KernelContext<'_>,
            p: &SourceKernel,
            image: Write<'_, Cpu<Rgb>>,
        ) -> drip::Result<()> {
            CALLS.lock().unwrap().push(p.value);
            if p.block {
                let gate = GATE.lock().unwrap();
                let (started, release) = gate.as_ref().unwrap();
                started.send(()).unwrap();
                release.recv_timeout(TIMEOUT).unwrap();
            }
            image.data[0] = [p.value, 1.0, 0.0];
            Ok(())
        }

        #[test]
        fn latest_targets_win_and_a_replaced_worker_skips_queued_work() {
            let source_kind = drip::definition::kind("test.blocking").unwrap();

            let (started, entered) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            *GATE.lock().unwrap() = Some((started, resume));
            let (wake, woke) = mpsc::channel();
            let mut worker = Worker::new(Arc::new(move || {
                let _ = wake.send(());
            }));
            let (mut graph, source, preview, histogram) = graph(source_kind);
            graph.set_param(source, "block", json!(true)).unwrap();
            worker.request(&graph, 1, vec![preview]).unwrap();
            entered.recv_timeout(TIMEOUT).unwrap();
            graph.set_param(source, "block", json!(false)).unwrap();
            for value in [2, 3] {
                graph.set_param(source, "value", json!(value)).unwrap();
                worker.request(&graph, 1, vec![preview, histogram]).unwrap();
            }
            assert!(worker.busy());
            let Effect::Preview { targets, .. } = &worker.pending.as_ref().unwrap().effect else {
                panic!("pending preview")
            };
            assert_eq!(targets, &[preview, histogram]);
            release.send(()).unwrap();
            woke.recv_timeout(COMPUTE_TIMEOUT).unwrap();
            assert!(worker.poll().is_empty(), "obsolete completion is not presented");
            assert!(worker.result(preview).is_none());
            wait(&mut worker);
            assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0]);
            assert_eq!(pixel(&worker, preview), [3.0, 1.0, 0.0, 1.0]);
            assert!(worker.result(histogram).is_some());

            // A new target set computes one fresh run and replaces the presentation.
            worker.request(&graph, 1, vec![histogram]).unwrap();
            wait(&mut worker);
            assert!(worker.result(preview).is_none());
            assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0, 3.0]);

            // A dropped worker finishes its current command, then skips its
            // queue: here an export that would evaluate the source again.
            let tiff = graph.add_node(drip::definition::kind("output.tiff").unwrap()).unwrap();
            graph
                .connect(
                    graph.dag.output_port(source, "image").unwrap(),
                    graph.dag.input_port(tiff, "image").unwrap(),
                )
                .unwrap();
            let (wake, woke) = mpsc::channel();
            let mut old = Worker::new(Arc::new(move || {
                let _ = wake.send(());
            }));
            graph.set_param(source, "block", json!(true)).unwrap();
            old.request(&graph, 1, vec![preview]).unwrap();
            entered.recv_timeout(TIMEOUT).unwrap();
            graph.set_param(source, "block", json!(false)).unwrap();
            graph.set_param(source, "value", json!(4)).unwrap();
            old.action(&graph, tiff, action(&graph, tiff)).unwrap();
            drop(old);
            // Evaluation wakes the UI for its preview, then as it stops.
            release.send(()).unwrap();
            for _ in 0..2 {
                woke.recv_timeout(COMPUTE_TIMEOUT).unwrap();
            }
            assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0, 3.0, 3.0]);
            worker.request(&graph, 2, vec![preview]).unwrap();
            wait(&mut worker);
            assert_eq!(pixel(&worker, preview), [4.0, 1.0, 0.0, 1.0]);
            graph.disconnect(graph.dag.input_port(preview, "image").unwrap()).unwrap();
            worker.request(&graph, 2, vec![preview]).unwrap();
            assert!(
                wait(&mut worker).iter().any(|notice| matches!(notice, Notice::Evaluated(Err(_))))
            );
            assert!(
                worker.result(preview).unwrap().is_err(),
                "a current error replaces the old image"
            );
        }
    }

    mod exports_read_current_files_at_full_detail_fixture {
        use super::*;

        #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
        struct FileKernel {
            #[param(open = [])]
            path: Option<std::path::PathBuf>,
        }
        // This fixture deliberately loads during computation to exercise worker ordering.
        // Production RAW sources bind immutable assets before evaluation.
        #[drip::node(id="test.file", category="test", name="file", contract=image_contract)]
        fn file(
            _: &KernelContext<'_>,
            p: &FileKernel,
            image: Write<'_, Cpu<Rgb>>,
        ) -> drip::Result<()> {
            let path =
                p.path.as_ref().ok_or_else(|| KernelError::Contract("choose a file".into()))?;
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            image.data[0] = [text.parse().unwrap(), 1.0, 0.0];
            Ok(())
        }
        #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
        struct WriteKernel {
            #[param(save = [])]
            path: Option<std::path::PathBuf>,
        }
        fn sink_contract(
            _: &drip::runtime::GlobalContext,
            _: &WriteKernel,
            _: &RgbDesc,
        ) -> drip::Result<()> {
            Ok(())
        }
        #[drip::node(id="test.write", category="test", name="write", contract=sink_contract)]
        fn write(
            _: &KernelContext<'_>,
            _: &WriteKernel,
            _image: Read<'_, Cpu<Rgb>>,
        ) -> drip::Result<()> {
            Ok(())
        }
        #[drip_macros::gui_node]
        impl crate::node_ui::GuiNode for write::Definition {
            type Presentation = ();
            const ACTION: Option<crate::node_ui::Action<Self>> = Some(crate::node_ui::Action {
                name: "write",
                run: |node, inputs, ctx| {
                    assert_eq!(ctx.global.scale, 1);
                    let image = inputs.image.data;
                    std::fs::write(
                        node.parameters.path.as_ref().unwrap(),
                        format!("{:?}", image[0]),
                    )
                    .map_err(|e| KernelError::Runtime(e.to_string()))?;
                    Ok(None)
                },
            });
        }

        #[test]
        fn exports_read_current_files_at_full_detail() {
            let input =
                std::env::temp_dir().join(format!("drip-worker-input-{}", std::process::id()));
            let output = input.with_extension("out");
            std::fs::write(&input, "1").unwrap();
            let (mut graph, source, preview, _) =
                graph(drip::definition::kind("test.file").unwrap());
            let export = graph.add_node(drip::definition::kind("test.write").unwrap()).unwrap();
            graph
                .connect(
                    graph.dag.output_port(source, "image").unwrap(),
                    graph.dag.input_port(export, "image").unwrap(),
                )
                .unwrap();
            graph.set_param(source, "path", json!(input)).unwrap();
            graph.set_param(export, "path", json!(output)).unwrap();
            let mut worker = Worker::new(Arc::new(|| {}));
            worker.request(&graph, 3, vec![preview]).unwrap();
            wait(&mut worker);
            let before = pixel(&worker, preview);
            std::fs::write(&input, "2").unwrap();
            perform(&mut worker, &graph, export);
            assert_eq!(std::fs::read_to_string(&output).unwrap(), "[2.0, 1.0, 0.0]");
            assert_eq!(pixel(&worker, preview), before, "export leaves preview intact");
            worker.request(&graph, 3, vec![preview]).unwrap();
            wait(&mut worker);
            assert_eq!(pixel(&worker, preview), [2.0, 1.0, 0.0, 1.0]);
            std::fs::write(&input, "3").unwrap();
            worker.request(&graph, 3, vec![preview]).unwrap();
            wait(&mut worker);
            assert_eq!(pixel(&worker, preview), [3.0, 1.0, 0.0, 1.0]);
            let mut profile =
                serde_json::to_value(drip::node::profile::Settings::default()).unwrap();
            profile["profile"] = json!({"file": {"path": input.with_extension("missing.icc")}});
            graph.set_param(preview, "mode", json!({"softproof": profile})).unwrap();
            worker.request(&graph, 3, vec![preview]).unwrap();
            wait(&mut worker);
            assert!(worker.result(preview).unwrap().is_err());
            perform(&mut worker, &graph, export);
            assert_eq!(
                std::fs::read_to_string(&output).unwrap(),
                "[3.0, 1.0, 0.0]",
                "GUI preparation failures do not block independent export actions"
            );
            assert!(worker.result(preview).unwrap().is_err());
            std::fs::remove_file(input).unwrap();
            std::fs::remove_file(output).unwrap();
        }
    }

    mod preparation_probe {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};

        pub static CALLS: AtomicUsize = AtomicUsize::new(0);

        #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
        pub struct Settings {
            #[param(default = true)]
            fail: bool,
        }

        #[drip::node(id="test.preparation_cache", category="test", name="Preparation", contract=empty_contract)]
        fn probe(_: &KernelContext<'_>, _: &Settings) -> drip::Result<()> {
            Ok(())
        }
        impl crate::node_ui::GuiNode for probe::Definition {
            type Presentation = ();
            const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|node, _, _| {
                CALLS.fetch_add(1, Ordering::Relaxed);
                if node.parameters.fail { Err("preparation failed".into()) } else { Ok(()) }
            });
        }
        #[linkme::distributed_slice(crate::node_ui::BINDINGS)]
        static UI: crate::node_ui::Binding = crate::node_ui::Binding::new::<probe::Definition>();
    }

    #[test]
    fn preparation_retries_each_request_without_persistent_image_cache() {
        use preparation_probe::CALLS;
        use std::sync::atomic::Ordering;

        let mut graph = Graph::default();
        let id = graph.add_node(drip::definition::kind("test.preparation_cache").unwrap()).unwrap();
        let mut worker = Worker::new(Arc::new(|| {}));
        for _ in 0..2 {
            worker.request(&graph, 1, vec![id]).unwrap();
            wait(&mut worker);
            assert!(
                matches!(worker.result(id), Some(Err(NodeError::Runtime(error))) if error == "preparation failed")
            );
        }
        assert_eq!(CALLS.load(Ordering::Relaxed), 2);
        graph.set_param(id, "fail", json!(false)).unwrap();
        worker.request(&graph, 1, vec![id]).unwrap();
        wait(&mut worker);
        let first = worker.result(id).unwrap().as_ref().unwrap().as_ref().unwrap().clone();
        worker.request(&graph, 1, vec![id]).unwrap();
        wait(&mut worker);
        let second = worker.result(id).unwrap().as_ref().unwrap().as_ref().unwrap();
        assert!(!Arc::ptr_eq(&first, second));
        assert_eq!(CALLS.load(Ordering::Relaxed), 4);
        worker.request(&graph, 2, vec![id]).unwrap();
        wait(&mut worker);
        assert_eq!(CALLS.load(Ordering::Relaxed), 5);
    }

    mod panic_wakes_the_ui_and_ends_the_busy_state_fixture {
        use super::*;

        #[drip::node(id="test.panic", category="test", name="panic", contract=empty_contract)]
        fn panic_node(_: &KernelContext<'_>, _: &()) -> drip::Result<()> {
            panic!("test worker failure")
        }

        #[test]
        fn panic_wakes_the_ui_and_ends_the_busy_state() {
            let mut graph = Graph::default();
            let id = graph.add_node(drip::definition::kind("test.panic").unwrap()).unwrap();
            let (wake, woke) = mpsc::channel();
            let mut worker = Worker::new(Arc::new(move || {
                let _ = wake.send(());
            }));
            worker.request(&graph, 0, vec![id]).unwrap();
            woke.recv_timeout(TIMEOUT).unwrap();
            assert!(worker.poll().iter().any(|notice| matches!(notice, Notice::Failed)));
            assert!(!worker.busy());
            assert!(worker.request(&graph, 0, vec![id]).is_err());
            assert!(!worker.busy());
        }
    }
}
