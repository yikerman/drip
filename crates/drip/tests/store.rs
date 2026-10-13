//! The store keeps materialized values across evaluations: a demand runs only
//! the nodes whose outputs changed, an unchanged demand runs none, keys
//! follow content rather than graph instances, and the budget evicts the
//! least durable, least recently used values first.
use drip::{
    Result,
    definition::{FileStamp, Identity},
    eval::{Evaluator, Statistics},
    graph::Dag,
    node::{self, chromatic_adaptation, data::*, exposure, orientation, sigmoid},
    ports::*,
    registry::tag,
    runtime::{Budget, GlobalContext, KernelContext, RuntimeContext},
};
use serde_json::json;
use std::sync::Arc;

fn metadata() -> Arc<drip_raw::Metadata> {
    let mut m =
        drip_raw::Metadata { make: "Fixture".into(), model: "store".into(), ..Default::default() };
    m.calibration.xyz_to_cam[..3].copy_from_slice(&[
        [0.66, -0.18, -0.05],
        [-0.52, 1.30, 0.25],
        [-0.10, 0.17, 0.65],
    ]);
    m.calibration.as_shot = [2.2, 1.0, 1.6, 1.0];
    Arc::new(m)
}

/// The template's processing from a Bayer mosaic: a demosaic kernel, then the
/// camera matrix, CAT16, Orientation, Exposure and Sigmoid as one segment.
struct Pipeline {
    dag: Dag,
    sigmoid: sigmoid::apply::Ports,
}
fn pipeline() -> Pipeline {
    let m = metadata();
    let mut dag = Dag::new();
    let d = &mut dag;
    let response = Response { camera: Camera::of(&m), balance: None };
    let extent = Extent { width: 8, height: 6 };
    let samples: Vec<f32> = (0..extent.pixels()).map(|i| 0.05 + i as f32 / 60.0).collect();
    let desc = BayerDesc { extent, phase: BayerPhase::Rggb, response };
    let mosaic = node::source::<Bayer>(d, desc, samples).unwrap();
    let metadata = node::source::<CaptureMetadata>(d, m.clone(), m).unwrap();
    let demosaic = node::bayer_2x2::add(d, ()).unwrap();
    let camera = node::camera_to_rgb::add(d, ()).unwrap();
    let cat = chromatic_adaptation::add(d, Default::default()).unwrap();
    let orientation = orientation::add(d, Default::default()).unwrap();
    let exposure = exposure::add(d, exposure::Exposure { ev: 0.5 }).unwrap();
    let sigmoid = sigmoid::apply::add(d, Default::default()).unwrap();
    d.connect(mosaic, demosaic.image).unwrap();
    d.connect(demosaic.output, camera.image).unwrap();
    d.connect(metadata, camera.metadata).unwrap();
    d.connect(camera.output, cat.image).unwrap();
    d.connect(metadata, cat.metadata).unwrap();
    d.connect(cat.output, orientation.image).unwrap();
    d.connect(metadata, orientation.metadata).unwrap();
    d.connect(orientation.output, exposure.image).unwrap();
    d.connect(exposure.output, sigmoid.image).unwrap();
    Pipeline { dag, sigmoid }
}

fn work(statistics: &Statistics) -> [usize; 5] {
    let s = statistics;
    [s.nodes, s.hits, s.fused, s.uploads, s.downloads]
}

/// A Sigmoid-only edit runs the five nodes of the segment it ends, in one
/// launch, from the stored demosaic output and metadata, whose device copy
/// the store kept. Demanding the same output again runs nothing.
fn a_sigmoid_edit_recomputes_only_the_tail(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut pipeline = pipeline();
    let first =
        evaluator.evaluate::<Rgb>(&pipeline.dag, &Default::default(), pipeline.sigmoid.output);
    let first = first.unwrap();
    // Two sources, the demosaic and the segment's five nodes; one upload.
    assert_eq!(work(&first.statistics), [8, 0, 1, 1, 0]);

    let unchanged = evaluator
        .evaluate::<Rgb>(&pipeline.dag, &Default::default(), pipeline.sigmoid.output)
        .unwrap();
    assert_eq!(work(&unchanged.statistics), [0, 1, 0, 0, 0]);
    assert_eq!(unchanged.data.host().unwrap(), first.data.host().unwrap());

    let contrast = sigmoid::Settings { contrast: 2.5, ..Default::default() };
    let settings = serde_json::to_value(contrast).unwrap();
    pipeline.dag.set_parameters(pipeline.sigmoid.node, settings).unwrap();
    let edited = evaluator
        .evaluate::<Rgb>(&pipeline.dag, &Default::default(), pipeline.sigmoid.output)
        .unwrap();
    // The demosaic output and the metadata are hits.
    assert_eq!(work(&edited.statistics), [5, 2, 1, 0, 0]);
    assert_ne!(edited.data.host().unwrap(), first.data.host().unwrap());
}

