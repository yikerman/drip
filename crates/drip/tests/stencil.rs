//! Stencils read a window of their materialized input around each sample.
//! Sharpen turns a delta into its unsharp-mask kernel, its reach converts to
//! the samples of a preview lattice, maps after it join its kernel, and a
//! crop through it computes the crop and its halo only, bit for bit equal to
//! the whole image.
use drip::{
    Result,
    eval::{Evaluator, Policy, Statistics, Target},
    graph::Dag,
    node::{self, data::*, exposure, sharpen},
    ports::*,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
    shape::Rect,
};

fn rgb(width: u32, height: u32) -> RgbDesc {
    RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65))
}

/// One at `(x, y)` in every channel, zero elsewhere.
fn delta(dag: &mut Dag, width: u32, height: u32, [x, y]: [u32; 2]) -> Output<Rgb> {
    let mut pixels = vec![[0.0f32; 3]; (width * height) as usize];
    pixels[(y * width + x) as usize] = [1.0; 3];
    node::source::<Rgb>(dag, rgb(width, height), pixels).unwrap()
}

/// Finite samples that vary along both axes, signed and above one.
fn pattern(dag: &mut Dag, width: u32, height: u32) -> Output<Rgb> {
    let pixels: Vec<_> = (0..width * height)
        .map(|i| {
            let (x, y) = ((i % width) as f32, (i / width) as f32);
            [(x * 0.37).sin() * 2.0 + y * 0.01, (y * 0.29).cos() + x * 0.02, (x - y) * 0.03]
        })
        .collect();
    node::source::<Rgb>(dag, rgb(width, height), pixels).unwrap()
}

fn sharpened(dag: &mut Dag, input: Output<Rgb>, radius: f32, amount: f32) -> Output<Rgb> {
    let sharpen = sharpen::add(dag, sharpen::Settings { radius, amount }).unwrap();
    dag.connect(input, sharpen.image).unwrap();
    sharpen.output
}

fn exposed(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    let exposure = exposure::add(dag, exposure::Exposure { ev: 0.7 }).unwrap();
    dag.connect(input, exposure.image).unwrap();
    exposure.output
}

/// A view that forces its input.
#[drip::node(id = "test-sink")]
fn sink(_: &KernelContext<'_>, _: &(), _image: Read<'_, Rgb>) -> Result<()> {
    Ok(())
}

fn same(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}
/// Forwards its materialized input, so the segment before it ends there.
#[drip::node(id = "test-forward", contract = same)]
fn forward(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Rgb>,
    output: Forward<'_, Rgb>,
) -> Result<()> {
    output.set(image.data)
}

fn evaluate(
    runtime: &RuntimeContext,
    dag: &Dag,
    scale: u32,
    output: Output<Rgb>,
) -> (RgbDesc, Vec<[f32; 3]>, Statistics) {
    let result = Evaluator::new(runtime.clone())
        .evaluate::<Rgb>(dag, &GlobalContext { scale }, output)
        .unwrap();
    (result.desc, result.data.host().unwrap().to_vec(), result.statistics)
}

/// The Gaussian weight of offset `(dx, dy)` for standard deviation `sigma`.
fn gaussian(sigma: f64, dx: i32, dy: i32) -> f64 {
    (-(f64::from(dx * dx + dy * dy)) / (2.0 * sigma * sigma)).exp()
}

/// A delta sharpens to `δ + amount · (δ − G)`, with `G` the Gaussian
/// normalized over the reach of 2.5 standard deviations, rounded up to
/// samples, and nothing beyond it.
fn a_delta_sharpens_to_its_kernel(runtime: RuntimeContext) {
    let (width, height, centre) = (21, 17, [10, 8]);
    let (sigma, amount) = (1.6, 0.5);
    let mut dag = Dag::new();
    let input = delta(&mut dag, width, height, centre);
    let output = sharpened(&mut dag, input, sigma as f32, amount as f32);
    let (_, pixels, statistics) = evaluate(&runtime, &dag, 1, output);
    assert_eq!((statistics.fused, statistics.approximated), (1, 0), "{statistics:?}");
    let reach = 4;
    let total: f64 = (-reach..=reach)
        .flat_map(|dy| (-reach..=reach).map(move |dx| gaussian(sigma, dx, dy)))
        .sum();
    for (i, px) in pixels.iter().enumerate() {
        let dx = (i as u32 % width) as i32 - centre[0] as i32;
        let dy = (i as u32 / width) as i32 - centre[1] as i32;
        let expected = if dx.abs() > reach || dy.abs() > reach {
            0.0
        } else {
            let delta = f64::from(u8::from(dx == 0 && dy == 0));
            delta + amount * (delta - gaussian(sigma, dx, dy) / total)
        };
        for v in px {
            assert!((f64::from(*v) - expected).abs() < 1e-6, "({dx}, {dy}): {v} vs {expected}");
        }
    }
}

/// The offsets from `centre` of the samples a stencil changed.
fn support(pixels: &[[f32; 3]], width: u32, centre: [u32; 2]) -> (i32, i32) {
    let offsets = pixels.iter().enumerate().filter(|(_, px)| px[0] != 0.0).map(|(i, _)| {
        let dx = (i as u32 % width) as i32 - centre[0] as i32;
        let dy = (i as u32 / width) as i32 - centre[1] as i32;
        (dx.abs(), dy.abs())
    });
    offsets.fold((0, 0), |(x, y), (dx, dy)| (x.max(dx), y.max(dy)))
}

