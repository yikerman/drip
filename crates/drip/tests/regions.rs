//! Regions and scale are evaluation's work. A target with a region receives
//! the samples covering it, equal bit for bit to the same samples of the
//! whole image, and a fused segment computes only those, so an export over
//! the device budget writes its image in bands. A preview scale reduces a
//! full-detail image once, in the consumer's kernel, outputs that do not
//! depend on the scale are keyed once, and a foreign node reads full detail
//! wherever it sits.
use drip::{
    Result,
    eval::{Evaluator, InputValues, Policy, Statistics, Target},
    graph::Dag,
    node::{
        self, chromatic_adaptation,
        data::*,
        exposure,
        filter::Filter,
        orientation::{self, Orientation},
        perspective::{self, Aspect},
        resample::{self, Factor, Size},
        sigmoid,
    },
    ports::*,
    runtime::{Budget, GlobalContext, KernelContext, RuntimeContext},
    shape::Rect,
};
use std::sync::{Arc, Mutex};

/// A view that forces its input.
#[drip::node(id = "test-sink")]
fn sink(_: &KernelContext<'_>, _: &(), _image: Read<'_, Rgb>) -> Result<()> {
    Ok(())
}

/// Finite samples that vary along both axes, signed and above one.
fn pattern(width: u32, height: u32) -> Vec<[f32; 3]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = ((i % width) as f32, (i / width) as f32);
            [(x * 0.37).sin() * 2.0 + y * 0.01, (y * 0.29).cos() + x * 0.02, (x - y) * 0.03]
        })
        .collect()
}

fn source(dag: &mut Dag, width: u32, height: u32) -> Output<Rgb> {
    let desc = RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65));
    node::source::<Rgb>(dag, desc, pattern(width, height)).unwrap()
}

fn then<P>(dag: &mut Dag, input: Output<Rgb>, add: impl FnOnce(&mut Dag) -> P) -> Output<Rgb>
where
    P: Ported,
{
    let ports = add(dag);
    dag.connect(input, ports.image()).unwrap();
    ports.output()
}

/// The image and output ports of the nodes these tests chain.
trait Ported {
    fn image(&self) -> Input<Rgb>;
    fn output(&self) -> Output<Rgb>;
}
macro_rules! ported {
    ($($ports:ty),*) => {$(
        impl Ported for $ports {
            fn image(&self) -> Input<Rgb> {
                self.image
            }
            fn output(&self) -> Output<Rgb> {
                self.output
            }
        }
    )*};
}
ported!(
    exposure::Ports,
    sigmoid::apply::Ports,
    orientation::Ports,
    perspective::Ports,
    resample::Ports
);

type Step = fn(&mut Dag, Output<Rgb>) -> Output<Rgb>;

/// `input` through `steps` in order.
fn chain(dag: &mut Dag, input: Output<Rgb>, steps: &[Step]) -> Output<Rgb> {
    steps.iter().fold(input, |image, step| step(dag, image))
}

fn exposed(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    then(dag, input, |d| exposure::add(d, exposure::Exposure { ev: 0.7 }).unwrap())
}
fn toned(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    let settings = sigmoid::Settings { contrast: 2.0, skew: 0.3, preserve_hue: 0.6 };
    then(dag, input, |d| sigmoid::apply::add(d, settings).unwrap())
}
fn turned(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    let settings = orientation::Settings { orientation: Orientation::Rotate90Clockwise };
    then(dag, input, |d| orientation::add(d, settings).unwrap())
}
fn keystoned(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    let quad = [[0.1, 0.0], [0.9, 0.05], [1.0, 1.0], [0.0, 0.9]];
    let settings = perspective::Settings::default().with_quad(quad, Aspect::Corners);
    then(dag, input, |d| perspective::add(d, settings).unwrap())
}
fn resampled(dag: &mut Dag, input: Output<Rgb>, filter: Filter, factor: f64) -> Output<Rgb> {
    let settings = resample::Settings { filter, size: Size::Factor(Factor { factor }) };
    then(dag, input, |d| resample::add(d, settings).unwrap())
}
fn minified(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    resampled(dag, input, Filter::Lanczos3, 0.6)
}
fn magnified(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    resampled(dag, input, Filter::Mitchell, 1.7)
}
fn halved(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    resampled(dag, input, Filter::Box, 0.5)
}

