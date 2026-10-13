//! Reduce and bins nodes fold or count the pixels of their input's segment
//! without storing them: results equal host references and the reduction of
//! the materialized samples, and a demand that only bins an image launches no
//! fused kernel and downloads nothing.
use drip::{
    Result,
    body::{BinsBody, ReduceBody},
    eval::{Evaluator, Policy, Target},
    graph::Dag,
    image::Image,
    lattice::Rect,
    node::{self, data::*, exposure, sigmoid},
    ports::*,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
};

/// Tenths of `[0, 1)` per channel. Other values, NaN included, count nothing.
static TENTHS: BinsBody = BinsBody {
    name: "test::tenths",
    constants: 0,
    bins: 10,
    channels: 3,
    bin: body::tenths::expand,
};
/// A small waveform of the red channel: 64 value rows by 128 position
/// columns, more bins than fit in shared memory.
static PLACES: BinsBody = BinsBody {
    name: "test::places",
    constants: 0,
    bins: 64 * 128,
    channels: 1,
    bin: body::places::expand,
};
/// Per channel, the sum of the samples times the first constant.
static SUMS: ReduceBody = ReduceBody {
    name: "test::sums",
    constants: 1,
    identity: &[0.0; 3],
    accumulate: body::sums::expand,
    combine: body::add::expand,
};

mod body {
    // CubeCL expands comparisons, not `Range::contains`.
    #![allow(clippy::manual_range_contains)]
    use cubecl::prelude::*;
    use drip::body::{Bin, Pixel, Position};

    #[cube]
    fn tenth(v: f32) -> u32 {
        let mut bin = 10u32;
        if v >= 0.0f32 && v < 1.0f32 {
            bin = u32::cast_from(v * 10.0f32);
        }
        bin
    }

    #[cube]
    pub fn tenths(px: Pixel, _at: Position, _k: &[f32]) -> Bin {
        Bin { c0: tenth(px.r), c1: tenth(px.g), c2: tenth(px.b) }
    }

    #[cube]
    pub fn places(px: Pixel, at: Position, _k: &[f32]) -> Bin {
        let mut row = 64u32;
        if px.r >= 0.0f32 && px.r < 1.0f32 {
            row = u32::cast_from(px.r * 64.0f32);
        }
        Bin { c0: row * 128u32 + at.x * 128u32 / at.width, c1: 0u32, c2: 0u32 }
    }

    #[cube]
    pub fn sums(px: Pixel, k: &[f32], acc: &mut [f32]) {
        acc[0] += px.r * k[0];
        acc[1] += px.g * k[0];
        acc[2] += px.b * k[0];
    }

    #[cube]
    pub fn add(acc: &mut [f32], other: &[f32]) {
        acc[0] += other[0];
        acc[1] += other[1];
        acc[2] += other[2];
    }
}

fn tenth(v: f32) -> usize {
    if (0.0..1.0).contains(&v) { (v * 10.0) as usize } else { 10 }
}
fn tenths(pixels: &[[f32; 3]]) -> Vec<u32> {
    let mut counts = vec![0; 30];
    for px in pixels {
        for (c, &v) in px.iter().enumerate() {
            if let Some(count) = counts.get_mut(tenth(v) * 3 + c) {
                *count += 1;
            }
        }
    }
    counts
}
fn places(pixels: &[[f32; 3]], width: usize) -> Vec<u32> {
    let mut counts = vec![0; 64 * 128];
    for (i, px) in pixels.iter().enumerate() {
        if (0.0..1.0).contains(&px[0]) {
            counts[(px[0] * 64.0) as usize * 128 + i % width * 128 / width] += 1;
        }
    }
    counts
}

/// Signed, out-of-range and NaN samples among a ramp.
fn pixels(len: usize) -> Vec<[f32; 3]> {
    (0..len)
        .map(|i| match i % 97 {
            0 => [-0.5, f32::NAN, 1.0],
            1 => [f32::INFINITY, 0.999_999_94, -0.0],
            _ => [(i % 101) as f32 / 101.0, (i * 7 % 53) as f32 / 50.0, (i % 13) as f32 * 0.07],
        })
        .collect()
}

fn rgb(width: u32, height: u32) -> RgbDesc {
    RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65))
}