fn rgb(dag: &mut Dag, pixels: usize) -> Output<Rgb> {
    let extent = Extent { width: pixels as u32, height: 1 };
    let desc = RgbDesc::new(extent, Coordinates::Space(REC2020_D65));
    node::source::<Rgb>(dag, desc, vec![[0.25, 0.5, 1.0]; pixels]).unwrap()
}
fn scaled(dag: &mut Dag, input: Output<Rgb>, gain: f32) -> Output<Rgb> {
    let node = first::gain::add(dag, Gain { gain }).unwrap();
    dag.connect(input, node.image).unwrap();
    node.output
}

/// A host budget of two and a half rasters holds the source and one scaled
/// copy. The older copy goes first, and the source, a High entry, survives.
#[test]
fn the_budget_evicts_the_least_durable_and_oldest_first() {
    let raster = 4 * 12;
    let budget = Budget { device: 0, host: raster * 5 / 2 };
    let evaluator = Evaluator::new(RuntimeContext::host().with_budget(budget));
    let mut dag = Dag::new();
    let source = rgb(&mut dag, 4);
    let (a, b) = (scaled(&mut dag, source, 2.0), scaled(&mut dag, source, 3.0));
    let evaluate = |output| evaluator.evaluate::<Rgb>(&dag, &Default::default(), output).unwrap();
    let counts = |output| {
        let s = evaluate(output).statistics;
        [s.nodes, s.hits, s.evictions]
    };
    assert_eq!(counts(a), [2, 0, 0]);
    assert_eq!(counts(b), [1, 1, 1]);
    assert_eq!(counts(b), [0, 1, 0]);
    assert_eq!(counts(a), [1, 1, 1]);
}

/// The store keeps a source's upload for the next demand. A device budget
/// below one raster drops it, and the host samples it keeps upload again.
fn a_device_eviction_keeps_the_host_samples(runtime: RuntimeContext) {
    let raster = 64 * 12;
    for (device, uploads, evictions) in [(4 * raster, 0, 0), (raster / 2, 1, 2)] {
        let budget = Budget { device, ..Budget::default() };
        let evaluator = Evaluator::new(runtime.clone().with_budget(budget));
        let mut dag = Dag::new();
        let source = rgb(&mut dag, 64);
        let gain = exposure::add(&mut dag, exposure::Exposure { ev: 1.0 }).unwrap();
        dag.connect(source, gain.image).unwrap();
        let evaluate = |dag: &Dag| {
            evaluator.evaluate::<Rgb>(dag, &Default::default(), gain.output).unwrap().statistics
        };
        let first = evaluate(&dag);
        assert_eq!((first.uploads, first.evictions), (1, evictions));
        dag.set_parameters(gain.node, json!({"ev": 2.0})).unwrap();
        let edited = evaluate(&dag);
        assert_eq!([edited.nodes, edited.hits, edited.uploads], [1, 1, uploads]);
    }
}