/// `sink`'s input over `region` at `scale`, with the call's statistics.
fn view(
    evaluator: &Evaluator,
    dag: &Dag,
    sink: NodeId,
    scale: u32,
    region: Option<Rect>,
) -> (InputValues, Statistics) {
    let target = Target { node: sink, region };
    let mut batch =
        evaluator.evaluate_batch(dag, &GlobalContext { scale }, &[target], Policy::Wait, None);
    (batch.inputs.remove(&sink).unwrap().unwrap(), batch.statistics)
}

fn image(values: &InputValues) -> (RgbDesc, Vec<[u32; 3]>) {
    let image = values.bind::<sink::Definition>().unwrap().image;
    let bits = image.data.host().unwrap().iter().map(|px| px.map(f32::to_bits)).collect();
    (image.desc.clone(), bits)
}

/// The samples of `whole` in `window`, row by row.
fn window(whole: &(RgbDesc, Vec<[u32; 3]>), window: Rect) -> Vec<[u32; 3]> {
    let width = whole.0.extent.width as usize;
    let [x, y] = window.origin.map(|v| v as usize);
    let [w, h] = window.extent.map(|v| v as usize);
    (y..y + h).flat_map(|row| whole.1[row * width + x..row * width + x + w].to_vec()).collect()
}

/// Each chain's crop equals the same samples of its whole output, and a
/// segment that launches once writes only the crop's samples. A filtering
/// warp after a map cuts the segment, and the map's launch computes the
/// window the resampling reads.
fn crops_equal_the_whole_image_and_compute_only_the_crop(runtime: RuntimeContext) {
    let chains: [(&str, &[Step], usize); 6] = [
        ("maps", &[exposed, toned], 1),
        ("reindex", &[turned, exposed], 1),
        ("projective", &[keystoned, exposed], 1),
        ("minified after a map", &[exposed, minified], 2),
        ("magnified", &[magnified, exposed], 1),
        ("warps", &[turned, keystoned, halved], 1),
    ];
    let region = Rect { origin: [7, 5], extent: [13, 9] };
    for (name, steps, launches) in chains {
        let mut dag = Dag::new();
        let input = source(&mut dag, 61, 47);
        let output = chain(&mut dag, input, steps);
        let sink = sink::add(&mut dag, ()).unwrap();
        dag.connect(output, sink.image).unwrap();
        let whole = image(&view(&Evaluator::new(runtime.clone()), &dag, sink.node, 1, None).0);
        assert!(whole.0.window.is_none());
        let (values, statistics) =
            view(&Evaluator::new(runtime.clone()), &dag, sink.node, 1, Some(region));
        let crop = image(&values);
        assert_eq!(crop.0.extent, Extent { width: 13, height: 9 }, "{name}");
        let placement = crop.0.window.clone().unwrap();
        assert_eq!(placement, Window { origin: [7, 5], lattice: whole.0.extent.clone() });
        assert_eq!(crop.1, window(&whole, region), "{name}");
        assert_eq!(statistics.fused, launches, "{name}");
        if launches == 1 {
            assert_eq!(statistics.samples, region.cells(), "{name}");
        }
    }
}

/// A crop at a preview scale counts the region in full-detail pixels and
/// equals the same samples of the whole preview. A region partly beyond the
/// plane is clamped, and one wholly beyond it fails.
fn regions_count_full_detail_pixels(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut dag = Dag::new();
    let input = source(&mut dag, 61, 47);
    let output = chain(&mut dag, input, &[exposed, toned]);
    let sink = sink::add(&mut dag, ()).unwrap();
    dag.connect(output, sink.image).unwrap();
    let whole = image(&view(&evaluator, &dag, sink.node, 4, None).0);
    assert_eq!(whole.0.extent, Extent { width: 16, height: 12 });
    // Samples cover 61 / 16 by 47 / 12 pixels: columns 2 to 10 meet pixels 9
    // to 38, and rows 1 to 11 meet rows 6 to 46, where the plane ends.
    let region = Rect { origin: [9, 6], extent: [30, 50] };
    let crop = image(&view(&evaluator, &dag, sink.node, 4, Some(region)).0);
    let samples = Rect { origin: [2, 1], extent: [9, 11] };
    assert_eq!(crop.0.window.as_ref().unwrap().origin, samples.origin);
    assert_eq!(crop.0.extent, Extent { width: 9, height: 11 });
    assert_eq!(crop.1, window(&whole, samples));
    let beyond = Target { node: sink.node, region: Some(Rect { origin: [70, 0], extent: [4, 4] }) };
    let mut batch =
        evaluator.evaluate_batch(&dag, &Default::default(), &[beyond], Policy::Wait, None);
    let error = batch.inputs.remove(&sink.node).unwrap().err().unwrap();
    assert!(error.to_string().contains("the region lies outside the image"), "{error}");
}

