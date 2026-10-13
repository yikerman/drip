use drip::{
    Error as KernelError,
    node::data::{Coordinates, Extent, REC2020_D65, Rgb, RgbDesc},
    ports::{Publish, Write},
    runtime::KernelContext,
};
use serde::{Deserialize, Serialize};
fn image_contract<P>(_: &drip::runtime::GlobalContext, _: &P) -> drip::Result<(RgbDesc,)> {
    Ok((RgbDesc::new(Extent { width: 1, height: 1 }, Coordinates::Space(REC2020_D65)),))
}
fn empty_contract<P>(_: &drip::runtime::GlobalContext, _: &P) -> drip::Result<()> {
    Ok(())
}

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
    let id = graph.add_node(&crate::node_ui::preview::observe::KIND).unwrap();
    let demand = preview(7, graph, 2, vec![id]);
    let error = NodeError::Runtime("DRIP_BACKEND=cpu requires --features drip/cpu".into());
    let Event::Evaluated { generation, views, failures, .. } =
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
    let preview = graph.add_node(&crate::node_ui::preview::observe::KIND).unwrap();
    let histogram = graph.add_node(&crate::node_ui::scopes::histogram::observe::KIND).unwrap();
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
    #[drip::node(id="test-diagnostics", name="diagnostics", contract=image_contract)]
    fn diagnostics(
        _: &KernelContext<'_>,
        p: &Source,
        mut image: Write<'_, Rgb>,
    ) -> drip::Result<()> {
        match p.state {
            State::Incomplete => Err(KernelError::Contract("choose a file".into())),
            State::Failed => Err(KernelError::Runtime("decode failed".into())),
            State::Ok => {
                image.host()[0] = [1., 1., 0.];
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

        let source_kind = &diagnostics::KIND;
        let (mut graph, source, preview, histogram) = graph(source_kind);
        let mut worker = Worker::new(Arc::new(|| {}), None);
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
                assert!(logs[0].1.contains(&format!("node={source:?} kind=test-diagnostics")));
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
        worker = Worker::new(Arc::new(|| {}), None);
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
                assert_eq!(logs.iter().filter(|line| line.contains("phase=evaluate")).count(), 3);
                assert_eq!(logs.iter().filter(|line| line.contains("phase=prepare")).count(), 2);
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
    #[drip::node(id="test-blocking", name="blocking", contract=image_contract)]
    fn blocking(
        _: &KernelContext<'_>,
        p: &SourceKernel,
        mut image: Write<'_, Rgb>,
    ) -> drip::Result<()> {
        CALLS.lock().unwrap().push(p.value);
        if p.block {
            let gate = GATE.lock().unwrap();
            let (started, release) = gate.as_ref().unwrap();
            started.send(()).unwrap();
            release.recv_timeout(TIMEOUT).unwrap();
        }
        image.host()[0] = [p.value, 1.0, 0.0];
        Ok(())
    }

    #[test]
    fn latest_targets_win_and_a_replaced_worker_skips_queued_work() {
        let source_kind = &blocking::KIND;

        let (started, entered) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        *GATE.lock().unwrap() = Some((started, resume));
        let (wake, woke) = mpsc::channel();
        let mut worker = Worker::new(
            Arc::new(move || {
                let _ = wake.send(());
            }),
            None,
        );
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

        // A new target set replaces the presentation. The evaluator's store
        // still holds the source's output, so the source does not run again.
        worker.request(&graph, 1, vec![histogram]).unwrap();
        wait(&mut worker);
        assert!(worker.result(preview).is_none());
        assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0]);

        // A dropped worker finishes its current command, then skips its
        // queue: here an export that would evaluate the source again.
        let tiff = graph.add_node(&drip::node::export::tiff::KIND).unwrap();
        graph
            .connect(
                graph.dag.output_port(source, "image").unwrap(),
                graph.dag.input_port(tiff, "image").unwrap(),
            )
            .unwrap();
        let (wake, woke) = mpsc::channel();
        let mut old = Worker::new(
            Arc::new(move || {
                let _ = wake.send(());
            }),
            None,
        );
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
        assert_eq!(*CALLS.lock().unwrap(), [1.0, 3.0, 3.0]);
        worker.request(&graph, 2, vec![preview]).unwrap();
        wait(&mut worker);
        assert_eq!(pixel(&worker, preview), [4.0, 1.0, 0.0, 1.0]);
        graph.disconnect(graph.dag.input_port(preview, "image").unwrap()).unwrap();
        worker.request(&graph, 2, vec![preview]).unwrap();
        assert!(wait(&mut worker).iter().any(|notice| matches!(notice, Notice::Evaluated(Err(_)))));
        assert!(worker.result(preview).unwrap().is_err(), "a current error replaces the old image");
    }
}

