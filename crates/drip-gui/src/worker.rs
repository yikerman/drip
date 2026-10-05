//! Background evaluation of explicit targets at one global preview level.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::thread;

use drip::eval::{Evaluator, NodeError};
use drip::graph::{Graph, NodeId};
use drip::value::{Histogram, Rgb, View};

/// Pixels are packed on the worker; the renderer only uploads them.
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub texels: Vec<u8>,
}

#[derive(Clone)]
pub enum PreparedView {
    Image(Arc<Image>),
    Histogram(Arc<Histogram>),
}

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
    Reset(Graph),
    Reload(Vec<PathBuf>),
    Action { graph: Graph, id: NodeId, name: &'static str },
    Collect,
    Shutdown,
}

enum Event {
    Evaluated { generation: u64, views: Snapshot },
    Action(Result<(), String>),
    Stopped,
}

pub enum Notice {
    Evaluated(Result<(), String>),
    Action(Result<(), String>),
    Failed,
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
        self.collect = true;
        self.pending =
            Some(Request { generation: self.generation, graph: graph.clone(), level, targets });
        self.start()
    }

    pub fn reset(&mut self, graph: &Graph) -> Result<(), String> {
        // Generations never reset, even when node IDs are reused by a project.
        self.generation += 1;
        self.pending = None;
        self.views.clear();
        self.collect = true;
        self.send(Command::Reset(graph.clone()))
    }

    pub fn reload(&self, paths: Vec<PathBuf>) -> Result<(), String> {
        self.send(Command::Reload(paths))
    }

    pub fn action(&self, graph: &Graph, id: NodeId, name: &'static str) -> Result<(), String> {
        self.send(Command::Action { graph: graph.clone(), id, name })
    }

    pub fn poll(&mut self) -> Vec<Notice> {
        let mut notices = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Evaluated { generation, views } => {
                    self.in_flight = false;
                    self.collect = true;
                    if generation == self.generation {
                        let error = views
                            .values()
                            .find_map(|view| view.as_ref().err().map(ToString::to_string));
                        // Keep off-screen nodes' last presentation: losing their
                        // view would change layout and hence the next target set.
                        self.views.extend(views);
                        notices.push(Notice::Evaluated(error.map_or(Ok(()), Err)));
                    }
                }
                Event::Action(result) => {
                    if !self.failed {
                        notices.push(Notice::Action(result));
                    }
                }
                Event::Stopped => {
                    self.failed = true;
                    self.in_flight = false;
                    self.pending = None;
                    notices.push(Notice::Failed);
                }
            }
        }
        if self.start().is_err() && !self.failed {
            self.failed = true;
            self.in_flight = false;
            self.pending = None;
            notices.push(Notice::Failed);
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

#[derive(Default)]
struct Prepared(Vec<(Arc<Rgb>, Arc<Image>)>);

impl Prepared {
    fn view(&mut self, view: &View) -> PreparedView {
        match view {
            View::Histogram(histogram) => PreparedView::Histogram(histogram.clone()),
            View::Image(value) => {
                let source = value.rgb();
                if let Some((_, image)) = self.0.iter().find(|(rgb, _)| Arc::ptr_eq(rgb, source)) {
                    return PreparedView::Image(image.clone());
                }
                let texels = source
                    .pixels
                    .iter()
                    .flat_map(|&[r, g, b]| [r, g, b, 1.0])
                    .flat_map(|v| half::f16::from_f32(v).to_ne_bytes())
                    .collect();
                let image = Arc::new(Image { width: source.width, height: source.height, texels });
                self.0.push((source.clone(), image.clone()));
                PreparedView::Image(image)
            }
        }
    }

    fn collect(&mut self) {
        self.0.retain(|(_, image)| Arc::strong_count(image) > 1);
    }
}

fn serve(commands: mpsc::Receiver<Command>, notify: &Notify) {
    let mut evaluator = Evaluator::default();
    let mut prepared = Prepared::default();
    for command in commands {
        match command {
            Command::Evaluate(request) => {
                evaluator.evaluate(&request.graph, request.level, &request.targets);
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
                notify.send(Event::Evaluated { generation: request.generation, views });
            }
            Command::Reset(graph) => evaluator = evaluator.fork(&graph),
            Command::Reload(paths) => {
                for path in paths {
                    evaluator.reload(&path);
                }
            }
            Command::Action { graph, id, name } => {
                let evaluator = evaluator.fork(&graph);
                let notify = notify.clone();
                thread::spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        evaluator.run_action(&graph, id, name).map_err(|e| e.to_string())
                    }))
                    .unwrap_or_else(|_| Err("the action crashed".into()));
                    notify.send(Event::Action(result));
                });
            }
            Command::Collect => prepared.collect(),
            Command::Shutdown => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drip::graph::Port;
    use drip::node::{Action, Evaluated, InputSpec, NodeKind, OutputSpec};
    use drip::nodes;
    use drip::param::{ParamKind, ParamSpec};
    use drip::value::{PortType, Value};
    use serde_json::json;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(5);

    fn image(value: f32, scale: u32) -> Evaluated {
        Evaluated {
            outputs: vec![Value::DisplayRec2020(Arc::new(Rgb {
                width: 1,
                height: 1,
                scale,
                pixels: vec![[value, scale as f32, 0.0]],
            }))],
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
        let Some(PreparedView::Image(image)) = worker.result(id).unwrap().as_ref().unwrap() else {
            panic!("image");
        };
        std::array::from_fn(|c| {
            half::f16::from_bits(u16::from_ne_bytes([image.texels[c * 2], image.texels[c * 2 + 1]]))
                .to_f32()
        })
    }

    #[test]
    fn latest_targets_win_and_project_reset_rejects_old_results() {
        static GATE: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
        static CALLS: Mutex<Vec<f32>> = Mutex::new(Vec::new());
        static SOURCE: NodeKind = NodeKind {
            name: "test.blocking",
            label: "blocking",
            params: &[
                ParamSpec::new("value", ParamKind::Float { min: 0.0, max: 10.0, default: 1.0 }),
                ParamSpec::new("block", ParamKind::Bool { default: false }),
            ],
            inputs: &[],
            outputs: &[OutputSpec { name: "image", ty: PortType::DisplayRec2020 }],
            eval: |p, _, ctx| {
                let value = p.float("value") as f32;
                CALLS.lock().unwrap().push(value);
                if p.bool("block") {
                    let gate = GATE.lock().unwrap();
                    let (started, release) = gate.as_ref().unwrap();
                    started.send(()).unwrap();
                    release.recv_timeout(TIMEOUT).unwrap();
                }
                Ok(image(value, ctx.scale()))
            },
            actions: &[],
        };
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

        // A new target set reuses computation and keeps off-screen layout stable.
        worker.request(&graph, 1, vec![histogram]).unwrap();
        wait(&mut worker);
        assert!(worker.result(preview).is_some());
        assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0]);

        graph.set_param(source, "block", json!(true)).unwrap();
        worker.request(&graph, 1, vec![preview]).unwrap();
        entered.recv_timeout(TIMEOUT).unwrap();
        graph.set_param(source, "block", json!(false)).unwrap();
        worker.reset(&graph).unwrap();
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
    fn exports_follow_reload_order_and_always_use_full_detail() {
        static FILE: NodeKind = NodeKind {
            name: "test.file",
            label: "file",
            params: &[ParamSpec::new("path", ParamKind::Path { output: false })],
            inputs: &[],
            outputs: &[OutputSpec { name: "image", ty: PortType::DisplayRec2020 }],
            eval: |p, _, ctx| {
                let value = ctx.resources().load(p.path("path").unwrap(), |path| {
                    std::fs::read_to_string(path).map_err(|e| e.to_string())
                })?;
                Ok(image(value.parse().unwrap(), ctx.scale()))
            },
            actions: &[],
        };
        static WRITE: NodeKind = NodeKind {
            name: "test.write",
            label: "write",
            params: &[ParamSpec::new("path", ParamKind::Path { output: true })],
            inputs: &[InputSpec { name: "image", accepts: &[PortType::DisplayRec2020] }],
            outputs: &[],
            eval: |_, _, _| Ok(Evaluated::default()),
            actions: &[Action {
                name: "write",
                run: |p, inputs, ctx| {
                    assert_eq!(ctx.scale(), 1);
                    std::fs::write(
                        p.path("path").unwrap(),
                        format!("{:?}", inputs[0].rgb().pixels[0]),
                    )
                    .map_err(|e| e.to_string())
                },
            }],
        };
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
        for (reload, expected) in [(false, "[1.0, 1.0, 0.0]"), (true, "[2.0, 1.0, 0.0]")] {
            if reload {
                worker.reload(vec![input.clone()]).unwrap();
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
        std::fs::remove_file(input).unwrap();
        std::fs::remove_file(output).unwrap();
    }

    #[test]
    fn preparation_reuses_images_and_keeps_destruction_on_its_owner() {
        let source =
            Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[-1.0, 0.5, 2.0]] });
        let raw = Arc::downgrade(&source);
        let view = View::Image(Value::DisplayRec2020(source));
        let mut prepared = Prepared::default();
        let PreparedView::Image(first) = prepared.view(&view) else { panic!("image") };
        let PreparedView::Image(second) = prepared.view(&view) else { panic!("image") };
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            first.texels,
            [0xbc00u16, 0x3800, 0x4000, 0x3c00]
                .into_iter()
                .flat_map(u16::to_ne_bytes)
                .collect::<Vec<_>>()
        );
        let pixels = Arc::downgrade(&first);
        drop(view);
        drop(first);
        prepared.collect();
        assert!(raw.upgrade().is_some(), "renderer still owns the prepared image");
        drop(second);
        assert!(pixels.upgrade().is_some(), "the worker owns final destruction");
        prepared.collect();
        assert!(pixels.upgrade().is_none() && raw.upgrade().is_none());
    }

    #[test]
    fn panic_wakes_the_ui_and_ends_the_busy_state() {
        static PANIC: NodeKind = NodeKind {
            name: "test.panic",
            label: "panic",
            params: &[],
            inputs: &[],
            outputs: &[],
            eval: |_, _, _| panic!("test worker failure"),
            actions: &[],
        };
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