/// Crops are stored under their region: the same region again runs only
/// the sink, another region recomputes the tail's crop, and once the whole
/// image is stored a crop copies its samples from it.
fn crops_are_stored_by_region(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut dag = Dag::new();
    let input = source(&mut dag, 61, 47);
    let output = chain(&mut dag, input, &[exposed, toned]);
    let sink = sink::add(&mut dag, ()).unwrap();
    dag.connect(output, sink.image).unwrap();
    let work = |region| {
        let s = view(&evaluator, &dag, sink.node, 1, region).1;
        [s.nodes, s.fused, s.samples]
    };
    let (a, b) =
        (Rect { origin: [0, 0], extent: [10, 10] }, Rect { origin: [30, 20], extent: [5, 4] });
    assert_eq!(work(Some(a)), [4, 1, 100]);
    assert_eq!(work(Some(a)), [1, 0, 0]);
    assert_eq!(work(Some(b)), [3, 1, 20]);
    assert_eq!(work(None), [3, 1, 61 * 47]);
    let c = Rect { origin: [1, 2], extent: [3, 3] };
    assert_eq!(work(Some(c)), [1, 1, 9]);
    // A region covering the plane is the whole image, which is stored.
    assert_eq!(work(Some(Rect { origin: [0, 0], extent: [61, 47] })), [1, 0, 0]);
}

/// A camera with a dense matrix and as-shot gains, oriented as captured.
fn metadata() -> Arc<drip_raw::Metadata> {
    let mut m = drip_raw::Metadata {
        make: "Fixture".into(),
        model: "regions".into(),
        ..Default::default()
    };
    m.calibration.xyz_to_cam[..3].copy_from_slice(&[
        [0.66, -0.18, -0.05],
        [-0.52, 1.30, 0.25],
        [-0.10, 0.17, 0.65],
    ]);
    m.calibration.as_shot = [2.2, 1.0, 1.6, 1.0];
    Arc::new(m)
}

/// The template's processing from a Bayer mosaic: RCD, then the camera
/// matrix, CAT16, Orientation, Exposure and Sigmoid as one segment.
fn template(dag: &mut Dag, width: u32, height: u32) -> (Output<Bayer>, Output<Rgb>) {
    let m = metadata();
    let response = Response { camera: Camera::of(&m), balance: None };
    let samples: Vec<f32> =
        (0..width * height).map(|i| 0.05 + ((i * 37) % 101) as f32 / 120.0).collect();
    let desc = BayerDesc { extent: Extent { width, height }, phase: BayerPhase::Rggb, response };
    let mosaic = node::source::<Bayer>(dag, desc, samples).unwrap();
    let metadata = node::source::<CaptureMetadata>(dag, m.clone(), m).unwrap();
    let demosaic = node::rcd::add(dag, ()).unwrap();
    let camera = node::camera_to_rgb::add(dag, ()).unwrap();
    let cat = chromatic_adaptation::add(dag, Default::default()).unwrap();
    let orientation = orientation::add(dag, Default::default()).unwrap();
    dag.connect(mosaic, demosaic.image).unwrap();
    dag.connect(demosaic.output, camera.image).unwrap();
    dag.connect(metadata, camera.metadata).unwrap();
    dag.connect(camera.output, cat.image).unwrap();
    dag.connect(metadata, cat.metadata).unwrap();
    dag.connect(cat.output, orientation.image).unwrap();
    dag.connect(metadata, orientation.metadata).unwrap();
    let output = chain(dag, orientation.output, &[exposed, toned]);
    (mosaic, output)
}