mod exports_read_current_files_at_full_detail_fixture {
    use super::*;
    use crate::test_nodes::write;

    #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
    struct FileKernel {
        #[param(open = [])]
        path: Option<std::path::PathBuf>,
    }
    // Reads its file whenever it runs, unlike a foreign node, which stamps
    // its files. The store keys the output by path only, since the kernel
    // does not claim the scale, so only a new path reads the file again.
    #[drip::node(id="test-file", name="file", contract=image_contract)]
    fn file(_: &KernelContext<'_>, p: &FileKernel, mut image: Write<'_, Rgb>) -> drip::Result<()> {
        let path = p.path.as_ref().ok_or_else(|| KernelError::Contract("choose a file".into()))?;
        let text =
            std::fs::read_to_string(path).map_err(|e| KernelError::Runtime(e.to_string()))?;
        image.host()[0] = [text.parse().unwrap(), 1.0, 0.0];
        Ok(())
    }
    #[test]
    fn exports_read_current_files_at_full_detail() {
        let input = std::env::temp_dir().join(format!("drip-worker-input-{}", std::process::id()));
        let output = input.with_extension("out");
        std::fs::write(&input, "1").unwrap();
        let (mut graph, source, preview, _) = graph(&file::KIND);
        let export = graph.add_node(&write::KIND).unwrap();
        graph
            .connect(
                graph.dag.output_port(source, "image").unwrap(),
                graph.dag.input_port(export, "image").unwrap(),
            )
            .unwrap();
        graph.set_param(source, "path", json!(input)).unwrap();
        graph.set_param(export, "path", json!(output)).unwrap();
        let mut worker = Worker::new(Arc::new(|| {}), None);
        worker.request(&graph, 3, vec![preview]).unwrap();
        wait(&mut worker);
        let before = pixel(&worker, preview);
        std::fs::write(&input, "2").unwrap();
        perform(&mut worker, &graph, export);
        // The preview's read serves the export at full detail too.
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "[1.0, 1.0, 0.0]");
        assert_eq!(pixel(&worker, preview), before, "export leaves preview intact");
        let chosen = input.with_extension("chosen");
        std::fs::write(&chosen, "3").unwrap();
        graph.set_param(source, "path", json!(chosen)).unwrap();
        worker.request(&graph, 3, vec![preview]).unwrap();
        wait(&mut worker);
        assert_eq!(pixel(&worker, preview), [3.0, 1.0, 0.0, 1.0]);
        let mut profile = serde_json::to_value(drip::node::profile::Settings::default()).unwrap();
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
        std::fs::remove_file(chosen).unwrap();
        std::fs::remove_file(output).unwrap();
    }
}

mod a_preview_waits_for_foreign_work_and_runs_again_when_it_ends_fixture {
    use super::*;

    static GATE: Mutex<Option<mpsc::Receiver<()>>> = Mutex::new(None);
    static CALLS: Mutex<Vec<f32>> = Mutex::new(Vec::new());
    #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
    struct Slow {
        #[param(default = 1.0, range = 0.0..=10.0)]
        value: f32,
    }
    /// Publishes the value once `GATE` releases it.
    #[drip::node(id = "test-slow-foreign", name = "slow", foreign = memory)]
    fn slow(p: &Slow, image: Publish<'_, Rgb>) -> drip::Result<()> {
        CALLS.lock().unwrap().push(p.value);
        GATE.lock().unwrap().as_ref().unwrap().recv_timeout(TIMEOUT).unwrap();
        let desc = RgbDesc::new(Extent { width: 1, height: 1 }, Coordinates::Space(REC2020_D65));
        image.set(desc, vec![[p.value, 1.0, 0.0]].into())
    }