/// The reach converts at the lattice the stencil reads: a reach of 4
/// full-detail pixels is 4 samples at scale 1 and 2 at scale 2, where the
/// delta arrives box-reduced. A reach below one preview sample is counted.
fn the_radius_converts_at_the_lattice(runtime: RuntimeContext) {
    let mut dag = Dag::new();
    let input = delta(&mut dag, 64, 48, [32, 24]);
    let output = sharpened(&mut dag, input, 1.6, 0.5);
    let (desc, pixels, _) = evaluate(&runtime, &dag, 1, output);
    assert_eq!(support(&pixels, desc.extent.width, [32, 24]), (4, 4));
    let (desc, pixels, statistics) = evaluate(&runtime, &dag, 2, output);
    assert_eq!(desc.extent, Extent { width: 32, height: 24 });
    assert_eq!(support(&pixels, 32, [16, 12]), (2, 2));
    assert_eq!(statistics.approximated, 0);

    let mut dag = Dag::new();
    let input = delta(&mut dag, 64, 48, [32, 24]);
    let output = sharpened(&mut dag, input, 0.25, 0.5);
    assert_eq!(evaluate(&runtime, &dag, 1, output).2.approximated, 0);
    assert_eq!(evaluate(&runtime, &dag, 2, output).2.approximated, 1);
}

/// Maps after a stencil run in its kernel, with the bits of the stencil's
/// materialized output mapped on its own. Maps before it run in a kernel of
/// their own.
fn maps_after_a_stencil_join_its_kernel(runtime: RuntimeContext) {
    let mut dag = Dag::new();
    let input = pattern(&mut dag, 37, 29);
    let sharp = sharpened(&mut dag, input, 1.2, 0.8);
    let fused = exposed(&mut dag, sharp);
    let cut = forward::add(&mut dag, ()).unwrap();
    dag.connect(sharp, cut.image).unwrap();
    let apart = exposed(&mut dag, cut.output);
    let before = exposed(&mut dag, input);
    let after = sharpened(&mut dag, before, 1.2, 0.8);

    let (_, fused, statistics) = evaluate(&runtime, &dag, 1, fused);
    assert_eq!(statistics.fused, 1, "{statistics:?}");
    let (_, apart, statistics) = evaluate(&runtime, &dag, 1, apart);
    assert_eq!(statistics.fused, 2, "{statistics:?}");
    let bits =
        |pixels: &[[f32; 3]]| -> Vec<_> { pixels.iter().map(|px| px.map(f32::to_bits)).collect() };
    assert_eq!(bits(&fused), bits(&apart));
    assert_eq!(evaluate(&runtime, &dag, 1, after).2.fused, 2);
}

/// `sink`'s input over `region` at full detail, with the call's statistics.
fn view(
    evaluator: &Evaluator,
    dag: &Dag,
    sink: NodeId,
    region: Option<Rect>,
) -> (RgbDesc, Vec<u32>, Statistics) {
    let target = Target { node: sink, region };
    let mut batch =
        evaluator.evaluate_batch(dag, &GlobalContext::default(), &[target], Policy::Wait, None);
    let values = batch.inputs.remove(&sink).unwrap().unwrap();
    let image = values.bind::<sink::Definition>().unwrap().image;
    let bits = image.data.host().unwrap().iter().flat_map(|px| px.map(f32::to_bits)).collect();
    (image.desc.clone(), bits, batch.statistics)
}

/// A crop through Exposure and Sharpen computes the Exposure segment on the
/// crop grown by the reach, clamped to the image, then the stencil on the
/// crop, and equals the whole image's samples there bit for bit.
fn a_crop_through_a_stencil_computes_the_crop_and_its_halo(runtime: RuntimeContext) {
    let (width, height) = (53, 41);
    let mut dag = Dag::new();
    let input = pattern(&mut dag, width, height);
    let exposed = exposed(&mut dag, input);
    let output = sharpened(&mut dag, exposed, 1.2, 0.8);
    let shown = sink::add(&mut dag, ()).unwrap();
    dag.connect(output, shown.image).unwrap();
    let (_, whole, _) = view(&Evaluator::new(runtime.clone()), &dag, shown.node, None);
    // A reach of 3 pixels: 2.5 · 1.2 rounded up.
    for (window, halo) in [
        (Rect { origin: [10, 12], extent: [20, 9] }, Rect { origin: [7, 9], extent: [26, 15] }),
        (Rect { origin: [0, 30], extent: [5, 11] }, Rect { origin: [0, 27], extent: [8, 14] }),
    ] {
        let (desc, crop, statistics) =
            view(&Evaluator::new(runtime.clone()), &dag, shown.node, Some(window));
        assert_eq!(desc.extent, Extent { width: window.extent[0], height: window.extent[1] });
        let expected: Vec<_> = (window.origin[1]..window.end()[1])
            .flat_map(|y| {
                (window.origin[0]..window.end()[0]).map(move |x| (y * width + x) as usize)
            })
            .flat_map(|i| whole[i * 3..i * 3 + 3].to_vec())
            .collect();
        assert_eq!(crop, expected);
        assert_eq!(statistics.fused, 2, "{statistics:?}");
        assert_eq!(statistics.samples, window.cells() + halo.cells(), "{statistics:?}");
    }
}

fn all(runtime: RuntimeContext) {
    a_delta_sharpens_to_its_kernel(runtime.clone());
    the_radius_converts_at_the_lattice(runtime.clone());
    maps_after_a_stencil_join_its_kernel(runtime.clone());
    a_crop_through_a_stencil_computes_the_crop_and_its_halo(runtime);
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_stencil() {
    all(RuntimeContext::cpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_stencil() {
    all(RuntimeContext::from_env().unwrap());
}