/// A 100 % crop of the template's output runs the whole demosaic, a
/// kernel, and the fused tail on the crop alone. Panning reuses the stored
/// demosaic output.
fn a_template_crop_computes_only_the_crop_in_the_tail(runtime: RuntimeContext) {
    let mut dag = Dag::new();
    let (_, output) = template(&mut dag, 96, 64);
    let sink = sink::add(&mut dag, ()).unwrap();
    dag.connect(output, sink.image).unwrap();
    let whole = image(&view(&Evaluator::new(runtime.clone()), &dag, sink.node, 1, None).0);
    let evaluator = Evaluator::new(runtime);
    let region = Rect { origin: [40, 12], extent: [24, 20] };
    let (values, statistics) = view(&evaluator, &dag, sink.node, 1, Some(region));
    assert_eq!(image(&values).1, window(&whole, region));
    assert_eq!([statistics.fused, statistics.samples], [1, region.cells()]);
    let panned = Rect { origin: [44, 12], extent: [24, 20] };
    let (values, statistics) = view(&evaluator, &dag, sink.node, 1, Some(panned));
    assert_eq!(image(&values).1, window(&whole, panned));
    // The sink and the five tail nodes run; the demosaic output is a hit.
    assert_eq!([statistics.nodes, statistics.fused, statistics.samples], [6, 1, 480]);
}

/// An export larger than the device budget computes its image in bands of
/// rows and writes the same bytes as an export in one piece. Perspective
/// Transform after the tail cuts the segment, so each band also runs the
/// maps on the window the warp reads.
fn a_tiled_export_matches_the_export_in_one_piece(runtime: RuntimeContext) {
    let file = std::env::temp_dir().join(format!("drip-regions-{}.tiff", std::process::id()));
    let mut dag = Dag::new();
    let (_, output) = template(&mut dag, 96, 64);
    let output = keystoned(&mut dag, output);
    let settings = node::export::Export { path: Some(file.clone()), ..Default::default() };
    let export = node::export::tiff::add(&mut dag, settings).unwrap();
    dag.connect(output, export.image).unwrap();
    let write = |device: u64| {
        let budget = Budget { device, ..Budget::default() };
        let evaluator = Evaluator::new(runtime.clone().with_budget(budget));
        let mut inputs = evaluator.evaluate_inputs(&dag, &Default::default(), &[export.node]);
        let inputs = inputs.remove(&export.node).unwrap().unwrap();
        let bands = inputs.bands(inputs.budget()).len();
        let node = dag.node(export.node).unwrap();
        (node.kind().action.unwrap().run)(node, &inputs).unwrap();
        (bands, std::fs::read(&file).unwrap())
    };
    let (one, whole) = write(Budget::default().device);
    // Ten rows of the 12-byte samples of an image at most 96 wide.
    let (bands, tiled) = write(96 * 12 * 10);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(one, 1);
    assert!(bands > 3, "{bands} bands");
    assert!(whole == tiled, "the tiled export differs");
}

/// At a preview scale a full-detail image on a plane is box-reduced in the
/// kernel of the maps that read it, one launch, onto `div_ceil(full,
/// scale)` samples.
fn a_full_detail_image_is_reduced_in_its_consumers_kernel(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut dag = Dag::new();
    let input = source(&mut dag, 8, 6);
    let output = exposed(&mut dag, input);
    let result = evaluator.evaluate::<Rgb>(&dag, &GlobalContext { scale: 2 }, output).unwrap();
    assert_eq!(
        (result.desc.extent.clone(), result.desc.full.clone()),
        (Extent { width: 4, height: 3 }, Extent { width: 8, height: 6 })
    );
    assert_eq!([result.statistics.fused, result.statistics.samples], [1, 12]);
    let pixels = pattern(8, 6);
    let gain = 0.7f32.exp2();
    for (i, actual) in result.data.host().unwrap().iter().enumerate() {
        let (x, y) = (i % 4 * 2, i / 4 * 2);
        let block =
            [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].map(|(x, y)| pixels[y * 8 + x]);
        for c in 0..3 {
            let mean = block.iter().map(|p| p[c]).sum::<f32>() / 4.0;
            assert!((actual[c] - mean * gain).abs() < 1e-5, "{actual:?} at {i}");
        }
    }
}