/// Counts and sums of rasters across sizes that leave partial workgroups,
/// several combine passes and empty position columns.
fn rasters_reduce_like_the_host(runtime: RuntimeContext) {
    let global = GlobalContext::default();
    let ctx = KernelContext::new(&global, &runtime);
    for (width, height) in [(1, 1), (17, 15), (300, 300), (1000, 3)] {
        let pixels = pixels(width * height);
        let raster =
            Raster::<Rgb>::from_host(rgb(width as u32, height as u32), pixels.clone().into());
        let image = Image::from(raster.unwrap());
        assert_eq!(image.count::<Rgb>(&TENTHS, &[], &ctx).unwrap(), tenths(&pixels));
        assert_eq!(image.count::<Rgb>(&PLACES, &[], &ctx).unwrap(), places(&pixels, width));
        let finite: Vec<_> =
            pixels.iter().map(|px| px.map(|v| if v.is_finite() { v } else { 0.0 })).collect();
        let image = Image::from(
            Raster::<Rgb>::from_host(rgb(width as u32, height as u32), finite.clone().into())
                .unwrap(),
        );
        let sums = image.fold::<Rgb>(&SUMS, &[0.5], &ctx).unwrap();
        for c in 0..3 {
            let expected: f64 = finite.iter().map(|px| f64::from(px[c]) * 0.5).sum();
            assert!(
                (f64::from(sums[c]) - expected).abs() <= 1e-5 * expected.abs().max(1.0),
                "{c}: {sums:?}"
            );
        }
        assert!(image.fold::<Rgb>(&SUMS, &[], &ctx).is_err(), "SUMS reads one constant");
    }
}

/// A crop holds part of its lattice, and a reduction covers that part only,
/// with positions counted from the crop's first sample.
fn a_reduction_covers_the_samples_an_image_holds(runtime: RuntimeContext) {
    let global = GlobalContext::default();
    let ctx = KernelContext::new(&global, &runtime);
    let width = 40;
    let pixels = pixels(width * 30);
    let image = Image::from(Raster::<Rgb>::from_host(rgb(40, 30), pixels.clone().into()).unwrap());
    let window = Rect { origin: [5, 7], extent: [20, 9] };
    let crop = image.crop(window, &ctx).unwrap();
    let inside: Vec<_> =
        (7..16).flat_map(|y| (5..25).map(move |x| y * width + x)).map(|i| pixels[i]).collect();
    assert_eq!(crop.count::<Rgb>(&TENTHS, &[], &ctx).unwrap(), tenths(&inside));
    assert_eq!(crop.count::<Rgb>(&PLACES, &[], &ctx).unwrap(), places(&inside, 20));
}

/// Source → Exposure → Sigmoid: Exposure's node and the sigmoid's output,
/// deferred.
fn tail(dag: &mut Dag, width: u32, height: u32) -> (NodeId, Output<Rgb>) {
    let pixels = pixels((width * height) as usize);
    let source = node::source::<Rgb>(dag, rgb(width, height), pixels).unwrap();
    let exposure = exposure::add(dag, exposure::Exposure { ev: -0.4 }).unwrap();
    let sigmoid = sigmoid::apply::add(dag, Default::default()).unwrap();
    dag.connect(source, exposure.image).unwrap();
    dag.connect(exposure.output, sigmoid.image).unwrap();
    (exposure.output.node(), sigmoid.output)
}

fn counts_contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(CountsDesc,)> {
    Ok((CountsDesc { bins: 10, channels: 3, coordinates: image.coordinates.clone() },))
}
/// Counts per tenth of each channel.
#[drip::node(id = "test-tenths", name = "Tenths", contract = counts_contract, bins = TENTHS)]
fn tenths_node(_: &(), _image: Point<'_, Rgb>, _counts: Point<'_, Counts>) -> Result<[f32; 0]> {
    Ok([])
}

/// Shows its input.
#[drip::node(id = "test-view", name = "View")]
fn view(_: &KernelContext<'_>, _: &(), _image: Read<'_, Rgb>) -> Result<()> {
    Ok(())
}

/// Three sums per channel.
struct Sums;
impl Payload for Sums {
    const NAME: &'static str = "Sums";
    type Desc = ();
    type Host = [f32; 3];
    fn validate(_: &()) -> Result<()> {
        Ok(())
    }
    fn check(_: &(), _: &[f32; 3]) -> Result<()> {
        Ok(())
    }
    fn allocate(_: &()) -> [f32; 3] {
        [0.0; 3]
    }
    fn byte_len(_: &()) -> usize {
        12
    }
}
fn sums_contract(_: &GlobalContext, _: &(), _: &RgbDesc) -> Result<((),)> {
    Ok(((),))
}
/// Half the sum of each channel.
#[drip::node(id = "test-sums", name = "Sums", contract = sums_contract, reduce = SUMS)]
fn sums_node(_: &(), _image: Point<'_, Rgb>, _sums: Point<'_, Sums>) -> Result<[f32; 1]> {
    Ok([0.5])
}