    /// Polls until the worker reports a preview that waits for `slow`.
    fn deferred(worker: &mut Worker) -> Vec<Notice> {
        let deadline = Instant::now() + COMPUTE_TIMEOUT;
        let mut notices = Vec::new();
        while worker.waiting().is_none() {
            notices.extend(worker.poll());
            assert!(Instant::now() < deadline, "the preview did not defer");
            thread::sleep(Duration::from_millis(1));
        }
        notices
    }

    #[test]
    fn a_preview_waits_for_foreign_work_and_runs_again_when_it_ends() {
        let (release, gate) = mpsc::channel();
        *GATE.lock().unwrap() = Some(gate);
        let mut worker = Worker::new(Arc::new(|| {}), None);
        let (mut graph, source, preview, histogram) = graph(&slow::KIND);

        // The batch returns while the job runs. Nothing shown yet stays empty,
        // and the waiting node is status, not a failure.
        worker.request(&graph, 1, vec![preview, histogram]).unwrap();
        let notices = deferred(&mut worker);
        assert!(notices.iter().all(|notice| matches!(notice, Notice::Evaluated(Ok(())))));
        assert_eq!(worker.waiting(), Some("slow"));
        assert!(worker.busy());
        assert!(worker.result(preview).is_none());
        // Another detail level has the same key and joins the running job.
        worker.request(&graph, 2, vec![preview, histogram]).unwrap();
        deferred(&mut worker);
        release.send(()).unwrap();
        // The job's end wakes the worker, which runs the latest preview again.
        wait(&mut worker);
        assert_eq!(worker.waiting(), None);
        assert_eq!(pixel(&worker, preview), [1.0, 1.0, 0.0, 1.0]);
        assert!(worker.result(histogram).unwrap().is_ok());
        assert_eq!(*CALLS.lock().unwrap(), [1.0]);

        // A new value runs a new job, and the views keep the previous one
        // until it ends.
        graph.set_param(source, "value", json!(2)).unwrap();
        worker.request(&graph, 2, vec![preview, histogram]).unwrap();
        deferred(&mut worker);
        assert_eq!(pixel(&worker, preview), [1.0, 1.0, 0.0, 1.0]);
        release.send(()).unwrap();
        wait(&mut worker);
        assert_eq!(pixel(&worker, preview), [2.0, 1.0, 0.0, 1.0]);
        assert_eq!(*CALLS.lock().unwrap(), [1.0, 2.0]);
    }
}

#[test]
fn preparation_retries_each_request_without_persistent_image_cache() {
    use crate::test_nodes::{PROBE_CALLS as CALLS, probe};
    use std::sync::atomic::Ordering;

    let mut graph = Graph::default();
    let id = graph.add_node(&probe::KIND).unwrap();
    let mut worker = Worker::new(Arc::new(|| {}), None);
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

    #[drip::node(id="test-panic", name="panic", contract=empty_contract)]
    fn panic_node(_: &KernelContext<'_>, _: &()) -> drip::Result<()> {
        panic!("test worker failure")
    }

    #[test]
    fn panic_wakes_the_ui_and_ends_the_busy_state() {
        let mut graph = Graph::default();
        let id = graph.add_node(&panic_node::KIND).unwrap();
        let (wake, woke) = mpsc::channel();
        let mut worker = Worker::new(
            Arc::new(move || {
                let _ = wake.send(());
            }),
            None,
        );
        worker.request(&graph, 0, vec![id]).unwrap();
        woke.recv_timeout(TIMEOUT).unwrap();
        assert!(worker.poll().iter().any(|notice| matches!(notice, Notice::Failed)));
        assert!(!worker.busy());
        assert!(worker.request(&graph, 0, vec![id]).is_err());
        assert!(!worker.busy());
    }
}

mod a_magnified_view_shows_a_full_detail_crop_over_its_preview_fixture {
    use super::*;
    use crate::node_ui::preview::ImageView;
    use drip::lattice::Rect;