fn gain_contract(_: &GlobalContext, _: &Gain, image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}
#[derive(Clone, serde::Serialize, serde::Deserialize, drip::Parameters)]
struct Ramp {
    #[param(default = 4, range = 1..=64)]
    length: i64,
}
/// A foreign node whose outputs its parameters determine, so equal
/// parameters in two graphs share a key.
#[drip::node(id = "test-ramp", foreign = memory)]
fn ramp(p: &Ramp, image: Publish<'_, Rgb>) -> Result<()> {
    let extent = Extent { width: p.length as u32, height: 1 };
    let pixels = (0..p.length).map(|i| [i as f32, 0.5, 1.0]).collect::<Vec<_>>();
    image.set(RgbDesc::new(extent, Coordinates::Space(REC2020_D65)), pixels.into())
}
#[derive(Clone, serde::Serialize, serde::Deserialize, drip::Parameters)]
pub struct Gain {
    #[param(default = 1.0, range = 0.0..=4.0)]
    pub gain: f32,
}
mod first {
    use super::*;
    /// Multiply by a gain, as version 1. Tagged a look, the least durable.
    #[drip::node(id = "test-gain", tags = [tag::LOOK], contract = gain_contract)]
    pub fn gain(
        _: &KernelContext<'_>,
        p: &Gain,
        image: Read<'_, Rgb>,
        mut output: Write<'_, Rgb>,
    ) -> Result<()> {
        for (o, i) in output.host().iter_mut().zip(image.data.host()?.iter()) {
            *o = i.map(|v| v * p.gain);
        }
        Ok(())
    }
}
mod second {
    use super::*;
    /// The same kind with changed numerics, so its version is bumped.
    #[drip::node(id = "test-gain", version = 2, tags = [tag::LOOK], contract = gain_contract)]
    pub fn gain(
        _: &KernelContext<'_>,
        p: &Gain,
        image: Read<'_, Rgb>,
        mut output: Write<'_, Rgb>,
    ) -> Result<()> {
        for (o, i) in output.host().iter_mut().zip(image.data.host()?.iter()) {
            *o = i.map(|v| v * p.gain * 2.0);
        }
        Ok(())
    }
}

/// Keys depend on content: the same graph built twice shares every value,
/// other parameters or a bumped version do not, and bound data is never
/// shared between instances.
#[test]
fn keys_follow_content_across_graph_instances() {
    let evaluator = Evaluator::new(RuntimeContext::host());
    let run = |kind: &'static drip::definition::Kind, gain: f32| {
        let mut dag = Dag::new();
        let source = ramp::add(&mut dag, Ramp { length: 4 }).unwrap();
        let node = dag.add((kind.build)(json!({"gain": gain})).unwrap()).unwrap();
        let image = dag.input_port(node, "image").unwrap();
        dag.connect_ids(source.image.id(), image).unwrap();
        let output = dag.typed_output::<Rgb>(dag.output_port(node, "output").unwrap()).unwrap();
        let s = evaluator.evaluate::<Rgb>(&dag, &Default::default(), output).unwrap().statistics;
        [s.nodes, s.hits]
    };
    assert_eq!(run(&first::gain::KIND, 2.0), [2, 0]);
    assert_eq!(run(&first::gain::KIND, 2.0), [0, 1]);
    assert_eq!(run(&first::gain::KIND, 3.0), [1, 1]);
    assert_eq!(run(&second::gain::KIND, 2.0), [1, 1]);
    assert_eq!(run(&first::gain::KIND, 2.0), [0, 1]);

    let bound = || {
        let mut dag = Dag::new();
        let source = rgb(&mut dag, 4);
        evaluator.evaluate::<Rgb>(&dag, &Default::default(), source).unwrap().statistics.nodes
    };
    assert_eq!([bound(), bound()], [1, 1]);
}

/// A foreign node's identity stamps the files its parameters name before it
/// reads them, so a rewritten file keys differently.
#[test]
fn a_foreign_identity_stamps_its_files() {
    let path = std::env::temp_dir().join(format!("drip-store-{}.arw", std::process::id()));
    let identity = || {
        let parameters = json!({"path": path});
        Identity::foreign(&node::raw::read::KIND, parameters).unwrap()
    };
    std::fs::write(&path, "first").unwrap();
    let Identity::Foreign(_, files) = identity() else { panic!("RAW decode is foreign") };
    assert_eq!(files, [FileStamp::read(path.clone()).unwrap()]);
    assert_eq!(files[0].len, 5);
    std::fs::write(&path, "second").unwrap();
    assert_ne!(identity(), Identity::Foreign(json!({"path": path}), files));
    std::fs::remove_file(&path).unwrap();
    let unset = Identity::foreign(&node::raw::read::KIND, json!({"path": null})).unwrap();
    assert_eq!(unset, Identity::Foreign(json!({"path": null}), Vec::new()));
}

fn all(runtime: RuntimeContext) {
    a_sigmoid_edit_recomputes_only_the_tail(runtime.clone());
    a_device_eviction_keeps_the_host_samples(runtime);
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_store() {
    all(RuntimeContext::cpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_store() {
    all(RuntimeContext::from_env().unwrap());
}
