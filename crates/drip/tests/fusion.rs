// SPDX-License-Identifier: GPL-3.0-or-later
// The legacy sigmoid kernel below is adapted from darktable. Source pins and
// numerical deviations: THIRD_PARTY.md.
//! Map, warp and resample nodes run as fused kernels: results match the
//! per-node kernels they replaced bit for bit, each segment launches once per
//! consumer that needs its samples, and segments cut only where composing
//! would change the result.
use cubecl::prelude::*;
use drip::{
    Result,
    eval::Evaluator,
    graph::Dag,
    image::Description,
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
    runtime::{GlobalContext, KernelContext, RuntimeContext},
    shape::{Pixel, Shape},
};
use std::sync::Arc;

/// A camera with a dense XYZ-to-camera matrix and as-shot gains, so the camera
/// and CAT16 matrices mix all three channels.
fn metadata() -> Arc<drip_raw::Metadata> {
    let mut m =
        drip_raw::Metadata { make: "Fixture".into(), model: "fusion".into(), ..Default::default() };
    m.calibration.xyz_to_cam[..3].copy_from_slice(&[
        [0.6640, -0.1847, -0.0503],
        [-0.5238, 1.3010, 0.2474],
        [-0.0993, 0.1673, 0.6527],
    ]);
    m.calibration.as_shot = [2.2, 1.0, 1.6, 1.0];
    Arc::new(m)
}

/// Signed, zero, tiny, huge, infinite and NaN samples, then a ramp: 41 × 9.
fn samples() -> Vec<[f32; 3]> {
    let special = [
        0.0,
        -0.0,
        0.18,
        1.0,
        -0.25,
        1e-30,
        3.0e38,
        -3.0e38,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::from_bits(0x7fc01234),
    ];
    let mut pixels: Vec<_> =
        special.iter().flat_map(|&a| special.iter().map(move |&b| [a, b, 0.3])).collect();
    pixels.extend((0..200).map(|i| {
        let t = i as f32 / 25.0 - 2.0;
        [t.exp2() * 0.18, (t * 0.7).sin(), 1.0 - t * 0.3]
    }));
    pixels
}

struct Chain {
    dag: Dag,
    source: Output<Rgb>,
    metadata: Output<CaptureMetadata>,
}
fn chain() -> Chain {
    let mut dag = Dag::new();
    let m = metadata();
    let response = Response { camera: Camera::of(&m), balance: None };
    let pixels = samples();
    let extent = Extent { width: 41, height: 9 };
    assert_eq!(extent.pixels(), pixels.len());
    let source =
        node::source::<Rgb>(&mut dag, RgbDesc::new(extent, Coordinates::Camera(response)), pixels)
            .unwrap();
    let metadata = node::source::<CaptureMetadata>(&mut dag, m.clone(), m).unwrap();
    Chain { dag, source, metadata }
}
fn exposure_settings() -> exposure::Exposure {
    exposure::Exposure { ev: 0.7 }
}
fn sigmoid_settings() -> sigmoid::Settings {
    sigmoid::Settings { contrast: 2.0, skew: 0.3, preserve_hue: 0.6 }
}

/// Camera matrix, CAT16, exposure and sigmoid as map nodes, after
/// `orientation` if one is given.
fn fused(chain: &mut Chain, orientation: Option<Orientation>) -> Output<Rgb> {
    let d = &mut chain.dag;
    let mut input = chain.source;
    if let Some(orientation) = orientation {
        let node = orientation::add(d, orientation::Settings { orientation }).unwrap();
        d.connect(input, node.image).unwrap();
        input = node.output;
    }
    let camera = node::camera_to_rgb::add(d, ()).unwrap();
    let cat = chromatic_adaptation::add(d, Default::default()).unwrap();
    let exposure = exposure::add(d, exposure_settings()).unwrap();
    let sigmoid = sigmoid::apply::add(d, sigmoid_settings()).unwrap();
    d.connect(input, camera.image).unwrap();
    d.connect(chain.metadata, camera.metadata).unwrap();
    d.connect(camera.output, cat.image).unwrap();
    d.connect(chain.metadata, cat.metadata).unwrap();
    d.connect(cat.output, exposure.image).unwrap();
    d.connect(exposure.output, sigmoid.image).unwrap();
    sigmoid.output
}

