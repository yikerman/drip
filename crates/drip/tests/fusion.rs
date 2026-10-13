// SPDX-License-Identifier: GPL-3.0-or-later
// The legacy sigmoid kernel below is adapted from darktable. Source pins and
// numerical deviations: THIRD_PARTY.md.
//! Map nodes run as fused kernels: results match the per-node kernels they
//! replaced bit for bit, and each chain of maps launches once per consumer
//! that needs its samples.
use cubecl::prelude::*;
use drip::{
    Result,
    eval::Evaluator,
    graph::Dag,
    image::Description,
    node::{self, chromatic_adaptation, data::*, exposure, sigmoid},
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

/// Signed, zero, tiny, huge, infinite and NaN samples, then a ramp.
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
    let extent = Extent { width: pixels.len() as u32, height: 1 };
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

/// Camera matrix, CAT16, exposure and sigmoid as map nodes.
fn fused(chain: &mut Chain) -> Output<Rgb> {
    let d = &mut chain.dag;
    let camera = node::camera_to_rgb::add(d, ()).unwrap();
    let cat = chromatic_adaptation::add(d, Default::default()).unwrap();
    let exposure = exposure::add(d, exposure_settings()).unwrap();
    let sigmoid = sigmoid::apply::add(d, sigmoid_settings()).unwrap();
    d.connect(chain.source, camera.image).unwrap();
    d.connect(chain.metadata, camera.metadata).unwrap();
    d.connect(camera.output, cat.image).unwrap();
    d.connect(chain.metadata, cat.metadata).unwrap();
    d.connect(cat.output, exposure.image).unwrap();
    d.connect(exposure.output, sigmoid.image).unwrap();
    sigmoid.output
}

/// The same chain as kernel nodes launching the kernels the maps replaced.
fn legacy(chain: &mut Chain) -> Output<Rgb> {
    let d = &mut chain.dag;
    let camera = legacy_camera::add(d, ()).unwrap();
    let cat = legacy_cat16::add(d, Default::default()).unwrap();
    let exposure = legacy_exposure::add(d, exposure_settings()).unwrap();
    let sigmoid = legacy_sigmoid::add(d, sigmoid_settings()).unwrap();
    d.connect(chain.source, camera.image).unwrap();
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

fn fused_chain_matches_the_replaced_kernels(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut chain = chain();
    let fused = fused(&mut chain);
    let legacy = legacy(&mut chain);
    let result = evaluator.evaluate::<Rgb>(&chain.dag, &Default::default(), fused).unwrap();
    assert_eq!(result.statistics.fused, 1);
    let finite = result.data.host().unwrap().iter().flatten().filter(|v| v.is_finite()).count();
    assert!(finite >= 600, "only {finite} finite samples");
    assert_eq!(bits(&evaluator, &chain.dag, fused), bits(&evaluator, &chain.dag, legacy));
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
    let evaluator = Evaluator::new(runtime);
    let mut dag = Dag::new();
    let source = rec2020(&mut dag, vec![[0.25, -1.0, 3.0]; 3]);
    let first = gain(&mut dag, source, 1.0);
    let twice = gain(&mut dag, first, 2.0);

    // `evaluate` materializes a map output, and two maps launch one kernel.
    let result = evaluator.evaluate::<Rgb>(&dag, &Default::default(), twice).unwrap();
    assert_eq!(result.statistics.fused, 1);
    assert_eq!(result.statistics.nodes, 3);
    assert_eq!(**result.data.host().unwrap(), [[2.0, -8.0, 24.0]; 3]);

    // A kernel consumer after two maps reads their samples.
    let copy = host_copy::add(&mut dag, ()).unwrap();
    dag.connect(twice, copy.image).unwrap();
    let result = evaluator.evaluate::<Rgb>(&dag, &Default::default(), copy.output).unwrap();
    assert_eq!(result.statistics.fused, 1);
    assert_eq!(**result.data.host().unwrap(), [[2.0, -8.0, 24.0]; 3]);

    // Two consumers of one deferred image share its materialization.
    let pair = sum::add(&mut dag, ()).unwrap();
    dag.connect(twice, pair.a).unwrap();
    dag.connect(twice, pair.b).unwrap();
    let result = evaluator.evaluate::<Rgb>(&dag, &Default::default(), pair.output).unwrap();
    assert_eq!(result.statistics.fused, 1);
    assert_eq!(**result.data.host().unwrap(), [[4.0, -16.0, 48.0]; 3]);

    // Maps after a fan-out each extend the shared plan, so nothing
    // materializes `first` itself.
    let left = gain(&mut dag, first, -1.0);
    let right = gain(&mut dag, first, 1.0);
    let pair = sum::add(&mut dag, ()).unwrap();
    dag.connect(left, pair.a).unwrap();
    dag.connect(right, pair.b).unwrap();
    let result = evaluator.evaluate::<Rgb>(&dag, &Default::default(), pair.output).unwrap();
    assert_eq!(result.statistics.fused, 2);
    assert_eq!(**result.data.host().unwrap(), [[1.25, -5.0, 15.0]; 3]);

    // A plan may read no constants.
    let negated = negate::add(&mut dag, ()).unwrap();
    dag.connect(source, negated.image).unwrap();
    let result = evaluator.evaluate::<Rgb>(&dag, &Default::default(), negated.output).unwrap();
    assert_eq!(result.statistics.fused, 1);
    assert_eq!(**result.data.host().unwrap(), [[-0.25, 1.0, -3.0]; 3]);
}

/// The template's colour tail: camera matrix and CAT16 materialize for
/// Orientation, then exposure and sigmoid run as one kernel.
fn template_tail_launches_once_after_orientation(runtime: RuntimeContext) {
    let evaluator = Evaluator::new(runtime);
    let mut chain = chain();
    let d = &mut chain.dag;
    let camera = node::camera_to_rgb::add(d, ()).unwrap();
    let cat = chromatic_adaptation::add(d, Default::default()).unwrap();
    let orientation = node::orientation::add(d, Default::default()).unwrap();
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
    let to_sigmoid = evaluator.evaluate::<Rgb>(d, &Default::default(), sigmoid.output).unwrap();
    assert_eq!(to_sigmoid.statistics.fused, 2);
    assert_eq!(to_sigmoid.statistics.uploads, 1);
    for kind in [&exposure::compute::KIND, &sigmoid::apply::KIND] {
        assert!(matches!(kind.shape, Shape::Map(_)), "{}", kind.id);
    }
}

fn all(runtime: RuntimeContext) {
    fused_chain_matches_the_replaced_kernels(runtime.clone());
    deferred_images_materialize_once_per_consumer_that_needs_samples(runtime.clone());
    template_tail_launches_once_after_orientation(runtime);
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

/// The per-node kernels of Drip 3479319, with unchanged arithmetic.
mod legacy {
    use cubecl::prelude::*;

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