/// A reindex keeps its input's sampling, so after a full-detail image it is
/// reduced in the kernel of the maps that read it, still one launch.
fn a_reindex_of_full_detail_is_reduced_after_it(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut dag = Dag::new();
    let input = source(&mut dag, 8, 6);
    let output = chain(&mut dag, input, &[turned, exposed]);
    let reduced = evaluator.evaluate::<Rgb>(&dag, &GlobalContext { scale: 2 }, output).unwrap();
    assert_eq!(reduced.desc.extent, Extent { width: 3, height: 4 });
    assert_eq!([reduced.statistics.fused, reduced.statistics.samples], [1, 12]);
}

/// The Bayer frames each test kernel received, by width.
static SEEN: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Negates a mosaic on the host. It neither reads the scale nor reads an
/// image on a plane, so its output is the same at every scale.
fn same<P: Clone>(_: &GlobalContext, _: &(), image: &P) -> Result<(P,)> {
    Ok((image.clone(),))
}
#[drip::node(id = "test-negate-bayer", contract = same)]
fn negate(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Bayer>,
    mut output: Write<'_, Bayer>,
) -> Result<()> {
    SEEN.lock().unwrap().push(image.desc.extent.width);
    for (out, v) in output.host().iter_mut().zip(image.data.host()?.iter()) {
        *out = -v;
    }
    Ok(())
}

/// Copies RGB on the host and records the width it saw.
#[drip::node(id = "test-foreign-copy", foreign = memory)]
fn copy(_: &(), image: Read<'_, Rgb>, output: Publish<'_, Rgb>) -> Result<()> {
    SEEN.lock().unwrap().push(image.desc.extent.width);
    output.set(image.desc.clone(), image.data.host()?.clone())
}

/// A node whose outputs do not depend on the scale is keyed once: a second
/// scale runs only the demosaic, which claims the scale, and what follows.
/// A foreign node after the demosaic reads it at full detail at every
/// scale, and its consumer at a preview scale receives it reduced.
fn scale_independent_outputs_are_keyed_once(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut dag = Dag::new();
    let (mosaic, _) = template(&mut dag, 32, 24);
    let negated = negate::add(&mut dag, ()).unwrap();
    let demosaic = node::bayer_2x2::add(&mut dag, ()).unwrap();
    let foreign = copy::add(&mut dag, ()).unwrap();
    let sink = sink::add(&mut dag, ()).unwrap();
    dag.connect(mosaic, negated.image).unwrap();
    dag.connect(negated.output, demosaic.image).unwrap();
    dag.connect(demosaic.output, foreign.image).unwrap();
    dag.connect(foreign.output, sink.image).unwrap();
    SEEN.lock().unwrap().clear();
    let nodes = |scale| {
        let (values, statistics) = view(&evaluator, &dag, sink.node, scale, None);
        (image(&values).0.extent, statistics.nodes)
    };
    // The mosaic, the negation, the demosaic, the copy and the sink.
    assert_eq!(nodes(1), (Extent { width: 16, height: 12 }, 5));
    // Only the sink, which reads the copy reduced.
    assert_eq!(nodes(4), (Extent { width: 4, height: 3 }, 1));
    assert_eq!(*SEEN.lock().unwrap(), [32, 16]);

    let direct = sink::add(&mut dag, ()).unwrap();
    dag.connect(demosaic.output, direct.image).unwrap();
    let (values, statistics) = view(&evaluator, &dag, direct.node, 4, None);
    // The demosaic at scale 4 from the stored negation, and the sink.
    assert_eq!((image(&values).0.extent, statistics.nodes), (Extent { width: 4, height: 3 }, 2));
    assert_eq!(*SEEN.lock().unwrap(), [32, 16]);
}

fn all(runtime: RuntimeContext) {
    crops_equal_the_whole_image_and_compute_only_the_crop(runtime.clone());
    regions_count_full_detail_pixels(runtime.clone());
    crops_are_stored_by_region(runtime.clone());
    a_template_crop_computes_only_the_crop_in_the_tail(runtime.clone());
    a_tiled_export_matches_the_export_in_one_piece(runtime.clone());
    a_full_detail_image_is_reduced_in_its_consumers_kernel(runtime.clone());
    a_reindex_of_full_detail_is_reduced_after_it(runtime.clone());
    scale_independent_outputs_are_keyed_once(runtime);
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_regions() {
    all(RuntimeContext::cpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_regions() {
    all(RuntimeContext::from_env().unwrap());
}