/// The same chain as kernel nodes launching the kernels the maps and the
/// warp replaced.
fn legacy(chain: &mut Chain, orientation: Option<Orientation>) -> Output<Rgb> {
    let d = &mut chain.dag;
    let mut input = chain.source;
    if let Some(orientation) = orientation {
        let node = legacy_orientation::add(d, orientation::Settings { orientation }).unwrap();
        d.connect(input, node.image).unwrap();
        input = node.output;
    }
    let camera = legacy_camera::add(d, ()).unwrap();
    let cat = legacy_cat16::add(d, Default::default()).unwrap();
    let exposure = legacy_exposure::add(d, exposure_settings()).unwrap();
    let sigmoid = legacy_sigmoid::add(d, sigmoid_settings()).unwrap();
    d.connect(input, camera.image).unwrap();
    d.connect(chain.metadata, camera.metadata).unwrap();
    d.connect(camera.output, cat.image).unwrap();
    d.connect(chain.metadata, cat.metadata).unwrap();
    d.connect(cat.output, exposure.image).unwrap();
    d.connect(exposure.output, sigmoid.image).unwrap();
    sigmoid.output
}

fn bits(evaluator: &Evaluator, dag: &Dag, output: Output<Rgb>) -> Vec<[u32; 3]> {
    let result = evaluator.evaluate::<Rgb>(dag, &Default::default(), output).unwrap();
    result.data.host().unwrap().iter().map(|p| p.map(f32::to_bits)).collect()
}

/// With and without an Orientation ahead of the maps, which joins their
/// kernel as an exact reindex.
fn fused_chain_matches_the_replaced_kernels(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let orientations = [
        None,
        Some(Orientation::MirrorHorizontal),
        Some(Orientation::Rotate180),
        Some(Orientation::MirrorVertical),
        Some(Orientation::Transpose),
        Some(Orientation::Rotate90Clockwise),
        Some(Orientation::Transverse),
        Some(Orientation::Rotate90Counterclockwise),
    ];
    for orientation in orientations {
        let mut chain = chain();
        let fused = fused(&mut chain, orientation);
        let legacy = legacy(&mut chain, orientation);
        let result = evaluator.evaluate::<Rgb>(&chain.dag, &Default::default(), fused).unwrap();
        assert_eq!(result.statistics.fused, 1);
        let finite = result.data.host().unwrap().iter().flatten().filter(|v| v.is_finite()).count();
        assert!(finite >= 600, "only {finite} finite samples");
        let (fused, legacy) =
            (bits(&evaluator, &chain.dag, fused), bits(&evaluator, &chain.dag, legacy));
        assert_eq!(fused, legacy, "{orientation:?}");
    }
}

fn copy_contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}
/// A kernel consumer: copies its input through the host.
#[drip::node(id = "test-host-copy", contract = copy_contract)]
fn host_copy(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Rgb>,
    mut output: Write<'_, Rgb>,
) -> Result<()> {
    output.host().copy_from_slice(image.data.host()?);
    Ok(())
}
fn pair_contract(_: &GlobalContext, _: &(), a: &RgbDesc, _: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((a.clone(),))
}
/// A kernel consumer of two images: their per-sample sum.
#[drip::node(id = "test-sum", contract = pair_contract)]
fn sum(
    _: &KernelContext<'_>,
    _: &(),
    a: Read<'_, Rgb>,
    b: Read<'_, Rgb>,
    mut output: Write<'_, Rgb>,
) -> Result<()> {
    let (a, b) = (a.data.host()?, b.data.host()?);
    for ((o, a), b) in output.host().iter_mut().zip(a.iter()).zip(b.iter()) {
        *o = std::array::from_fn(|c| a[c] + b[c]);
    }
    Ok(())
}

/// Negate every sample, a body without constants.
#[drip::node(id = "test-negate", contract = same, map = negate_body)]
fn negate(_: &(), _image: Point<'_, Rgb>, _output: Point<'_, Rgb>) -> Result<[f32; 0]> {
    Ok([])
}
#[cube]
fn negate_body(px: Pixel, _k: &[f32]) -> Pixel {
    Pixel { r: -px.r, g: -px.g, b: -px.b }
}

fn rec2020(dag: &mut Dag, pixels: Vec<[f32; 3]>) -> Output<Rgb> {
    let extent = Extent { width: pixels.len() as u32, height: 1 };
    node::source::<Rgb>(dag, RgbDesc::new(extent, Coordinates::Space(REC2020_D65)), pixels).unwrap()
}
fn gain(dag: &mut Dag, input: Output<Rgb>, ev: f32) -> Output<Rgb> {
    let node = exposure::add(dag, exposure::Exposure { ev }).unwrap();
    dag.connect(input, node.image).unwrap();
    node.output
}

