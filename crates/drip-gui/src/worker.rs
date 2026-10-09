//! Background evaluation of explicit targets at one global preview level.

use std::collections::BTreeMap;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Instant;

use crate::model::{Graph, NodeId};
use drip::Error as NodeError;
use drip::eval::{Evaluator, InputValues, NodeTiming};

use crate::render::node_views::{Drawable, ImageCache};

pub type ViewResult = Result<Option<Arc<dyn Drawable>>, NodeError>;
type Snapshot = BTreeMap<NodeId, ViewResult>;

struct Request {
    generation: u64,
    graph: Graph,
    level: u8,
    targets: Vec<NodeId>,
}

enum Command {
    Evaluate(Request),
    Reset,
    Invalidate,
    Action { graph: Graph, id: NodeId, name: &'static str },
    Collect,
    Shutdown,
    Bind { ticket: u64, id: NodeId, kind: &'static str, params: serde_json::Value },
    Open { ticket: u64, file: std::path::PathBuf },
}

enum Event {
    Evaluated { generation: u64, level: u8, views: Snapshot, failures: BTreeMap<NodeId, Failure> },
    Action(Result<(), String>),
    Stopped,
    Bound { ticket: u64, id: NodeId, result: drip::Result<Arc<dyn drip::definition::Node>> },
    Opened { ticket: u64, file: std::path::PathBuf, result: Result<crate::model::Project, String> },
}

pub enum Notice {
    Evaluated(Result<(), String>),
    Action(Result<(), String>),
    Failed,
    Bound { id: NodeId, result: drip::Result<Arc<dyn drip::definition::Node>> },
    Opened { file: std::path::PathBuf, result: Result<crate::model::Project, String> },
}

#[derive(Clone, PartialEq)]
struct Failure {
    kind: &'static str,
    error: NodeError,
}

impl Failure {
    fn level(&self) -> log::Level {
        let error = match &self.error {
            NodeError::Node { source, .. } => source.as_ref(),
            error => error,
        };
        match error {
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

/// UI-side mailbox. At most one preview is sent and one latest request waits.
pub struct Worker {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Event>,
    generation: u64,
    in_flight: bool,
    pending: Option<Request>,
    views: Snapshot,
    views_generation: Option<u64>,
    failures: BTreeMap<NodeId, Failure>,
    collect: bool,
    failed: bool,
    job_ticket: u64,
    bindings: BTreeMap<NodeId, u64>,
    opening: Option<u64>,
}

impl Worker {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (commands, requests) = mpsc::channel();
        let (events, replies) = mpsc::channel();
        let notify = Notify { events, wake: Arc::new(wake) };
        thread::Builder::new()
            .name("drip-preview".into())
            .spawn(move || {
                // A panic must wake the UI too, so it can stop showing “evaluating…”.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    serve(requests, &notify)
                }));
                notify.send(Event::Stopped);
            })
            .expect("start preview worker");
        Self {
            commands,
            events: replies,
            generation: 0,
            in_flight: false,
            pending: None,
            views: Snapshot::new(),
            views_generation: None,
            failures: BTreeMap::new(),
            collect: false,
            failed: false,
            job_ticket: 0,
            bindings: BTreeMap::new(),
            opening: None,
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
        self.in_flight
            || self.pending.is_some()
            || !self.bindings.is_empty()
            || self.opening.is_some()
    }

    pub fn bind(
        &mut self,
        graph: &Graph,
        id: NodeId,
        params: serde_json::Value,
    ) -> Result<(), String> {
        let kind = graph.dag.node(id).map_err(|e| e.to_string())?.metadata().id;
        self.job_ticket += 1;
        self.send(Command::Bind { ticket: self.job_ticket, id, kind, params })?;
        self.bindings.insert(id, self.job_ticket);
        Ok(())
    }
    pub fn open(&mut self, file: std::path::PathBuf) -> Result<(), String> {
        self.job_ticket += 1;
        self.send(Command::Open { ticket: self.job_ticket, file })?;
        self.opening = Some(self.job_ticket);
        Ok(())
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
        self.views.retain(|id, _| graph.node(*id).is_some());
        self.failures.retain(|id, _| graph.node(*id).is_some());
        if let Some(previous) = &self.pending {
            log::trace!(
                "coalesced preview generation={} into generation={}",
                previous.generation,
                self.generation
            );
        }
        self.collect = true;
        self.pending =
            Some(Request { generation: self.generation, graph: graph.clone(), level, targets });
        self.start()
    }

    pub fn reset(&mut self) -> Result<(), String> {
        // Generations never reset, even when node IDs are reused by a project.
        self.generation += 1;
        self.pending = None;
        self.bindings.clear();
        self.opening = None;
        self.views.clear();
        self.views_generation = None;
        self.failures.clear();
        log::debug!("reset preview generation={}", self.generation);
        self.collect = true;
        self.send(Command::Reset)
    }

    pub fn invalidate(&mut self) -> Result<(), String> {
        self.generation += 1;
        self.pending = None;
        log::debug!("invalidating node and resource caches generation={}", self.generation);
        self.send(Command::Invalidate)
    }

    pub fn action(&self, graph: &Graph, id: NodeId, name: &'static str) -> Result<(), String> {
        self.send(Command::Action { graph: graph.clone(), id, name })
    }

    pub fn poll(&mut self) -> Vec<Notice> {
        let mut notices = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Bound { ticket, id, result } => {
                    if self.bindings.get(&id) == Some(&ticket) {
                        self.bindings.remove(&id);
                        notices.push(Notice::Bound { id, result });
                    }
                }
                Event::Opened { ticket, file, result } => {
                    if self.opening == Some(ticket) {
                        self.opening = None;
                        notices.push(Notice::Opened { file, result });
                    }
                }
                Event::Evaluated { generation, level, views, failures } => {
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
                                "preview generation={generation} level={level} node={id:?} kind={}: {}",
                                failure.kind,
                                failure.error
                            );
                        }
                    }
                    let error = failures.iter().next().map(|(id, failure)| {
                        format!("{id:?} ({}): {}", failure.kind, failure.error)
                    });
                    self.failures = failures;
                    self.views = views;
                    self.views_generation = Some(generation);
                    notices.push(Notice::Evaluated(error.map_or(Ok(()), Err)));
                }
                Event::Action(result) => {
                    if !self.failed {
                        notices.push(Notice::Action(result));
                    }
                }
                Event::Stopped => {
                    log::error!("preview worker stopped");
                    notices.push(self.fail());
                }
            }
        }
        if self.start().is_err() && !self.failed {
            log::error!("preview worker disconnected");
            notices.push(self.fail());
        }
        notices
    }

    /// Called after rendering releases its previous callbacks/textures. The
    /// worker holds every prepared allocation until it has no other owners.
    pub fn after_frame(&mut self) {
        if std::mem::take(&mut self.collect) {
            let _ = self.send(Command::Collect);
        }
    }

    fn fail(&mut self) -> Notice {
        self.failed = true;
        self.in_flight = false;
        self.pending = None;
        self.bindings.clear();
        self.opening = None;
        Notice::Failed
    }

    fn start(&mut self) -> Result<(), String> {
        if !self.in_flight
            && let Some(request) = self.pending.take()
        {
            self.send(Command::Evaluate(request))?;
            self.in_flight = true;
        }
        Ok(())
    }

    fn send(&self, command: Command) -> Result<(), String> {
        if self.failed {
            return Err("the preview worker stopped".into());
        }
        self.commands.send(command).map_err(|_| "the preview worker stopped".into())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Do not join a decode or kernel from the UI's shutdown path.
        let _ = self.commands.send(Command::Shutdown);
    }
}