    #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
    struct Ramp {
        #[param(default = 1.0, range = 0.0..=10.0)]
        gain: f32,
    }
    const WIDTH: u32 = 16;
    fn ramp_contract(_: &drip::runtime::GlobalContext, _: &Ramp) -> drip::Result<(RgbDesc,)> {
        let extent = Extent { width: WIDTH, height: 12 };
        Ok((RgbDesc::new(extent, Coordinates::Space(REC2020_D65)),))
    }
    /// Sample `i` holds `gain · i / 256` in red, exact in f16.
    #[drip::node(id = "test-ramp", name = "ramp", contract = ramp_contract)]
    fn ramp(_: &KernelContext<'_>, p: &Ramp, mut image: Write<'_, Rgb>) -> drip::Result<()> {
        for (i, px) in image.host().iter_mut().enumerate() {
            *px = [p.gain * i as f32 / 256.0, 0.0, 0.0];
        }
        Ok(())
    }

    /// Red of each packed texel of `image`.
    fn reds(image: &crate::render::image::Image) -> Vec<f32> {
        let texel = |i: usize| u16::from_ne_bytes([image.texels[i * 8], image.texels[i * 8 + 1]]);
        (0..image.width * image.height).map(|i| half::f16::from_bits(texel(i)).to_f32()).collect()
    }

    fn detail(worker: &Worker, id: NodeId) -> Option<&ImageView> {
        worker.detail_view(id).map(|view| view.downcast_ref::<ImageView>().unwrap())
    }

    /// The view asks for its visible region. The crop of the region at full
    /// detail arrives after the preview and shows over it while both belong
    /// to the same graph. An edit hides it once the new preview shows, until
    /// the region's new crop arrives. Scopes keep the preview's samples.
    #[test]
    fn a_magnified_view_shows_a_full_detail_crop_over_its_preview() {
        let (mut graph, source, preview, histogram) = graph(&ramp::KIND);
        let mut worker = Worker::new(Arc::new(|| {}), None);
        worker.request(&graph, 2, vec![preview, histogram]).unwrap();
        wait(&mut worker);
        let shown = &view::<ImageView>(&worker, preview).image;
        assert_eq!((shown.width, shown.height, shown.plane.full), (4, 3, [16.0, 12.0]));
        let scope = worker.result(histogram).unwrap().as_ref().unwrap().clone().unwrap();

        let region = Rect { origin: [4, 2], extent: [6, 5] };
        worker.detail(&graph, preview, Some(region)).unwrap();
        wait(&mut worker);
        let crop = &detail(&worker, preview).unwrap().image;
        assert_eq!((crop.width, crop.height), (6, 5));
        assert_eq!(crop.plane.covers, [[4.0, 2.0], [10.0, 7.0]]);
        let expected: Vec<f32> =
            (2..7).flat_map(|y| (4..10).map(move |x| (y * WIDTH + x) as f32 / 256.0)).collect();
        assert_eq!(reds(crop), expected);
        let unchanged = worker.result(histogram).unwrap().as_ref().unwrap().as_ref().unwrap();
        assert!(Arc::ptr_eq(unchanged, &scope), "scopes keep the preview's view");
        // The same region again asks for nothing.
        worker.detail(&graph, preview, Some(region)).unwrap();
        assert!(!worker.busy());

        graph.set_param(source, "gain", json!(2.0)).unwrap();
        worker.request(&graph, 2, vec![preview, histogram]).unwrap();
        worker.detail(&graph, preview, Some(region)).unwrap();
        // The preview runs first, and with it the previous crop leaves.
        while worker.showing_previous() {
            assert!(detail(&worker, preview).is_some(), "the crop shows with its preview");
            worker.poll();
            thread::sleep(Duration::from_millis(1));
        }
        assert!(detail(&worker, preview).is_none());
        wait(&mut worker);
        let crop = &detail(&worker, preview).unwrap().image;
        assert_eq!(reds(crop), expected.iter().map(|v| v * 2.0).collect::<Vec<_>>());

        worker.detail(&graph, preview, None).unwrap();
        assert!(detail(&worker, preview).is_none());
    }
}