/// The bins pass computes the deferred tail per pixel: counts equal those of
/// the materialized tail, without a fused launch or a download, and the
/// store keeps the counts. When other consumers read the same image, the
/// first to run materializes it once for all of them.
fn binning_a_deferred_image_does_not_store_it(runtime: RuntimeContext) {
    let (width, height) = (61, 23);
    let mut dag = Dag::new();
    let (exposure, sigmoid) = tail(&mut dag, width, height);
    let bins = tenths_node::add(&mut dag, ()).unwrap();
    dag.connect(sigmoid, bins.image).unwrap();
    let samples = Evaluator::new(runtime.clone())
        .evaluate::<Rgb>(&dag, &Default::default(), sigmoid)
        .unwrap();
    let expected = tenths(samples.data.host().unwrap());

    let evaluator = Evaluator::new(runtime);
    let global = GlobalContext::default();
    let batch = |dag: &Dag, targets: &[Target]| {
        evaluator.evaluate_batch(dag, &global, targets, Policy::Wait, None)
    };
    let mut first = batch(&dag, &[bins.node.into()]);
    let stats = &first.statistics;
    assert_eq!((stats.fused, stats.reductions, stats.downloads), (0, 1, 0), "{stats:?}");
    let values = first.inputs.remove(&bins.node).unwrap().unwrap();
    let outputs = values.bind_outputs::<tenths_node::Definition>().unwrap();
    assert_eq!(&**outputs.counts.data.host().unwrap(), expected.as_slice());
    assert_eq!(outputs.counts.desc.coordinates, Coordinates::Space(REC2020_D65));
    assert!(values.bind::<tenths_node::Definition>().is_err(), "a reduction hands its outputs");
    let again = batch(&dag, &[bins.node.into()]).statistics;
    assert_eq!((again.nodes, again.reductions, again.hits), (0, 0, 1), "{again:?}");

    let twice = tenths_node::add(&mut dag, ()).unwrap();
    dag.connect(sigmoid, twice.image).unwrap();
    dag.set_parameters(exposure, serde_json::json!({"ev": 0.3})).unwrap();
    let stats = batch(&dag, &[bins.node.into(), twice.node.into()]).statistics;
    assert_eq!((stats.fused, stats.reductions), (1, 2), "{stats:?}");

    let shown = view::add(&mut dag, ()).unwrap();
    dag.connect(sigmoid, shown.image).unwrap();
    dag.set_parameters(exposure, serde_json::json!({"ev": 0.6})).unwrap();
    let both = batch(&dag, &[bins.node.into(), shown.node.into()]);
    let stats = &both.statistics;
    assert_eq!((stats.fused, stats.reductions), (1, 1), "{stats:?}");
    assert!(both.inputs.values().all(Result::is_ok));
}

/// A reduce node publishes its accumulator, and the deferred tail folds to
/// the same bits as its samples.
fn a_reduce_node_publishes_its_accumulator(runtime: RuntimeContext) {
    let mut dag = Dag::new();
    let (_, sigmoid) = tail(&mut dag, 300, 300);
    let sums = sums_node::add(&mut dag, ()).unwrap();
    dag.connect(sigmoid, sums.image).unwrap();
    let evaluator = Evaluator::new(runtime.clone());
    let result = evaluator.evaluate::<Sums>(&dag, &Default::default(), sums.sums).unwrap();
    assert_eq!((result.statistics.fused, result.statistics.reductions), (0, 1));
    let global = GlobalContext::default();
    let ctx = KernelContext::new(&global, &runtime);
    let samples = Evaluator::new(runtime.clone())
        .evaluate::<Rgb>(&dag, &Default::default(), sigmoid)
        .unwrap();
    let stored = Image::from(samples.data).fold::<Rgb>(&SUMS, &[0.5], &ctx).unwrap();
    let bits = |v: &[f32]| v.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(result.data.host().unwrap()), bits(&stored));
}

/// A reduction runs at the requested scale on the reduced image, and a
/// region on a node with outputs is refused.
fn reductions_follow_the_scale_and_take_no_region(runtime: RuntimeContext) {
    let mut dag = Dag::new();
    let (_, sigmoid) = tail(&mut dag, 61, 23);
    let bins = tenths_node::add(&mut dag, ()).unwrap();
    dag.connect(sigmoid, bins.image).unwrap();
    let evaluator = Evaluator::new(runtime);
    let global = GlobalContext { scale: 4 };
    let reduced = evaluator.evaluate::<Rgb>(&dag, &global, sigmoid).unwrap();
    assert_eq!(reduced.desc.extent, Extent { width: 16, height: 6 });
    let counts = evaluator.evaluate::<Counts>(&dag, &global, bins.counts).unwrap();
    assert_eq!(&**counts.data.host().unwrap(), tenths(reduced.data.host().unwrap()).as_slice());
    let region = Some(Rect { origin: [0; 2], extent: [8, 8] });
    let target = Target { node: bins.node, region };
    let mut batch = evaluator.evaluate_batch(&dag, &global, &[target], Policy::Wait, None);
    assert!(batch.inputs.remove(&bins.node).unwrap().is_err());
}

fn all(runtime: RuntimeContext) {
    rasters_reduce_like_the_host(runtime.clone());
    a_reduction_covers_the_samples_an_image_holds(runtime.clone());
    binning_a_deferred_image_does_not_store_it(runtime.clone());
    a_reduce_node_publishes_its_accumulator(runtime.clone());
    reductions_follow_the_scale_and_take_no_region(runtime);
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_reduce() {
    all(RuntimeContext::cpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_reduce() {
    all(RuntimeContext::from_env().unwrap());
}