fn serve(commands: mpsc::Receiver<Command>, notify: &Notify) {
    let mut evaluator = None;
    let mut images = ImageCache::default();
    let mut resources = drip::resource::Resources::default();
    while let Ok(command) = commands.recv() {
        match command {
            Command::Bind { ticket, id, kind, params } => {
                notify.send(Event::Bound {
                    ticket,
                    id,
                    result: drip::definition::registered(kind, params),
                });
            }
            Command::Open { ticket, file } => {
                let result =
                    std::fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|text| {
                        crate::model::Project::from_json(&text, &crate::model::Registry)
                            .map_err(|e| e.to_string())
                    });
                notify.send(Event::Opened { ticket, file, result });
            }
            Command::Evaluate(request) => {
                let evaluator = evaluator.get_or_insert_with(|| {
                    drip::runtime::RuntimeContext::from_env().map(Evaluator::new)
                });
                notify.send(evaluate(&request, evaluator.as_ref(), &mut images, &resources));
            }
            Command::Reset | Command::Invalidate => {
                resources = Default::default();
                images.collect();
            }
            Command::Action { graph, id, name } => {
                let notify = notify.clone();
                let resources = resources.clone();
                thread::spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let runtime =
                            drip::runtime::RuntimeContext::from_env().map_err(|e| e.to_string())?;
                        let evaluator = Evaluator::new(runtime);
                        run_action(&evaluator, &graph, id, name, &resources)
                    }))
                    .unwrap_or_else(|_| Err("the action crashed".into()));
                    notify.send(Event::Action(result));
                });
            }
            Command::Collect => images.collect(),
            Command::Shutdown => break,
        }
    }
}