fn deferred_images_materialize_once_per_consumer_that_needs_samples(runtime: RuntimeContext) {
    // A new evaluator per check, so no launch is saved by the store.
    let evaluate = |dag: &Dag, output| {
        Evaluator::new(runtime.clone()).evaluate::<Rgb>(dag, &Default::default(), output).unwrap()
    };
    let mut dag = Dag::new();
    let source = rec2020(&mut dag, vec![[0.25, -1.0, 3.0]; 3]);
    let first = gain(&mut dag, source, 1.0);
    let twice = gain(&mut dag, first, 2.0);

    // `evaluate` materializes a map output, and two maps launch one kernel.
    let result = evaluate(&dag, twice);
    assert_eq!(result.statistics.fused, 1);
    assert_eq!(result.statistics.nodes, 3);
    assert_eq!(**result.data.host().unwrap(), [[2.0, -8.0, 24.0]; 3]);

    // A kernel consumer after two maps reads their samples.
    let copy = host_copy::add(&mut dag, ()).unwrap();
    dag.connect(twice, copy.image).unwrap();
    let result = evaluate(&dag, copy.output);
    assert_eq!(result.statistics.fused, 1);
    assert_eq!(**result.data.host().unwrap(), [[2.0, -8.0, 24.0]; 3]);

    // Two consumers of one deferred image share its materialization.
    let pair = sum::add(&mut dag, ()).unwrap();
    dag.connect(twice, pair.a).unwrap();
    dag.connect(twice, pair.b).unwrap();
    let result = evaluate(&dag, pair.output);
    assert_eq!(result.statistics.fused, 1);
    assert_eq!(**result.data.host().unwrap(), [[4.0, -16.0, 48.0]; 3]);

    // Maps after a fan-out each extend the shared plan, so nothing
    // materializes `first` itself.
    let left = gain(&mut dag, first, -1.0);
    let right = gain(&mut dag, first, 1.0);
    let pair = sum::add(&mut dag, ()).unwrap();
    dag.connect(left, pair.a).unwrap();
    dag.connect(right, pair.b).unwrap();
    let result = evaluate(&dag, pair.output);
    assert_eq!(result.statistics.fused, 2);
    assert_eq!(**result.data.host().unwrap(), [[1.25, -5.0, 15.0]; 3]);

    // A plan may read no constants.
    let negated = negate::add(&mut dag, ()).unwrap();
    dag.connect(source, negated.image).unwrap();
    let result = evaluate(&dag, negated.output);
    assert_eq!(result.statistics.fused, 1);
    assert_eq!(**result.data.host().unwrap(), [[-0.25, 1.0, -3.0]; 3]);
}

/// The template's colour tail: Orientation between CAT16 and exposure is a
/// reindex, so it joins the maps' kernel and the tail launches once.
fn template_tail_launches_once(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut chain = chain();
    let d = &mut chain.dag;
    let camera = node::camera_to_rgb::add(d, ()).unwrap();
    let cat = chromatic_adaptation::add(d, Default::default()).unwrap();
    let orientation = orientation::add(d, Default::default()).unwrap();
    let exposure = exposure::add(d, exposure_settings()).unwrap();
    let sigmoid = sigmoid::apply::add(d, sigmoid_settings()).unwrap();
    d.connect(chain.source, camera.image).unwrap();
    d.connect(chain.metadata, camera.metadata).unwrap();
    d.connect(camera.output, cat.image).unwrap();
    d.connect(chain.metadata, cat.metadata).unwrap();
    d.connect(cat.output, orientation.image).unwrap();
    d.connect(chain.metadata, orientation.metadata).unwrap();
    d.connect(orientation.output, exposure.image).unwrap();
    d.connect(exposure.output, sigmoid.image).unwrap();
    let to_orientation =
        evaluator.evaluate::<Rgb>(d, &Default::default(), orientation.output).unwrap();
    assert_eq!(to_orientation.statistics.fused, 1);
    // A new evaluator, so the tail does not start from the stored orientation.
    let evaluator = Evaluator::new(evaluator.runtime().clone());
    let to_sigmoid = evaluator.evaluate::<Rgb>(d, &Default::default(), sigmoid.output).unwrap();
    assert_eq!(to_sigmoid.statistics.fused, 1);
    assert_eq!(to_sigmoid.statistics.uploads, 1);
    for kind in [&exposure::compute::KIND, &sigmoid::apply::KIND] {
        assert!(matches!(kind.shape, Shape::Map(_)), "{}", kind.id);
    }
    assert_eq!(orientation::compute::KIND.shape, Shape::Warp);
    assert_eq!(resample::compute::KIND.shape, Shape::Resample);
}

