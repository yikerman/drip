//! Background evaluation of explicit targets at one global preview level.

use std::collections::BTreeMap;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Instant;

use drip::eval::{Evaluator, NodeError};
use drip::graph::{Graph, NodeId};
use drip::param::ParamKind;

use crate::render::node_views::{Prepared, PreparedView};

pub type Presentation = Result<Option<PreparedView>, NodeError>;
type Snapshot = BTreeMap<NodeId, Presentation>;

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
}

enum Event {
    Evaluated { generation: u64, level: u8, views: Snapshot, failures: BTreeMap<NodeId, Failure> },
    Action(Result<(), String>),
    Stopped,
}

pub enum Notice {
    Evaluated(Result<(), String>),
    Action(Result<(), String>),
    Failed,
}

#[derive(Clone, PartialEq)]
struct Failure {
    kind: &'static str,
    error: NodeError,
}

impl Failure {
    fn level(&self) -> log::Level {
        match self.error {
            NodeError::MissingInput(_) | NodeError::Incomplete(_) => log::Level::Debug,
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
    failures: BTreeMap<NodeId, Failure>,
    collect: bool,
    failed: bool,
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
            failures: BTreeMap::new(),
            collect: false,
            failed: false,
        }
    }

    pub fn result(&self, id: NodeId) -> Option<&Presentation> {
        self.views.get(&id)
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
        self.views.clear();
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
    let mut evaluator = Evaluator::default();
    let mut prepared = Prepared::default();
    for command in commands {
        match command {
            Command::Evaluate(request) => {
                notify.send(evaluate(&request, &mut evaluator, &mut prepared));
            }
            Command::Reset | Command::Invalidate => evaluator = Evaluator::default(),
            Command::Action { graph, id, name } => {
                let evaluator = evaluator.fork();
                let notify = notify.clone();
                thread::spawn(move || {
                    notify.send(Event::Action(run_action(evaluator, &graph, id, name)));
                });
            }
            Command::Collect => prepared.collect(),
            Command::Shutdown => break,
        }
    }
}

fn evaluate(request: &Request, evaluator: &mut Evaluator, prepared: &mut Prepared) -> Event {
    let start = Instant::now();
    log::debug!(
        "preview started generation={} level={} targets={}",
        request.generation,
        request.level,
        request.targets.len()
    );
    let computed = evaluator.evaluate(&request.graph, request.level, &request.targets);
    let evaluated = start.elapsed();
    let failures: BTreeMap<_, _> = evaluator
        .failures(&request.graph, &request.targets)
        .map(|(id, error)| {
            (
                id,
                Failure {
                    kind: request.graph.node(id).expect("evaluated node").kind.id,
                    error: error.clone(),
                },
            )
        })
        .collect();
    let views = request
        .targets
        .iter()
        .map(|&id| {
            let result = evaluator
                .result(id)
                .expect("evaluated target")
                .as_ref()
                .map(|result| result.view.as_ref().map(|view| prepared.view(view)))
                .map_err(Clone::clone);
            (id, result)
        })
        .collect();
    log::debug!(
        "preview finished generation={} level={} recomputed={} failures={} evaluate={evaluated:.1?} prepare={:.1?}",
        request.generation,
        request.level,
        computed.len(),
        failures.len(),
        start.elapsed() - evaluated
    );
    Event::Evaluated { generation: request.generation, level: request.level, views, failures }
}

fn run_action(evaluator: Evaluator, graph: &Graph, id: NodeId, name: &str) -> Result<(), String> {
    let node = graph.node(id).expect("action node");
    let destinations: Vec<_> = node
        .kind
        .params
        .iter()
        .filter(|p| matches!(p.kind, ParamKind::Path { output: true }))
        .map(|p| (&p.name, &node.params[p.name]))
        .collect();
    let context =
        format!("node={id:?} kind={} action={name} destinations={destinations:?}", node.kind.id);
    let start = Instant::now();
    log::info!("action started {context}");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        evaluator.run_action(graph, id, name).map_err(|e| e.to_string())
    }))
    .unwrap_or_else(|_| Err("the action crashed".into()));
    match &result {
        Ok(()) => {
            log::info!("action completed {context} elapsed={:.1?}", start.elapsed())
        }
        Err(error) => {
            log::error!("action failed {context} elapsed={:.1?}: {error}", start.elapsed())
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use drip::image::{DisplayRec2020, ThreeChannelMatrix};
    use drip::node::{EvalContext, KernelError, NodeKernel, TypedAction};
    use drip::param::Params;
    use drip::ports::Read;

    use super::*;
    use drip::graph::Port;
    use drip::image::Rgb;
    use drip::node::{Evaluated, NodeKind};
    use drip::nodes;
    use drip::param::{ParamKind, ParamSpec};
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(5);

    fn image(value: f32, scale: u32) -> Evaluated<(Arc<DisplayRec2020>,)> {
        Evaluated {
            outputs: (Arc::new(DisplayRec2020::from(Arc::new(Rgb {
                width: 1,
                height: 1,
                scale,
                pixels: vec![[value, scale as f32, 0.0]],
            }))),),
            view: None,
        }
    }

    fn graph(source: &'static NodeKind) -> (Graph, NodeId, NodeId, NodeId) {
        let mut graph = Graph::default();
        let source = graph.add_node(source);
        let preview = graph.add_node(&nodes::PREVIEW);
        let histogram = graph.add_node(&nodes::HISTOGRAM);
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
        let Some(PreparedView::Image(image, _)) = worker.result(id).unwrap().as_ref().unwrap()
        else {
            panic!("image");
        };
        std::array::from_fn(|c| {
            half::f16::from_bits(u16::from_ne_bytes([image.texels[c * 2], image.texels[c * 2 + 1]]))
                .to_f32()
        })
    }

    #[test]
    fn preview_logs_only_changed_root_failures_at_their_severity() {
        use std::cell::RefCell;
        thread_local! {
            static RECORDS: RefCell<Vec<(log::Level, String)>> = const { RefCell::new(Vec::new()) };
        }
        struct Capture;
        impl log::Log for Capture {
            fn enabled(&self, _: &log::Metadata<'_>) -> bool {
                true
            }
            fn log(&self, record: &log::Record<'_>) {
                let text = record.args().to_string();
                if text.starts_with("preview generation=") {
                    RECORDS.with_borrow_mut(|records| records.push((record.level(), text)));
                }
            }
            fn flush(&self) {}
        }
        log::set_logger(&Capture).unwrap();
        log::set_max_level(log::LevelFilter::Trace);

        static SOURCE: NodeKind = NodeKind::new::<Source>(
            "test.diagnostics",
            "test",
            "diagnostics",
            &[ParamSpec::new(
                "state",
                ParamKind::Choice {
                    options: &["incomplete", "failed", "ok"],
                    default: "incomplete",
                },
            )],
            &[],
            &["image"],
        );
        struct Source;
        impl NodeKernel for Source {
            type Inputs = ();
            type Outputs = (Arc<DisplayRec2020>,);
            fn eval(
                p: Params<'_>,
                (): (),
                _: &EvalContext<'_>,
            ) -> Result<Evaluated<Self::Outputs>, KernelError> {
                match p.choice("state") {
                    "incomplete" => Err(KernelError::Incomplete("choose a file")),
                    "failed" => Err("decode failed".into()),
                    _ => Ok(image(1.0, 1)),
                }
            }
        }
        let (mut graph, source, preview, histogram) = graph(&SOURCE);
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
        graph.disconnect(&Port(preview, "image".into()));
        worker.request(&graph, 1, vec![preview, histogram]).unwrap();
        wait(&mut worker);
        let logs = RECORDS.with_borrow_mut(std::mem::take);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, log::Level::Debug);
        assert!(logs[0].1.contains("not connected"));
    }

    #[test]
    fn latest_targets_win_and_project_reset_rejects_old_results() {
        static GATE: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
        static CALLS: Mutex<Vec<f32>> = Mutex::new(Vec::new());
        static SOURCE: NodeKind = NodeKind::new::<SourceKernel>(
            "test.blocking",
            "test",
            "blocking",
            &[
                ParamSpec::new("value", ParamKind::Float { min: 0.0, max: 10.0, default: 1.0 }),
                ParamSpec::new("block", ParamKind::Bool { default: false }),
            ],
            &[],
            &["image"],
        );
        struct SourceKernel;
        impl NodeKernel for SourceKernel {
            type Inputs = ();
            type Outputs = (Arc<DisplayRec2020>,);

            fn eval(
                p: Params<'_>,
                (): (),
                ctx: &EvalContext<'_>,
            ) -> Result<Evaluated<Self::Outputs>, KernelError> {
                let value = p.float("value") as f32;
                CALLS.lock().unwrap().push(value);
                if p.bool("block") {
                    let gate = GATE.lock().unwrap();
                    let (started, release) = gate.as_ref().unwrap();
                    started.send(()).unwrap();
                    release.recv_timeout(TIMEOUT).unwrap();
                }
                Ok(image(value, ctx.scale()))
            }
        }

        let (started, entered) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        *GATE.lock().unwrap() = Some((started, resume));
        let (wake, woke) = mpsc::channel();
        let mut worker = Worker::new(move || {
            let _ = wake.send(());
        });
        let (mut graph, source, preview, histogram) = graph(&SOURCE);
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
        assert_eq!(pixel(&worker, preview), [3.0, 2.0, 0.0, 1.0]);
        assert!(worker.result(histogram).is_some());

        // A new target set reuses computation and replaces the presentation.
        worker.request(&graph, 1, vec![histogram]).unwrap();
        wait(&mut worker);
        assert!(worker.result(preview).is_none());
        assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0]);

        // Invalidation also recomputes nodes with no file dependencies.
        worker.invalidate().unwrap();
        worker.request(&graph, 1, vec![preview, histogram]).unwrap();
        wait(&mut worker);
        assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0, 3.0]);

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
        assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0, 3.0, 3.0, 3.0]);

        graph.set_param(source, "block", json!(true)).unwrap();
        worker.request(&graph, 1, vec![preview]).unwrap();
        entered.recv_timeout(TIMEOUT).unwrap();
        graph.set_param(source, "block", json!(false)).unwrap();
        worker.reset().unwrap();
        assert!(worker.result(histogram).is_none());
        worker.request(&graph, 2, vec![preview]).unwrap();
        release.send(()).unwrap();
        wait(&mut worker);
        assert_eq!(pixel(&worker, preview), [3.0, 4.0, 0.0, 1.0]);
        graph.disconnect(&Port(preview, "image".into()));
        worker.request(&graph, 2, vec![preview]).unwrap();
        assert!(wait(&mut worker).iter().any(|notice| matches!(notice, Notice::Evaluated(Err(_)))));
        assert!(worker.result(preview).unwrap().is_err(), "a current error replaces the old image");
    }

    #[test]
    fn exports_follow_invalidation_order_and_always_use_full_detail() {
        static FILE: NodeKind = NodeKind::new::<FileKernel>(
            "test.file",
            "test",
            "file",
            &[ParamSpec::new("path", ParamKind::Path { output: false })],
            &[],
            &["image"],
        );
        struct FileKernel;
        impl NodeKernel for FileKernel {
            type Inputs = ();
            type Outputs = (Arc<DisplayRec2020>,);

            fn eval(
                p: Params<'_>,
                (): (),
                ctx: &EvalContext<'_>,
            ) -> Result<Evaluated<Self::Outputs>, KernelError> {
                let value = ctx.resources().load(p.path("path").unwrap(), |path| {
                    std::fs::read_to_string(path).map_err(|e| e.to_string())
                })?;
                Ok(image(value.parse().unwrap(), ctx.scale()))
            }
        }

        static WRITE: NodeKind = NodeKind::new::<WriteKernel>(
            "test.write",
            "test",
            "write",
            &[ParamSpec::new("path", ParamKind::Path { output: true })],
            &["image"],
            &[],
        );
        struct WriteKernel;
        impl NodeKernel for WriteKernel {
            type Inputs = (Read<DisplayRec2020>,);
            type Outputs = ();
            const ACTIONS: &'static [TypedAction<Self>] = &[TypedAction {
                name: "write",
                run: |p, (input0,), ctx| {
                    assert_eq!(ctx.scale(), 1);
                    std::fs::write(p.path("path").unwrap(), format!("{:?}", input0.rgb().pixels[0]))
                        .map_err(|e| KernelError::Failed(e.to_string()))
                },
            }];
            fn eval(
                _: Params<'_>,
                _: (&DisplayRec2020,),
                _: &EvalContext<'_>,
            ) -> Result<Evaluated<Self::Outputs>, KernelError> {
                Ok(Evaluated::default())
            }
        }

        let input = std::env::temp_dir().join(format!("drip-worker-input-{}", std::process::id()));
        let output = input.with_extension("out");
        std::fs::write(&input, "1").unwrap();
        let (mut graph, source, preview, _) = graph(&FILE);
        let export = graph.add_node(&WRITE);
        graph.connect(Port(source, "image".into()), Port(export, "image".into())).unwrap();
        graph.set_param(source, "path", json!(input)).unwrap();
        graph.set_param(export, "path", json!(output)).unwrap();
        let mut worker = Worker::new(|| {});
        worker.request(&graph, 3, vec![preview]).unwrap();
        wait(&mut worker);
        let before = pixel(&worker, preview);
        std::fs::write(&input, "2").unwrap();
        for (invalidate, expected) in [(false, "[1.0, 1.0, 0.0]"), (true, "[2.0, 1.0, 0.0]")] {
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
        assert_eq!(pixel(&worker, preview), [2.0, 8.0, 0.0, 1.0]);
        std::fs::write(&input, "3").unwrap();
        worker.reset().unwrap();
        worker.request(&graph, 3, vec![preview]).unwrap();
        wait(&mut worker);
        assert_eq!(pixel(&worker, preview), [3.0, 8.0, 0.0, 1.0]);
        std::fs::remove_file(input).unwrap();
        std::fs::remove_file(output).unwrap();
    }

    #[test]
    fn panic_wakes_the_ui_and_ends_the_busy_state() {
        static PANIC: NodeKind =
            NodeKind::new::<PanicKernel>("test.panic", "test", "panic", &[], &[], &[]);
        struct PanicKernel;
        impl NodeKernel for PanicKernel {
            type Inputs = ();
            type Outputs = ();

            fn eval(
                _: Params<'_>,
                (): (),
                _: &EvalContext<'_>,
            ) -> Result<Evaluated<Self::Outputs>, KernelError> {
                panic!("test worker failure")
            }
        }

        let mut graph = Graph::default();
        let id = graph.add_node(&PANIC);
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