fn evaluate(
    request: &Request,
    evaluator: Result<&Evaluator, &NodeError>,
    images: &mut ImageCache,
    resources: &drip::resource::Resources,
) -> Event {
    let start = Instant::now();
    let trace = log::log_enabled!(log::Level::Trace);
    let mut timings = Vec::new();
    let mut preparation_timings = Vec::new();
    let ctx =
        crate::node_ui::data::PrepareContext { resources: resources.clone(), level: request.level };
    let mut inputs = evaluate_nodes(request, evaluator, trace.then_some(&mut timings));
    let mut failures = BTreeMap::new();
    let views = request
        .targets
        .iter()
        .filter_map(|&id| {
            let node = request.graph.node(id)?;
            let kind = node.kind;
            let result = match inputs.remove(&id) {
                Some(inputs) => inputs.and_then(|inputs| {
                    prepare_view(
                        id,
                        node,
                        &inputs,
                        &ctx,
                        images,
                        trace.then_some(&mut preparation_timings),
                    )
                }),
                None => Ok(None),
            };
            if let Err(error) = &result {
                let (id, kind) = match error {
                    NodeError::Node { node, kind, .. } => (*node, *kind),
                    _ => (id, kind.id),
                };
                failures.insert(id, Failure { kind, error: error.clone() });
            }
            Some((id, result))
        })
        .collect();
    let elapsed = start.elapsed();
    // Emit after evaluation/preparation so log I/O is outside measured work.
    log_timings(request, "evaluate", &timings);
    log_timings(request, "prepare", &preparation_timings);
    log::debug!(
        "preview generation={} level={} elapsed={:.1?} failures={}",
        request.generation,
        request.level,
        elapsed,
        failures.len()
    );
    Event::Evaluated { generation: request.generation, level: request.level, views, failures }
}

fn evaluate_nodes(
    request: &Request,
    evaluator: Result<&Evaluator, &NodeError>,
    timings: Option<&mut Vec<NodeTiming>>,
) -> BTreeMap<NodeId, drip::Result<InputValues>> {
    let targets = &request.targets;
    let evaluator = match evaluator {
        Ok(evaluator) => evaluator,
        Err(error) => return targets.iter().map(|&id| (id, Err(error.clone()))).collect(),
    };
    let viewers: Vec<_> = targets
        .iter()
        .copied()
        .filter(|&id| {
            request
                .graph
                .node(id)
                .and_then(|n| crate::node_ui::binding(n.kind))
                .is_some_and(|b| b.has_preparation())
        })
        .collect();
    let global = drip::runtime::GlobalContext { scale: 1 << request.level };
    evaluator.evaluate_batch_with_timings(&request.graph.dag, &global, targets, &viewers, timings)
}

fn prepare_view(
    id: NodeId,
    node: crate::model::Node,
    inputs: &InputValues,
    ctx: &crate::node_ui::data::PrepareContext,
    images: &mut ImageCache,
    timings: Option<&mut Vec<NodeTiming>>,
) -> ViewResult {
    let Some(binding) = crate::node_ui::binding(node.kind).filter(|b| b.has_preparation()) else {
        return Ok(None);
    };
    let start = timings.as_ref().map(|_| Instant::now());
    let result = binding.prepare(node.params, inputs, ctx, images);
    if let (Some(start), Some(timings)) = (start, timings) {
        timings.push(NodeTiming {
            node: id,
            kind: node.kind.id,
            host_elapsed: start.elapsed(),
            success: result.is_ok(),
        });
    }
    result
}

fn log_timings(request: &Request, phase: &str, timings: &[NodeTiming]) {
    for timing in timings {
        log::trace!(
            "node timing generation={} level={} node={:?} kind={} phase={} host_elapsed={:.3?} success={}",
            request.generation,
            request.level,
            timing.node,
            timing.kind,
            phase,
            timing.host_elapsed,
            timing.success,
        );
    }
}