fn half() -> resample::Settings {
    resample::Settings { filter: Filter::Lanczos3, size: Size::Factor(Factor { factor: 0.5 }) }
}
fn resampled(dag: &mut Dag, input: Output<Rgb>, settings: resample::Settings) -> Output<Rgb> {
    let node = resample::add(dag, settings).unwrap();
    dag.connect(input, node.image).unwrap();
    node.output
}
fn transformed(dag: &mut Dag, input: Output<Rgb>, quad: [[f64; 2]; 4]) -> Output<Rgb> {
    let p = perspective::Settings::default().with_quad(quad, Aspect::Corners);
    let node = perspective::add(dag, p).unwrap();
    dag.connect(input, node.image).unwrap();
    node.output
}
fn copied(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    let node = host_copy::add(dag, ()).unwrap();
    dag.connect(input, node.image).unwrap();
    node.output
}
fn sigmoid(dag: &mut Dag, input: Output<Rgb>) -> Output<Rgb> {
    let node = sigmoid::apply::add(dag, sigmoid_settings()).unwrap();
    dag.connect(input, node.image).unwrap();
    node.output
}

/// The fused launches evaluating `output` takes, and its sample bits.
fn launches(evaluator: &Evaluator, dag: &Dag, output: Output<Rgb>) -> (usize, Vec<[u32; 3]>) {
    let result = evaluator.evaluate::<Rgb>(dag, &Default::default(), output).unwrap();
    let bits = result.data.host().unwrap().iter().map(|p| p.map(f32::to_bits)).collect();
    (result.statistics.fused, bits)
}

