//! Foreign nodes run once per key on a thread of their own. A deferred
//! evaluation reports them pending and wakes when they end, a failure
//! repeats without running again, and the disk tier hands their outputs to
//! a new evaluator, discards damaged entries and stays within its budget.
use drip::{
    Error, Result,
    eval::{Evaluator, Policy},
    graph::Dag,
    node::{self, data::*},
    ports::*,
    runtime::{KernelContext, RuntimeContext},
    store::Disk,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(30);

/// An empty store directory under the system's temporary directory, removed
/// when dropped.
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("drip-foreign-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
    fn evaluator(&self) -> Evaluator {
        Evaluator::new(RuntimeContext::host()).with_disk(Disk::new(&self.0))
    }
    fn entries(&self) -> Vec<PathBuf> {
        let mut entries: Vec<_> = fs::read_dir(&self.0)
            .map(|files| files.flatten().map(|file| file.path()).collect())
            .unwrap_or_default();
        entries.sort();
        entries
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, drip::Parameters)]
struct Fill {
    #[param(default = 0.0, range = -100.0..=100.0)]
    value: f32,
}
fn desc() -> RgbDesc {
    RgbDesc::new(Extent { width: 4, height: 1 }, Coordinates::Space(REC2020_D65))
}

/// The values each foreign test node ran with. Tests use distinct values.
static RUNS: Mutex<Vec<f32>> = Mutex::new(Vec::new());
fn runs(value: f32) -> usize {
    RUNS.lock().unwrap().iter().filter(|run| **run == value).count()
}

/// Four samples of the value, kept on disk. A negative value fails.
#[drip::node(id = "test-fill", foreign = disk)]
fn fill(p: &Fill, image: Publish<'_, Rgb>) -> Result<()> {
    RUNS.lock().unwrap().push(p.value);
    if p.value < 0.0 {
        return Err(Error::Runtime("negative fill".into()));
    }
    image.set(desc(), vec![[p.value; 3]; 4].into())
}

/// The input doubled, kept on disk.
#[drip::node(id = "test-double", foreign = disk)]
fn double(_: &(), image: Read<'_, Rgb>, output: Publish<'_, Rgb>) -> Result<()> {
    let doubled: Vec<_> = image.data.host()?.iter().map(|px| px.map(|v| v * 2.0)).collect();
    output.set(image.desc.clone(), doubled.into())
}

/// Waits for `GATE` before publishing the value, in memory only.
static GATE: Mutex<Option<mpsc::Receiver<()>>> = Mutex::new(None);
#[drip::node(id = "test-slow", foreign = memory)]
fn slow(p: &Fill, image: Publish<'_, Rgb>) -> Result<()> {
    RUNS.lock().unwrap().push(p.value);
    GATE.lock().unwrap().as_ref().unwrap().recv_timeout(TIMEOUT).unwrap();
    image.set(desc(), vec![[p.value; 3]; 4].into())
}

/// A view that forces its input.
#[drip::node(id = "test-sink")]
fn sink(_: &KernelContext<'_>, _: &(), _image: Read<'_, Rgb>) -> Result<()> {
    Ok(())
}

fn filled(value: f32) -> (Dag, Output<Rgb>) {
    let mut dag = Dag::new();
    let fill = fill::add(&mut dag, Fill { value }).unwrap();
    (dag, fill.image)
}

#[test]
fn a_new_evaluator_reads_a_disk_node_instead_of_running_it() {
    let scratch = Scratch::new("reopen");
    let (dag, output) = filled(1.0);
    let evaluate = || scratch.evaluator().evaluate(&dag, &Default::default(), output).unwrap();
    let first = evaluate();
    assert_eq!([first.statistics.nodes, first.statistics.reads], [1, 0]);
    assert_eq!(scratch.entries().len(), 1);
    let second = evaluate();
    let s = &second.statistics;
    assert_eq!([s.nodes, s.hits, s.reads], [0, 1, 1]);
    assert_eq!(second.desc, first.desc);
    assert_eq!(second.data.host().unwrap(), first.data.host().unwrap());
    assert_eq!(runs(1.0), 1);
}

#[test]
fn a_damaged_entry_is_discarded_and_computed_again() {
    let scratch = Scratch::new("damaged");
    let (dag, output) = filled(2.0);
    let evaluate = || scratch.evaluator().evaluate(&dag, &Default::default(), output).unwrap();
    let expected = evaluate().data.host().unwrap().to_vec();
    let entry = scratch.entries().remove(0);
    let stored = fs::read(&entry).unwrap();
    let mut format = stored.clone();
    format[0] ^= 1;
    let truncated = stored[..stored.len() - 1].to_vec();
    let trailing = [stored.as_slice(), &[0]].concat();
    for damaged in [format, truncated, trailing] {
        fs::write(&entry, damaged).unwrap();
        let again = evaluate();
        assert_eq!([again.statistics.nodes, again.statistics.reads], [1, 0]);
        assert_eq!(&**again.data.host().unwrap(), expected.as_slice());
        assert_eq!(fs::read(&entry).unwrap(), stored, "the entry is written again");
    }
    assert_eq!(runs(2.0), 4);
}

/// A budget of two entries keeps the two used last: reading an entry counts
/// as a use, so the entry written between the read and the next write goes.
#[test]
fn the_disk_budget_deletes_the_least_recently_used_entries() {
    let scratch = Scratch::new("budget");
    let size = |path: &Path| fs::metadata(path).unwrap().len();
    let evaluate = |value, budget| {
        let disk = Disk { budget, ..Disk::new(&scratch.0) };
        let (dag, output) = filled(value);
        let evaluator = Evaluator::new(RuntimeContext::host()).with_disk(disk);
        let nodes = evaluator.evaluate(&dag, &Default::default(), output).unwrap().statistics.nodes;
        // Modification times order the entries; keep them apart.
        thread::sleep(Duration::from_millis(20));
        nodes
    };
    evaluate(3.0, u64::MAX);
    let budget = 2 * size(&scratch.entries()[0]);
    let [read, evicted, written] = [3.0, 4.0, 5.0];
    assert_eq!(evaluate(evicted, budget), 1);
    assert_eq!(evaluate(read, budget), 0);
    assert_eq!(evaluate(written, budget), 1);
    assert_eq!(scratch.entries().len(), 2);
    assert_eq!([evaluate(read, budget), evaluate(written, budget)], [0, 0]);
    assert_eq!(evaluate(evicted, budget), 1);
}

/// Bound data has a key random per process, so what depends on it is not
/// written.
#[test]
fn outputs_derived_from_bound_data_stay_off_the_disk() {
    let scratch = Scratch::new("bound");
    let mut dag = Dag::new();
    let bound = node::source::<Rgb>(&mut dag, desc(), vec![[1.0; 3]; 4]).unwrap();
    let (stored, local) = (double::add(&mut dag, ()).unwrap(), double::add(&mut dag, ()).unwrap());
    let fill = fill::add(&mut dag, Fill { value: 6.0 }).unwrap();
    dag.connect(bound, local.image).unwrap();
    dag.connect(fill.image, stored.image).unwrap();
    let evaluator = scratch.evaluator();
    let result = evaluator.evaluate(&dag, &Default::default(), local.output).unwrap();
    assert_eq!(&**result.data.host().unwrap(), &[[2.0; 3]; 4]);
    assert!(scratch.entries().is_empty());
    evaluator.evaluate(&dag, &Default::default(), stored.output).unwrap();
    assert_eq!(scratch.entries().len(), 2);
}

#[test]
fn a_failed_job_repeats_its_error_without_running_again() {
    let (dag, output) = filled(-1.0);
    let evaluator = Evaluator::new(RuntimeContext::host());
    for _ in 0..2 {
        let error = evaluator.evaluate(&dag, &Default::default(), output).err().unwrap();
        assert_eq!(error, Error::Runtime("negative fill".into()));
    }
    assert_eq!(runs(-1.0), 1);
}

/// A deferred batch reports the running job's node and its consumers as
/// pending, serves independent targets, and starts no second job for the
/// same key. The job's end wakes the caller, and the next batch finds its
/// outputs stored.
#[test]
fn a_deferred_demand_reports_pending_and_wakes_when_the_job_ends() {
    let (release, gate) = mpsc::channel();
    *GATE.lock().unwrap() = Some(gate);
    let (wake, woke) = mpsc::channel();
    let evaluator = Evaluator::new(RuntimeContext::host()).with_wake(move || {
        let _ = wake.send(());
    });
    let mut dag = Dag::new();
    let slow = slow::add(&mut dag, Fill { value: 7.0 }).unwrap();
    let bound = node::source::<Rgb>(&mut dag, desc(), vec![[1.0; 3]; 4]).unwrap();
    let (waiting, independent) =
        (sink::add(&mut dag, ()).unwrap(), sink::add(&mut dag, ()).unwrap());
    dag.connect(slow.image, waiting.image).unwrap();
    dag.connect(bound, independent.image).unwrap();
    let targets = [waiting.node, independent.node];
    let batch =
        || evaluator.evaluate_batch(&dag, &Default::default(), &targets, Policy::Defer, None);
    for _ in 0..2 {
        let values = batch();
        assert!(
            matches!(&values[&waiting.node], Err(Error::Node { node, source, .. })
                if *node == slow.node && **source == Error::Pending),
            "{:?}",
            values[&waiting.node].as_ref().err()
        );
        assert!(values[&independent.node].is_ok());
    }
    assert!(woke.try_recv().is_err());
    release.send(()).unwrap();
    woke.recv_timeout(TIMEOUT).unwrap();
    let values = batch();
    let inputs = values[&waiting.node].as_ref().unwrap();
    let inputs = inputs.bind::<sink::Definition>().unwrap();
    assert_eq!(&**inputs.image.data.host().unwrap(), &[[7.0; 3]; 4]);
    assert_eq!(runs(7.0), 1);
}

/// RAW decode is a disk node: a new evaluator reads the mosaic and the
/// metadata without decoding.
#[test]
fn reopening_a_raw_reads_its_decode_from_disk() {
    let scratch = Scratch::new("raw");
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/pixls/nikon-d70s.nef");
    let mut dag = Dag::new();
    let raw = node::raw::add(&mut dag, node::raw::Settings { path: Some(fixture) }).unwrap();
    let global = Default::default();
    let decoded = scratch.evaluator();
    let mosaic = decoded.evaluate(&dag, &global, raw.mosaic).unwrap();
    let metadata = decoded.evaluate(&dag, &global, raw.metadata).unwrap();
    assert_eq!([mosaic.statistics.nodes, metadata.statistics.hits], [1, 1]);
    drop(decoded);
    assert_eq!(scratch.entries().len(), 2);
    let reopened = scratch.evaluator();
    let read = reopened.evaluate(&dag, &global, raw.mosaic).unwrap();
    assert_eq!([read.statistics.nodes, read.statistics.reads], [0, 1]);
    assert_eq!(read.desc, mosaic.desc);
    assert_eq!(read.data.host().unwrap(), mosaic.data.host().unwrap());
    let facts = reopened.evaluate(&dag, &global, raw.metadata).unwrap();
    assert_eq!([facts.statistics.nodes, facts.statistics.reads], [0, 1]);
    assert_eq!(*facts.desc, *metadata.desc);
}