fn run_action(
    evaluator: &Evaluator,
    graph: &Graph,
    id: NodeId,
    name: &str,
    resources: &drip::resource::Resources,
) -> Result<(), String> {
    let node = graph.node(id).ok_or("action node no longer exists")?;
    let binding = crate::node_ui::binding(node.kind).ok_or("node has no actions")?;
    let inputs = evaluator
        .evaluate_inputs(&graph.dag, &Default::default(), &[id])
        .remove(&id)
        .expect("requested action")
        .map_err(|e| e.to_string())?;
    let ctx = crate::node_ui::data::PrepareContext { resources: resources.clone(), level: 0 };
    binding.run_action(name, node.params, &inputs, &ctx).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use crate::model::Registry;
    use drip::{
        Error as KernelError,
        node::data::{Color, ColorRgb, Extent, ImageDesc},
        param::ParamKind,
        ports::{Cpu, Write},
        runtime::KernelContext,
    };
    use serde::{Deserialize, Serialize};
    fn image_contract<P>(
        _: &drip::runtime::GlobalContext,
        _: &P,
    ) -> drip::Result<(Option<ImageDesc<Color>>,)> {
        Ok((Some(ImageDesc {
            extent: Extent { width: 1, height: 1 },
            interpretation: drip::node::raw::working_color(),
        }),))
    }
    fn empty_contract<P>(_: &drip::runtime::GlobalContext, _: &P) -> drip::Result<()> {
        Ok(())
    }

    use drip::ports::Read;

    use super::*;
    use crate::model::NodeKind;
    use crate::model::Port;
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn backend_configuration_errors_reach_preview_results() {
        let mut graph = Graph::default();
        let id = graph.add_node(Registry.get("view.preview").unwrap()).unwrap();
        let request = Request { generation: 7, graph, level: 2, targets: vec![id] };
        let error = NodeError::Runtime("DRIP_BACKEND=cpu requires --features drip/cpu".into());
        let Event::Evaluated { generation, level, views, failures } =
            evaluate(&request, Err(&error), &mut ImageCache::default(), &Default::default())
        else {
            panic!("expected preview completion");
        };
        assert_eq!((generation, level), (7, 2));
        assert_eq!(views[&id].as_ref().err(), Some(&error));
        assert_eq!(failures[&id].error, error);
    }

    fn graph(source: &'static NodeKind) -> (Graph, NodeId, NodeId, NodeId) {
        let mut graph = Graph::default();
        let source = graph.add_node(source).unwrap();
        let preview = graph.add_node(Registry.get("view.preview").unwrap()).unwrap();
        let histogram = graph.add_node(Registry.get("view.histogram").unwrap()).unwrap();
        for target in [preview, histogram] {
            graph.connect(Port(source, "image".into()), Port(target, "image".into())).unwrap();
        }
        (graph, source, preview, histogram)
    }

    fn wait(worker: &mut Worker) -> Vec<Notice> {
        let deadline = Instant::now() + TIMEOUT;
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

    fn pixel(worker: &Worker, id: NodeId) -> [f32; 4] {
        let prepared = worker.result(id).unwrap().as_ref().unwrap().as_ref().unwrap();
        let image = &prepared.downcast_ref::<crate::node_ui::preview::ImageView>().unwrap().image;
        std::array::from_fn(|c| {
            half::f16::from_bits(u16::from_ne_bytes([image.texels[c * 2], image.texels[c * 2 + 1]]))
                .to_f32()
        })
    }

    #[test]
    fn newest_binding_wins_and_reset_discards_in_flight_loads() {
        let mut graph = Graph::default();
        let id = graph.add_node(Registry.get("rgb-exposure").unwrap()).unwrap();
        let mut worker = Worker::new(|| {});
        worker.bind(&graph, id, json!({"ev":1.0})).unwrap();
        worker.bind(&graph, id, json!({"ev":2.0})).unwrap();
        let notices = wait(&mut worker);
        assert_eq!(notices.len(), 1);
        let Notice::Bound { id: loaded, result } = &notices[0] else { panic!("expected binding") };
        assert_eq!(*loaded, id);
        assert_eq!(result.as_ref().unwrap().parameters().unwrap(), json!({"ev":2.0}));
        worker.bind(&graph, id, json!({"ev":3.0})).unwrap();
        worker.reset().unwrap();
        // A later bind is also a barrier: both discarded work and reset precede it.
        worker.bind(&graph, id, json!({"ev":4.0})).unwrap();
        let notices = wait(&mut worker);
        assert_eq!(notices.len(), 1);
        let Notice::Bound { result, .. } = &notices[0] else { panic!("expected binding") };
        assert_eq!(result.as_ref().unwrap().parameters().unwrap(), json!({"ev":4.0}));
    }

    mod preview_logs_only_changed_root_failures_at_their_severity_fixture {
        use super::*;
        #[derive(Clone, Default, drip::Choice)]
        enum State {
            #[default]
            #[choice("incomplete")]
            Incomplete,
            #[choice("failed")]
            Failed,
            #[choice("ok")]
            Ok,
        }
        #[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
        struct Source {
            #[param(State::Incomplete.schema())]
            state: State,
        }
        #[drip::node(id="test.diagnostics", category="test", name="diagnostics", contract=image_contract)]
        fn diagnostics(
            _: &KernelContext<'_>,
            p: &Source,
            image: Write<'_, Cpu<ColorRgb>>,
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

            let source_kind = Registry.get("test.diagnostics").unwrap();
            let (mut graph, source, preview, histogram) = graph(source_kind);
            let mut worker = Worker::new(|| {});
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
                    assert!(logs[0].1.contains("level=1"));
                } else {
                    assert!(logs.is_empty(), "{logs:?}");
                }
                if state != "ok" {
                    assert!(notices.iter().any(|notice| matches!(notice,
                    Notice::Evaluated(Err(error)) if error.contains("test.diagnostics")
                        && !error.contains("upstream"))));
                }
            }
            worker.reset().unwrap();
            worker.request(&graph, 1, vec![preview]).unwrap();
            wait(&mut worker);
            assert_eq!(
                RECORDS.with_borrow_mut(std::mem::take).len(),
                1,
                "reset clears failure history"
            );

            graph.set_param(source, "state", json!("ok")).unwrap();
            graph.disconnect(&Port(preview, "image".into())).unwrap();
            worker.request(&graph, 1, vec![preview, histogram]).unwrap();
            wait(&mut worker);
            let logs = RECORDS.with_borrow_mut(std::mem::take);
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].0, log::Level::Debug);
            assert!(logs[0].1.contains("missing input"));

            graph.connect(Port(source, "image".into()), Port(preview, "image".into())).unwrap();
            let request =
                Request { generation: 42, graph, level: 1, targets: vec![preview, histogram] };
            let evaluator = Evaluator::new(drip::runtime::RuntimeContext::host());
            for enabled in [true, false] {
                TRACE.set(enabled);
                let Event::Evaluated { failures, .. } = super::super::evaluate(
                    &request,
                    Ok(&evaluator),
                    &mut ImageCache::default(),
                    &Default::default(),
                ) else {
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
                    assert!(logs.iter().all(|line| line.contains("generation=42 level=1")
                        && line.contains("host_elapsed=")
                        && line.contains("success=true")));
                }
            }
            TRACE.set(true);
        }
    }

    mod latest_targets_win_and_project_reset_rejects_old_results_fixture {
        use super::*;

        static GATE: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
        static CALLS: Mutex<Vec<f32>> = Mutex::new(Vec::new());
        #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
        struct SourceKernel {
            #[param(ParamKind::Float { min: 0.0, max: 10.0, default: 1.0 })]
            value: f32,
            #[param(ParamKind::Bool { default: false })]
            block: bool,
        }
        impl Default for SourceKernel {
            fn default() -> Self {
                Self { value: 1.0, block: false }
            }
        }
        #[drip::node(id="test.blocking", category="test", name="blocking", contract=image_contract)]
        fn blocking(
            _: &KernelContext<'_>,
            p: &SourceKernel,
            image: Write<'_, Cpu<ColorRgb>>,
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
        fn latest_targets_win_and_project_reset_rejects_old_results() {
            let source_kind = Registry.get("test.blocking").unwrap();

            let (started, entered) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            *GATE.lock().unwrap() = Some((started, resume));
            let (wake, woke) = mpsc::channel();
            let mut worker = Worker::new(move || {
                let _ = wake.send(());
            });
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
            assert_eq!(worker.pending.as_ref().unwrap().targets, [preview, histogram]);
            release.send(()).unwrap();
            woke.recv_timeout(TIMEOUT).unwrap();
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

            // Invalidation also recomputes nodes with no file dependencies.
            worker.invalidate().unwrap();
            worker.request(&graph, 1, vec![preview, histogram]).unwrap();
            wait(&mut worker);
            assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0, 3.0, 3.0]);

            // Work already running at invalidation cannot restore an old result.
            graph.set_param(source, "block", json!(true)).unwrap();
            worker.request(&graph, 1, vec![preview]).unwrap();
            entered.recv_timeout(TIMEOUT).unwrap();
            worker.request(&graph, 2, vec![preview]).unwrap();
            worker.invalidate().unwrap();
            assert!(worker.pending.is_none());
            release.send(()).unwrap();
            assert!(wait(&mut worker).is_empty(), "invalidated completion is not presented");
            graph.set_param(source, "block", json!(false)).unwrap();
            worker.request(&graph, 1, vec![preview]).unwrap();
            wait(&mut worker);
            assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0, 3.0, 3.0, 3.0, 3.0]);

            graph.set_param(source, "block", json!(true)).unwrap();
            worker.request(&graph, 1, vec![preview]).unwrap();
            entered.recv_timeout(TIMEOUT).unwrap();
            graph.set_param(source, "block", json!(false)).unwrap();
            worker.reset().unwrap();
            assert!(worker.result(histogram).is_none());
            worker.request(&graph, 2, vec![preview]).unwrap();
            release.send(()).unwrap();
            wait(&mut worker);
            assert_eq!(pixel(&worker, preview), [3.0, 1.0, 0.0, 1.0]);
            graph.disconnect(&Port(preview, "image".into())).unwrap();
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

    mod exports_follow_invalidation_order_and_always_use_full_detail_fixture {
        use super::*;

        #[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
        struct FileKernel {
            #[param(ParamKind::Path { output: false })]
            path: Option<std::path::PathBuf>,
        }
        // This fixture deliberately loads during computation to exercise worker ordering.
        // Production RAW sources bind immutable assets before evaluation.
        #[drip::node(id="test.file", category="test", name="file", contract=image_contract)]
        fn file(
            _: &KernelContext<'_>,
            p: &FileKernel,
            image: Write<'_, Cpu<ColorRgb>>,
        ) -> drip::Result<()> {
            let path =
                p.path.as_ref().ok_or_else(|| KernelError::Contract("choose a file".into()))?;
            let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            image.data[0] = [text.parse().unwrap(), 1.0, 0.0];
            Ok(())
        }
        #[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
        struct WriteKernel {
            #[param(ParamKind::Path { output: true })]
            path: Option<std::path::PathBuf>,
        }
        fn sink_contract(
            _: &drip::runtime::GlobalContext,
            _: &WriteKernel,
            _: Option<&ImageDesc<Color>>,
        ) -> drip::Result<()> {
            Ok(())
        }
        #[drip::node(id="test.write", category="test", name="write", contract=sink_contract)]
        fn write(
            _: &KernelContext<'_>,
            _: &WriteKernel,
            image: Read<'_, Cpu<ColorRgb>>,
        ) -> drip::Result<()> {
            let _ = image;
            Ok(())
        }
        struct Writer;
        #[drip_macros::gui_node]
        impl crate::node_ui::GuiNode for Writer {
            type Parameters = WriteKernel;
            type Presentation = ();
            const ID: &'static str = "test.write";
            const ACTION_NAME: &'static str = "write";
            const ACTION: Option<crate::node_ui::Action<Self>> = Some(|p, inputs, ctx| {
                assert_eq!(ctx.level, 0);
                let (_, image) = inputs.get("image").unwrap().get::<Cpu<ColorRgb>>()?;
                std::fs::write(p.path.as_ref().unwrap(), format!("{:?}", image[0]))
                    .map_err(|e| KernelError::Runtime(e.to_string()))
            });
        }

        #[test]
        fn exports_follow_invalidation_order_and_always_use_full_detail() {
            let input =
                std::env::temp_dir().join(format!("drip-worker-input-{}", std::process::id()));
            let output = input.with_extension("out");
            std::fs::write(&input, "1").unwrap();
            let (mut graph, source, preview, _) = graph(Registry.get("test.file").unwrap());
            let export = graph.add_node(Registry.get("test.write").unwrap()).unwrap();
            graph.connect(Port(source, "image".into()), Port(export, "image".into())).unwrap();
            graph.set_param(source, "path", json!(input)).unwrap();
            graph.set_param(export, "path", json!(output)).unwrap();
            let mut worker = Worker::new(|| {});
            worker.request(&graph, 3, vec![preview]).unwrap();
            wait(&mut worker);
            let before = pixel(&worker, preview);
            std::fs::write(&input, "2").unwrap();
            for (invalidate, expected) in [(false, "[2.0, 1.0, 0.0]"), (true, "[2.0, 1.0, 0.0]")] {
                if invalidate {
                    worker.invalidate().unwrap();
                }
                worker.action(&graph, export, "write").unwrap();
                let deadline = Instant::now() + TIMEOUT;
                loop {
                    if worker.poll().into_iter().any(|notice| match notice {
                        Notice::Action(result) => {
                            result.unwrap();
                            true
                        }
                        _ => false,
                    }) {
                        break;
                    }
                    assert!(Instant::now() < deadline);
                    thread::sleep(Duration::from_millis(1));
                }
                assert_eq!(std::fs::read_to_string(&output).unwrap(), expected);
                assert_eq!(pixel(&worker, preview), before, "export leaves preview intact");
            }
            worker.request(&graph, 3, vec![preview]).unwrap();
            wait(&mut worker);
            assert_eq!(pixel(&worker, preview), [2.0, 1.0, 0.0, 1.0]);
            std::fs::write(&input, "3").unwrap();
            worker.reset().unwrap();
            worker.request(&graph, 3, vec![preview]).unwrap();
            wait(&mut worker);
            assert_eq!(pixel(&worker, preview), [3.0, 1.0, 0.0, 1.0]);
            graph.set_param(preview, "mode", json!("softproof")).unwrap();
            graph.set_param(preview, "profile", json!("file")).unwrap();
            graph
                .set_param(preview, "profile_file", json!(input.with_extension("missing.icc")))
                .unwrap();
            worker.request(&graph, 3, vec![preview]).unwrap();
            wait(&mut worker);
            assert!(worker.result(preview).unwrap().is_err());
            worker.action(&graph, export, "write").unwrap();
            let deadline = Instant::now() + TIMEOUT;
            loop {
                if worker.poll().into_iter().any(|notice| match notice {
                    Notice::Action(result) => {
                        result.unwrap();
                        true
                    }
                    _ => false,
                }) {
                    break;
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
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
            #[param(ParamKind::Bool { default: true })]
            fail: bool,
        }

        impl Default for Settings {
            fn default() -> Self {
                Self { fail: true }
            }
        }
        #[drip::node(id="test.preparation_cache", category="test", name="Preparation", contract=empty_contract)]
        fn probe(_: &KernelContext<'_>, _: &Settings) -> drip::Result<()> {
            Ok(())
        }

        struct ProbeGui;
        impl crate::node_ui::GuiNode for ProbeGui {
            type Parameters = Settings;
            type Presentation = ();
            const ID: &'static str = "test.preparation_cache";
            const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|settings, _, _| {
                CALLS.fetch_add(1, Ordering::Relaxed);
                if settings.fail { Err("preparation failed".into()) } else { Ok(()) }
            });
        }
        #[linkme::distributed_slice(crate::node_ui::BINDINGS)]
        static UI: crate::node_ui::Binding = crate::node_ui::Binding::new::<ProbeGui>();
    }

    #[test]
    fn preparation_retries_each_request_without_persistent_image_cache() {
        use preparation_probe::CALLS;
        use std::sync::atomic::Ordering;

        let mut graph = Graph::default();
        let id = graph.add_node(Registry.get("test.preparation_cache").unwrap()).unwrap();
        let mut worker = Worker::new(|| {});
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
        worker.invalidate().unwrap();
        worker.request(&graph, 2, vec![id]).unwrap();
        wait(&mut worker);
        assert_eq!(CALLS.load(Ordering::Relaxed), 6);
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
            let id = graph.add_node(Registry.get("test.panic").unwrap()).unwrap();
            let (wake, woke) = mpsc::channel();
            let mut worker = Worker::new(move || {
                let _ = wake.send(());
            });
            worker.request(&graph, 0, vec![id]).unwrap();
            woke.recv_timeout(TIMEOUT).unwrap();
            assert!(worker.poll().iter().any(|notice| matches!(notice, Notice::Failed)));
            assert!(!worker.busy());
            assert!(worker.request(&graph, 0, vec![id]).is_err());
            assert!(!worker.busy());
        }
    }
}