/// A filtering step after a nonlinear map resamples the map's output in a
/// second launch, with the bits a materialization between them gives. Before
/// the map it fuses. A reindex joins the stage wherever it sits, and steps
/// that move nothing launch nothing.
fn segments_cut_only_where_composing_changes_the_result(runtime: RuntimeContext) {
    // A new evaluator per evaluation, so every count starts from the source.
    let launches = |dag: &Dag, output| launches(&Evaluator::new(runtime.clone()), dag, output);
    let mut dag = Dag::new();
    let d = &mut dag;
    let pixels: Vec<_> =
        (0..12 * 8).map(|i| [i as f32 / 20.0, 1.0 - i as f32 / 50.0, 0.4]).collect();
    let desc = RgbDesc::new(Extent { width: 12, height: 8 }, Coordinates::Space(REC2020_D65));
    let source = node::source::<Rgb>(d, desc, pixels).unwrap();
    let mapped = sigmoid(d, source);
    let materialized = copied(d, mapped);

    // A Resample after a map: two launches, as with a kernel between them.
    let cut = resampled(d, mapped, half());
    let separate = resampled(d, materialized, half());
    let (fused, bits) = launches(d, cut);
    assert_eq!(fused, 2);
    assert_eq!(bits, launches(d, separate).1);

    // A Resample before a map: one launch, with the bits of two kernels.
    let first = resampled(d, source, half());
    let after = sigmoid(d, first);
    let first_copy = copied(d, first);
    let separate = sigmoid(d, first_copy);
    let (fused, bits) = launches(d, after);
    assert_eq!(fused, 1);
    assert_eq!(bits, launches(d, separate).1);

    // Perspective Transform after a map: a rotation filters and cuts, while
    // an integer crop is a reindex and joins the map's kernel.
    let rotated = [[0.1, 0.0], [1.0, 0.1], [0.9, 1.0], [0.0, 0.9]];
    let crop = [[0.25, 0.0], [0.75, 0.0], [0.75, 0.5], [0.25, 0.5]];
    let rotation = transformed(d, mapped, rotated);
    assert_eq!(launches(d, rotation).0, 2);
    let cropped = transformed(d, mapped, crop);
    let separate = transformed(d, materialized, crop);
    let (fused, bits) = launches(d, cropped);
    assert_eq!(fused, 1);
    assert_eq!(bits, launches(d, separate).1);

    // Warps compose with each other and with a Resample after them. A
    // Resample fixes its lattice, so a filtering warp after it cuts.
    let once = transformed(d, source, rotated);
    let twice = transformed(d, once, rotated);
    let both = resampled(d, twice, half());
    assert_eq!(launches(d, both).0, 1);
    let rotation = transformed(d, first, rotated);
    assert_eq!(launches(d, rotation).0, 2);

    // The identity, and two quarter turns that cancel, share the source.
    let unchanged = launches(d, source);
    assert_eq!(unchanged.0, 0);
    let identity = transformed(d, source, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    assert_eq!(launches(d, identity), unchanged);
    let clockwise = turned(d, source, Orientation::Rotate90Clockwise);
    let back = turned(d, clockwise, Orientation::Rotate90Counterclockwise);
    assert_eq!(launches(d, back), unchanged);
}

fn turned(dag: &mut Dag, input: Output<Rgb>, orientation: Orientation) -> Output<Rgb> {
    let node = orientation::add(dag, orientation::Settings { orientation }).unwrap();
    dag.connect(input, node.image).unwrap();
    node.output
}

fn all(runtime: RuntimeContext) {
    fused_chain_matches_the_replaced_kernels(runtime.clone());
    deferred_images_materialize_once_per_consumer_that_needs_samples(runtime.clone());
    template_tail_launches_once(runtime.clone());
    segments_cut_only_where_composing_changes_the_result(runtime);
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_fusion() {
    all(RuntimeContext::cpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_fusion() {
    all(RuntimeContext::from_env().unwrap());
}

fn described(desc: &RgbDesc) -> Description {
    Arc::new(desc.clone())
}
fn launch(
    ctx: &KernelContext<'_>,
    image: &Read<'_, Rgb>,
    output: &Write<'_, Rgb>,
    constants: &[f32],
    elements: usize,
    kernel: fn(&Client, CubeCount, CubeDim, BufferArg, BufferArg, BufferArg),
) -> Result<()> {
    let groups = elements.div_ceil(256) as u32;
    let constants = ctx.constants(constants)?;
    kernel(
        ctx.client()?,
        CubeCount::Static(groups, 1, 1),
        CubeDim::new_1d(256),
        image.data.device(ctx)?.argument(),
        constants.argument(),
        output.device(ctx)?.argument(),
    );
    Ok(())
}

fn to_rec2020(
    _: &GlobalContext,
    _: &(),
    image: &RgbDesc,
    _: &Arc<drip_raw::Metadata>,
) -> Result<(RgbDesc,)> {
    Ok((RgbDesc { coordinates: Coordinates::Space(REC2020_D65), ..image.clone() },))
}
#[drip::node(id = "legacy-camera-to-color-rgb", contract = to_rec2020)]
fn legacy_camera(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Rgb>,
    metadata: Read<'_, CaptureMetadata>,
    output: Write<'_, Rgb>,
) -> Result<()> {
    let (input, out) = (described(image.desc), described(output.desc));
    let k = node::camera_to_rgb::compute(&(), Point::new(&input), metadata, Point::new(&out))?;
    launch(ctx, &image, &output, &k, image.desc.extent.pixels(), legacy::matrix::launch)
}
fn same_with_metadata(
    _: &GlobalContext,
    _: &chromatic_adaptation::Settings,
    image: &RgbDesc,
    _: &Arc<drip_raw::Metadata>,
) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}
#[drip::node(id = "legacy-chromatic-adaptation", contract = same_with_metadata)]
fn legacy_cat16(
    ctx: &KernelContext<'_>,
    p: &chromatic_adaptation::Settings,
    image: Read<'_, Rgb>,
    metadata: Read<'_, CaptureMetadata>,
    output: Write<'_, Rgb>,
) -> Result<()> {
    let (input, out) = (described(image.desc), described(output.desc));
    let k = chromatic_adaptation::compute(p, Point::new(&input), Some(metadata), Point::new(&out))?;
    launch(ctx, &image, &output, &k, image.desc.extent.pixels(), legacy::matrix::launch)
}
fn same<P>(_: &GlobalContext, _: &P, image: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}
#[drip::node(id = "legacy-exposure", contract = same)]
fn legacy_exposure(
    ctx: &KernelContext<'_>,
    p: &exposure::Exposure,
    image: Read<'_, Rgb>,
    output: Write<'_, Rgb>,
) -> Result<()> {
    let (input, out) = (described(image.desc), described(output.desc));
    let [gain] = exposure::compute(p, Point::new(&input), Point::new(&out))?;
    let elements = image.desc.extent.pixels() * 3;
    let groups = elements.div_ceil(256) as u32;
    legacy::exposure::launch(
        ctx.client()?,
        CubeCount::Static(groups, 1, 1),
        CubeDim::new_1d(256),
        image.data.device(ctx)?.argument(),
        output.device(ctx)?.argument(),
        gain,
    );
    Ok(())
}
#[drip::node(id = "legacy-sigmoid", contract = same)]
fn legacy_sigmoid(
    ctx: &KernelContext<'_>,
    p: &sigmoid::Settings,
    image: Read<'_, Rgb>,
    output: Write<'_, Rgb>,
) -> Result<()> {
    let (input, out) = (described(image.desc), described(output.desc));
    let k = sigmoid::apply(p, Point::new(&input), Point::new(&out))?;
    launch(ctx, &image, &output, &k, image.desc.extent.pixels(), legacy::sigmoid::launch)
}

fn oriented(_: &GlobalContext, p: &orientation::Settings, image: &RgbDesc) -> Result<(RgbDesc,)> {
    let mut output = image.clone();
    if legacy_code(p.orientation) >= 5 {
        std::mem::swap(&mut output.extent.width, &mut output.extent.height);
        std::mem::swap(&mut output.full.width, &mut output.full.height);
    }
    Ok((output,))
}
fn legacy_code(orientation: Orientation) -> usize {
    match orientation {
        Orientation::FromMetadata | Orientation::Identity => 1,
        Orientation::MirrorHorizontal => 2,
        Orientation::Rotate180 => 3,
        Orientation::MirrorVertical => 4,
        Orientation::Transpose => 5,
        Orientation::Rotate90Clockwise => 6,
        Orientation::Transverse => 7,
        Orientation::Rotate90Counterclockwise => 8,
    }
}
#[drip::node(id = "legacy-orientation", contract = oriented)]
fn legacy_orientation(
    ctx: &KernelContext<'_>,
    p: &orientation::Settings,
    image: Read<'_, Rgb>,
    output: Write<'_, Rgb>,
) -> Result<()> {
    let groups = image.desc.extent.pixels().div_ceil(256) as u32;
    legacy::orient::launch(
        ctx.client()?,
        CubeCount::Static(groups, 1, 1),
        CubeDim::new_1d(256),
        image.data.device(ctx)?.argument(),
        output.device(ctx)?.argument(),
        image.desc.extent.width as usize,
        image.desc.extent.height as usize,
        legacy_code(p.orientation),
    );
    Ok(())
}

/// The per-node kernels of Drip 3479319, with unchanged arithmetic.
mod legacy {
    use cubecl::prelude::*;

    #[cube(launch)]
    pub fn orient(
        input: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
        orientation: usize,
    ) {
        let i = ABSOLUTE_POS;
        if i < width * height {
            let mut x = i % width;
            let mut y = i / width;
            if orientation == 2 || orientation == 3 || orientation == 7 || orientation == 8 {
                x = width - 1 - x;
            }
            if orientation == 3 || orientation == 4 || orientation == 6 || orientation == 7 {
                y = height - 1 - y;
            }
            let mut target = y * width + x;
            if orientation >= 5 {
                target = x * height + y;
            }
            for c in 0..3 {
                output[target * 3 + c] = input[i * 3 + c];
            }
        }
    }

    #[cube(launch)]
    pub fn matrix(input: &[f32], coeff: &[f32], output: &mut [f32]) {
        let i = ABSOLUTE_POS * 3;
        if i < input.len() {
            let r = input[i];
            let g = input[i + 1];
            let b = input[i + 2];
            for c in 0..3 {
                output[i + c] = coeff[c * 3] * r + coeff[c * 3 + 1] * g + coeff[c * 3 + 2] * b;
            }
        }
    }

    #[cube(launch)]
    pub fn exposure(input: &[f32], output: &mut [f32], gain: f32) {
        let i = ABSOLUTE_POS;
        if i < input.len() {
            output[i] = input[i] * gain;
        }
    }

    #[cube]
    fn down64(v: f32) -> f32 {
        let bits = u32::reinterpret(v);
        if ((bits >> 23) & 255) > 64 {
            f32::reinterpret(bits - (64u32 << 23))
        } else {
            v * 5.421_011e-20_f32
        }
    }
    #[cube]
    fn curve(value: f32, p: &[f32], log_scale: f32) -> f32 {
        if value <= 0.0f32 {
            0.0f32
        } else {
            let z = p[2] - p[0] * (value.ln() + log_scale);
            let softplus = z.max(0.0f32) + (1.0f32 + (-z.abs()).exp()).ln();
            (-p[1] * softplus).exp()
        }
    }
    #[allow(clippy::manual_swap)]
    #[cube(launch)]
    pub fn sigmoid(input: &[f32], p: &[f32], output: &mut [f32]) {
        let i = ABSOLUTE_POS * 3;
        if i < input.len() {
            let mut rgb = Array::<f32>::new(3usize);
            let mut work = Array::<f32>::new(3usize);
            let mut mapped = Array::<f32>::new(3usize);
            let magnitude = input[i].abs().max(input[i + 1].abs()).max(input[i + 2].abs());
            let input_scale = if magnitude > 1e20f32 { 18446744073709551616.0f32 } else { 1.0f32 };
            let mut bounded = Array::<f32>::new(3usize);
            for c in 0..3 {
                bounded[c] = if magnitude > 1e20f32 { down64(input[i + c]) } else { input[i + c] };
            }
            let log_scale = input_scale.ln();
            let average =
                (bounded[0] / 3.0f32 + bounded[1] / 3.0f32 + bounded[2] / 3.0f32).max(0.0f32);
            let minimum = bounded[0].min(bounded[1]).min(bounded[2]);
            let scale = average.max(-minimum);
            let saturation = if minimum < 0.0f32 {
                (average / scale) / (average / scale - minimum / scale)
            } else {
                1.0f32
            };
            for c in 0..3 {
                rgb[c] = ((1.0f32 - saturation) * average + saturation * bounded[c]).max(0.0f32);
            }
            for c in 0..3 {
                work[c] = p[4 + c * 3] * rgb[0] + p[5 + c * 3] * rgb[1] + p[6 + c * 3] * rgb[2];
                mapped[c] = curve(work[c], p, log_scale);
            }
            let mut lo = 0usize;
            let mut mid = 1usize;
            let mut hi = 2usize;
            if work[lo] > work[mid] {
                let t = lo;
                lo = mid;
                mid = t;
            }
            if work[mid] > work[hi] {
                let t = mid;
                mid = hi;
                hi = t;
            }
            if work[lo] > work[mid] {
                let t = lo;
                lo = mid;
                mid = t;
            }
            let chroma = work[hi] - work[lo];
            let fraction = if chroma != 0.0f32 { (work[mid] - work[lo]) / chroma } else { 0.0f32 };
            let hue = p[3];
            let corrected = mapped[lo] + (mapped[hi] - mapped[lo]) * fraction;
            let naive = (1.0f32 - hue) * mapped[mid] + hue * corrected;
            let scale2 = work[lo].max(work[mid]);
            let blend = if scale2 != 0.0f32 {
                2.0f32 * (work[lo] / scale2) / (work[lo] / scale2 + work[mid] / scale2)
            } else {
                0.0f32
            };
            let energy = blend * (mapped[0] + mapped[1] + mapped[2])
                + (1.0f32 - blend) * (mapped[lo] + naive + mapped[hi]);
            if naive <= mapped[mid] {
                mapped[mid] = ((1.0f32 - hue) * mapped[mid]
                    + hue * (fraction * mapped[hi] + (1.0f32 - fraction) * (energy - mapped[hi])))
                    / (1.0f32 + hue * (1.0f32 - fraction));
                mapped[lo] = energy - mapped[hi] - mapped[mid];
            } else {
                mapped[mid] = ((1.0f32 - hue) * mapped[mid]
                    + hue * (mapped[lo] * (1.0f32 - fraction) + fraction * (energy - mapped[lo])))
                    / (1.0f32 + hue * fraction);
                mapped[hi] = energy - mapped[lo] - mapped[mid];
            }
            for c in 0..3 {
                output[i + c] = p[13 + c * 3] * mapped[0]
                    + p[14 + c * 3] * mapped[1]
                    + p[15 + c * 3] * mapped[2];
            }
        }
    }
}
