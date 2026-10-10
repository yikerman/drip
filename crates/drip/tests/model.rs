use drip::{
    Error, Result,
    eval::Evaluator,
    graph::Dag,
    node,
    node::data::*,
    ports::*,
    runtime::{KernelContext, RuntimeContext},
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn camera() -> Camera {
    Camera {
        coordinates: "fixture-camera-response-v1".into(),
        scale: "black-relative-white-reference".into(),
    }
}
fn color() -> Color {
    Color {
        space: "rec2020-d65".into(),
        encoding: Encoding::Identity,
        scale: "relative-white-1".into(),
    }
}
fn image_desc() -> ImageDesc<Color> {
    ImageDesc { extent: Extent { width: 2, height: 1 }, interpretation: color() }
}
fn image(dag: &mut Dag) -> Output<ColorRgb> {
    node::source::<ColorRgb>(dag, image_desc(), vec![[0.1, -0.2, 2.0]; 2]).unwrap()
}

type CMatrix = Matrix3<Camera, Color>;
fn identity() -> [[f32; 3]; 3] {
    [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
}

fn copy_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    i: Option<&ImageDesc<Color>>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    Ok((i.cloned(),))
}
/// CPU identity for checking transport; no color reinterpretation.
#[node(id="test-cpu-copy", contract=copy_contract, references=[("contract", "https://example.invalid/contract")], port_labels(image = "Input RGB", output = "Output RGB"))]
fn cpu_copy(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<ColorRgb>>,
    output: Write<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    output.data.copy_from_slice(image.data);
    Ok(())
}

#[test]
fn sources_check_actual_storage_at_admission() {
    let mut d = Dag::new();
    assert!(node::source::<ColorRgb>(&mut d, image_desc(), vec![[0.; 3]]).is_err());
    let mut desc = image_desc();
    desc.extent.width = 0;
    assert!(node::source::<ColorRgb>(&mut d, desc, vec![]).is_err());
}
#[test]
fn mismatches_are_rejected_before_runtime_exists() {
    let mut d = Dag::new();
    let camera_image = node::source::<CameraRgb>(
        &mut d,
        ImageDesc { extent: image_desc().extent, interpretation: camera() },
        vec![[0.5; 3]; 2],
    )
    .unwrap();
    let mut wrong = camera();
    wrong.coordinates = "other camera".into();
    let matrix =
        node::source::<CMatrix>(&mut d, MatrixDesc { source: wrong, target: color() }, identity())
            .unwrap();
    let n = node::camera_to_rgb::add(&mut d, ()).unwrap();
    d.connect(camera_image, n.image).unwrap();
    assert!(
        d.connect(matrix, n.matrix).unwrap_err().to_string().contains("differs from matrix source")
    );
    let valid = node::source::<CMatrix>(
        &mut d,
        MatrixDesc { source: camera(), target: color() },
        identity(),
    )
    .unwrap();
    d.connect(valid, n.matrix).unwrap(); // failed edit left the input free
    let exposure = node::exposure::add(&mut d, node::Exposure { ev: 0. }).unwrap();
    assert!(d.connect_ids(camera_image.id(), exposure.image.id()).is_err());
    d.connect(n.output, exposure.image).unwrap();
}
#[test]
fn rejects_encoded_rgb_without_guessing() {
    let mut d = Dag::new();
    let mut desc = image_desc();
    desc.interpretation.encoding = Encoding::Srgb;
    let source = node::source::<ColorRgb>(&mut d, desc, vec![[0.; 3]; 2]).unwrap();
    let e = node::exposure::add(&mut d, node::Exposure { ev: 0. }).unwrap();
    assert!(d.connect(source, e.image).is_err());
    assert!(node::exposure::add(&mut d, node::Exposure { ev: f32::NAN }).is_err());
}
#[test]
fn typed_macro_supplies_docs_ports_and_discovery() {
    let n = drip::definition::registered("test-cpu-copy", serde_json::Value::Null).unwrap();
    assert!(n.metadata().help.contains("CPU identity"));
    assert_eq!(n.metadata().references.len(), 1);
    assert_eq!(n.inputs()[0].name, "image");
    assert_eq!(n.outputs()[0].name, "output");
    assert_eq!(n.inputs()[0].payload_name, "Input RGB");
    assert_eq!(n.outputs()[0].payload_name, "Output RGB");
    assert!(n.inputs()[0].is::<ColorRgb>());
    assert!(n.outputs()[0].is::<ColorRgb>());
    assert!(n.metadata().parameters.is_empty());
    assert!(
        drip::definition::registered("tone.exposure", serde_json::json!({"ev":"bad"})).is_err()
    );
}
#[test]
fn parameter_schema_comes_from_the_function_parameter_type() {
    let n = relabel::definition(Relabel::default());
    let [parameter] = n.metadata().parameters else { panic!("expected one parameter") };
    assert_eq!(parameter.name, "encoded");
    assert_eq!(parameter.kind, drip::param::ParamKind::Bool { default: false });
}
#[test]
fn incomplete_nodes_do_not_make_unrelated_targets_unevaluable() {
    let mut d = Dag::new();
    let source = image(&mut d);
    let incomplete = cpu_copy::add(&mut d, ()).unwrap();
    let eval = Evaluator::new(RuntimeContext::host());
    assert!(eval.evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), source).is_ok());
    assert!(matches!(
        eval.evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), incomplete.output),
        Err(Error::MissingInput { .. })
    ));
    let downstream = cpu_copy::add(&mut d, ()).unwrap();
    d.connect(incomplete.output, downstream.image).unwrap();
    assert!(eval.evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), downstream.output).is_err());
    d.connect(source, incomplete.image).unwrap();
    assert!(eval.evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), downstream.output).is_ok());
}
#[test]
fn rejects_cycles_occupancy_foreign_handles_and_wrong_directions() {
    let mut d = Dag::new();
    let source = image(&mut d);
    let a = cpu_copy::add(&mut d, ()).unwrap();
    let b = cpu_copy::add(&mut d, ()).unwrap();
    d.connect(source, a.image).unwrap();
    d.connect(a.output, b.image).unwrap();
    assert!(d.connect(source, a.image).is_err());
    assert!(
        d.edit(|e| {
            e.disconnect(a.image.id())?;
            e.connect(b.output, a.image)
        })
        .is_err()
    );
    let mut other = Dag::new();
    let foreign = image(&mut other);
    assert!(d.connect_ids(foreign.id(), a.image.id()).is_err());
    assert!(d.connect_ids(a.image.id(), b.output.id()).is_err());
    assert_eq!(
        Evaluator::new(RuntimeContext::host())
            .evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), b.output)
            .unwrap()
            .statistics
            .nodes,
        3
    );
}
#[test]
fn handles_from_aborted_edits_and_removed_nodes_stay_stale() {
    let mut d = Dag::new();
    let source = image(&mut d);
    let mut escaped = None;
    let failed: Result<()> = d.edit(|e| {
        escaped = Some(cpu_copy::Ports::from_node(e.add(cpu_copy::definition(()))));
        Err(Error::Graph("abort".into()))
    });
    assert!(failed.is_err());
    let a = cpu_copy::add(&mut d, ()).unwrap();
    assert!(d.connect(source, escaped.unwrap().image).is_err());
    d.remove(a.node).unwrap();
    let b = cpu_copy::add(&mut d, ()).unwrap();
    assert!(d.connect(source, a.image).is_err());
    d.connect(source, b.image).unwrap();
}
#[derive(Default, serde::Serialize, serde::Deserialize, drip::Parameters)]
struct Relabel {
    #[param(drip::param::ParamKind::Bool { default: false })]
    encoded: bool,
}
fn relabel_contract(
    _: &drip::runtime::GlobalContext,
    p: &Relabel,
    i: Option<&ImageDesc<Color>>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    let mut output = i.cloned();
    if let Some(d) = &mut output {
        d.interpretation.encoding = if p.encoded { Encoding::Srgb } else { Encoding::Identity };
    }
    Ok((output,))
}
/// Explicit test-only assignment of encoding, without numeric conversion.
#[node(id="test-relabel",contract=relabel_contract)]
fn relabel(
    _: &KernelContext<'_>,
    _: &Relabel,
    image: Read<'_, Cpu<ColorRgb>>,
    output: Write<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    output.data.copy_from_slice(image.data);
    Ok(())
}
#[test]
fn parameter_edits_recheck_downstream_and_batch_edits_commit_together() {
    let mut d = Dag::new();
    let source = image(&mut d);
    let a = relabel::add(&mut d, Relabel { encoded: false }).unwrap();
    let b = node::exposure::add(&mut d, node::Exposure { ev: 0. }).unwrap();
    d.connect(source, a.image).unwrap();
    d.connect(a.output, b.image).unwrap();
    assert!(d.replace(a.node, relabel::definition(Relabel { encoded: true })).is_err());
    assert_eq!(
        d.description(a.output).unwrap().unwrap().interpretation.encoding,
        Encoding::Identity
    );
    d.edit(|e| {
        e.replace(a.node, relabel::definition(Relabel { encoded: true }))?;
        e.disconnect(b.image.id())
    })
    .unwrap();
    assert_eq!(d.description(a.output).unwrap().unwrap().interpretation.encoding, Encoding::Srgb);
}

static PRODUCED: AtomicUsize = AtomicUsize::new(0);
type SplitDescriptions = (Option<ImageDesc<Color>>, Option<MatrixDesc<Camera, Color>>);
fn split_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    i: Option<&ImageDesc<Color>>,
) -> Result<SplitDescriptions> {
    Ok((i.cloned(), i.map(|d| MatrixDesc { source: camera(), target: d.interpretation.clone() })))
}
/// Produce independent image and matrix ports to exercise multi-output adapters.
#[node(id="test-split",contract=split_contract)]
fn split(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<ColorRgb>>,
    copy: Write<'_, Cpu<ColorRgb>>,
    matrix: Write<'_, Cpu<CMatrix>>,
) -> Result<()> {
    PRODUCED.fetch_add(1, Ordering::Relaxed);
    copy.data.copy_from_slice(image.data);
    *matrix.data = identity();
    Ok(())
}
fn consume_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    a: Option<&ImageDesc<Color>>,
    b: Option<&MatrixDesc<Camera, Color>>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    if let (Some(a), Some(b)) = (a, b)
        && a.interpretation != b.target
    {
        return Err(Error::Contract("target mismatch".into()));
    }
    Ok((if b.is_some() { a.cloned() } else { None },))
}
#[node(id="test-consume",contract=consume_contract)]
fn consume(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<ColorRgb>>,
    _matrix: Read<'_, Cpu<CMatrix>>,
    output: Write<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    output.data.copy_from_slice(image.data);
    Ok(())
}
#[test]
fn independent_outputs_share_one_producer_per_evaluation_without_persistent_cache() {
    PRODUCED.store(0, Ordering::Relaxed);
    let mut d = Dag::new();
    let source = image(&mut d);
    let split = split::add(&mut d, ()).unwrap();
    let consumer = consume::add(&mut d, ()).unwrap();
    d.connect(source, split.image).unwrap();
    d.connect(split.copy, consumer.image).unwrap();
    d.connect(split.matrix, consumer._matrix).unwrap();
    let eval = Evaluator::new(RuntimeContext::host());
    for expected in 1..=2 {
        let result =
            eval.evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), consumer.output).unwrap();
        assert_eq!(result.statistics.nodes, 3);
        assert_eq!(PRODUCED.load(Ordering::Relaxed), expected);
    }
    let matrix = eval.evaluate::<Cpu<CMatrix>>(&d, &Default::default(), split.matrix).unwrap();
    assert_eq!(*matrix.data, identity()); // image output may be unused
    let native = eval.evaluate_node(&d, &Default::default(), split.node).unwrap();
    assert_eq!(native.outputs.len(), 2);
    assert_eq!(*native.outputs[1].get::<Cpu<CMatrix>>().unwrap().1, identity());
}

fn checked_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    i: Option<&ImageDesc<Color>>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    Ok((i.cloned(),))
}
#[node(id="test-value-check",contract=checked_contract)]
fn checked(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<ColorRgb>>,
    output: Write<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    if image.data.iter().any(|p| p[0] == 0.0) {
        return Err(Error::Contract("data-dependent zero divisor".into()));
    }
    output.data.copy_from_slice(image.data);
    Ok(())
}
#[test]
fn data_dependent_checks_remain_at_eval() {
    let mut d = Dag::new();
    let source = node::source::<ColorRgb>(&mut d, image_desc(), vec![[0.; 3]; 2]).unwrap();
    let checked = checked::add(&mut d, ()).unwrap();
    d.connect(source, checked.image).unwrap();
    let error = Evaluator::new(RuntimeContext::host())
        .evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), checked.output)
        .err()
        .unwrap();
    assert!(error.to_string().contains("zero divisor"));
}

fn device_chain(runtime: RuntimeContext) {
    let metadata = drip_raw::Metadata {
        iso: f32::from_bits(0x7fc00012),
        shutter: -0.0,
        aperture: 2.8,
        focal_length: 50.0,
        timestamp: -1234567,
        make: "測試".into(),
        model: "x".into(),
        datetime: Vec::new(),
    };
    let desc = MetadataDesc::of(&metadata);
    let device = CaptureMetadata::upload(&desc, &metadata, &runtime).unwrap();
    let restored = CaptureMetadata::download(&desc, &device, &runtime).unwrap();
    assert_eq!(restored.iso.to_bits(), metadata.iso.to_bits());
    assert_eq!(restored.shutter.to_bits(), metadata.shutter.to_bits());
    assert_eq!(
        (restored.make, restored.model, restored.datetime, restored.timestamp),
        (metadata.make, metadata.model, metadata.datetime, metadata.timestamp)
    );
    let mut d = Dag::new();
    let source = node::source::<Bayer>(
        &mut d,
        BayerDesc {
            extent: Extent { width: 5, height: 3 },
            phase: BayerPhase::Rggb,
            interpretation: camera(),
        },
        vec![1.; 15],
    )
    .unwrap();
    let matrix = node::source::<CMatrix>(
        &mut d,
        MatrixDesc { source: camera(), target: color() },
        identity(),
    )
    .unwrap();
    let preview = node::bayer_preview::add(&mut d, ()).unwrap();
    let convert = node::camera_to_rgb::add(&mut d, ()).unwrap();
    let exposure = node::exposure::add(&mut d, node::Exposure { ev: 1. }).unwrap();
    let island = cpu_copy::add(&mut d, ()).unwrap();
    let after = node::exposure::add(&mut d, node::Exposure { ev: -2. }).unwrap();
    d.connect(source, preview.image).unwrap();
    d.connect(preview.output, convert.image).unwrap();
    d.connect(matrix, convert.matrix).unwrap();
    d.connect(convert.output, exposure.image).unwrap();
    d.connect(exposure.output, island.image).unwrap();
    d.connect(island.output, after.image).unwrap();
    let eval = Evaluator::new(runtime);
    let result = eval.evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), after.output).unwrap();
    assert_eq!(result.desc.extent, Extent { width: 2, height: 1 });
    for p in result.data.iter() {
        for v in p {
            assert!((*v - 0.5).abs() < 2e-6);
        }
    }
    assert_eq!(result.statistics.nodes, 7);
    assert_eq!(result.statistics.uploads, 3);
    assert_eq!(result.statistics.downloads, 2);
    let resident =
        eval.evaluate::<Device<ColorRgb>>(&d, &Default::default(), exposure.output).unwrap();
    assert_eq!(resident.statistics.downloads, 0);
    assert_eq!(resident.statistics.uploads, 2);

    let observer = sink::add(&mut d, ()).unwrap();
    d.connect(after.output, observer.image).unwrap();
    let mut timings = Vec::new();
    let values = eval.evaluate_batch_with_timings(
        &d,
        &Default::default(),
        &[observer.node],
        &[observer.node],
        Some(&mut timings),
    );
    let image = values[&observer.node].as_ref().unwrap().get("image").unwrap();
    assert_eq!(image.get::<Cpu<ColorRgb>>().unwrap().1, result.data.as_ref());
    assert_eq!(timings.len(), 8);
    assert!(timings.iter().all(|timing| timing.success));
}
#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a compute adapter; run explicitly"]
fn cubecl_wgpu_hybrid() {
    device_chain(RuntimeContext::wgpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_hybrid() {
    device_chain(RuntimeContext::from_env().unwrap());
}
#[cfg(feature = "cpu")]
#[test]
fn cubecl_cpu_same_kernels() {
    device_chain(RuntimeContext::cpu());
}

fn sink_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    _: Option<&ImageDesc<Color>>,
) -> Result<()> {
    Ok(())
}
#[node(id = "test-input-only", contract = sink_contract)]
fn sink(_: &KernelContext<'_>, _: &(), image: Read<'_, Cpu<ColorRgb>>) -> Result<()> {
    if image.data.is_empty() {
        return Err(Error::Contract("empty sink input".into()));
    }
    Ok(())
}

#[test]
fn node_targets_include_input_only_nodes() {
    let mut d = Dag::new();
    let source = image(&mut d);
    let outputs = cpu_copy::add(&mut d, ()).unwrap();
    d.connect(source, outputs.image).unwrap();
    let eval = Evaluator::new(RuntimeContext::host());
    let result = eval.evaluate_node(&d, &Default::default(), outputs.node).unwrap();
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0].get::<Cpu<ColorRgb>>().unwrap().1.len(), 2);
    assert!(result.outputs[0].get::<Cpu<CMatrix>>().is_err());
    let sink = sink::add(&mut d, ()).unwrap();
    assert!(eval.evaluate_node(&d, &Default::default(), sink.node).is_err());
    d.connect(source, sink.image).unwrap();
    let result = eval.evaluate_node(&d, &Default::default(), sink.node).unwrap();
    assert!(result.outputs.is_empty());
    assert_eq!(result.statistics.nodes, 2);
}

#[node(id = "test-bad-producer", contract = copy_contract)]
fn bad_producer(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<ColorRgb>>,
    output: Write<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    *output.data = image.data[..1].to_vec().into();
    Ok(())
}
#[test]
fn invalid_storage_cannot_escape_a_node_adapter() {
    let mut d = Dag::new();
    let source = image(&mut d);
    let bad = bad_producer::add(&mut d, ()).unwrap();
    d.connect(source, bad.image).unwrap();
    let error = Evaluator::new(RuntimeContext::host())
        .evaluate::<Cpu<ColorRgb>>(&d, &Default::default(), bad.output)
        .err()
        .unwrap();
    assert!(error.to_string().contains("extent/storage mismatch"));
    let mut desc = image_desc();
    desc.interpretation.space.clear();
    assert!(node::source::<ColorRgb>(&mut d, desc, vec![[0.0; 3]; 2]).is_err());
}

static BATCH_PRODUCED: AtomicUsize = AtomicUsize::new(0);
#[node(id="test-batch-producer", contract=copy_contract)]
fn batch_producer(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<ColorRgb>>,
    output: Write<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    BATCH_PRODUCED.fetch_add(1, Ordering::Relaxed);
    output.data.copy_from_slice(image.data);
    Ok(())
}

#[test]
fn batch_shares_ancestors_retains_only_observers_and_isolates_failures() {
    let mut dag = Dag::new();
    let source = image(&mut dag);
    let producer = batch_producer::add(&mut dag, ()).unwrap();
    dag.connect(source, producer.image).unwrap();
    let a = sink::add(&mut dag, ()).unwrap();
    let b = sink::add(&mut dag, ()).unwrap();
    dag.connect(producer.output, a.image).unwrap();
    dag.connect(producer.output, b.image).unwrap();
    let zero = node::source::<ColorRgb>(&mut dag, image_desc(), vec![[0.; 3]; 2]).unwrap();
    let failure = checked::add(&mut dag, ()).unwrap();
    dag.connect(zero, failure.image).unwrap();
    let downstream = sink::add(&mut dag, ()).unwrap();
    dag.connect(failure.output, downstream.image).unwrap();
    let incomplete = sink::add(&mut dag, ()).unwrap();
    let targets = [producer.node, a.node, b.node, failure.node, downstream.node, incomplete.node];
    let mut timings = Vec::new();
    let values = Evaluator::new(RuntimeContext::host()).evaluate_batch_with_timings(
        &dag,
        &Default::default(),
        &targets,
        &[a.node, b.node],
        Some(&mut timings),
    );
    assert_eq!(timings.len(), 6); // Shared nodes once; upstream failures and missing inputs skipped.
    assert_eq!(timings.iter().filter(|t| t.node == producer.node).count(), 1);
    let position = |id| timings.iter().position(|t| t.node == id).unwrap();
    assert!(position(source.node()) < position(producer.node));
    assert!(position(producer.node) < position(a.node));
    assert!(position(producer.node) < position(b.node));
    let failed: Vec<_> = timings.iter().filter(|t| !t.success).map(|t| (t.node, t.kind)).collect();
    assert_eq!(failed, [(failure.node, "test-value-check")]);
    assert!(!timings.iter().any(|t| t.node == downstream.node || t.node == incomplete.node));
    assert_eq!(BATCH_PRODUCED.load(Ordering::Relaxed), 1);
    assert!(values[&producer.node].as_ref().unwrap().get("image").is_none());
    let a =
        values[&a.node].as_ref().unwrap().get("image").unwrap().shared::<Cpu<ColorRgb>>().unwrap();
    let b =
        values[&b.node].as_ref().unwrap().get("image").unwrap().shared::<Cpu<ColorRgb>>().unwrap();
    assert!(std::sync::Arc::ptr_eq(&a, &b));
    for id in [failure.node, downstream.node] {
        assert!(matches!(&values[&id], Err(Error::Node { node, .. }) if *node == failure.node));
    }
    assert!(matches!(&values[&incomplete.node], Err(Error::MissingInput { .. })));
}

#[node(id="test-optional-input", contract=sink_contract)]
fn optional_sink(
    _: &KernelContext<'_>,
    _: &(),
    image: Option<Read<'_, Cpu<ColorRgb>>>,
) -> Result<()> {
    if let Some(image) = image {
        assert_eq!(image.data.len(), 2);
    }
    Ok(())
}
#[test]
fn optional_input_is_absent_only_when_disconnected() {
    let mut dag = Dag::new();
    let observer = optional_sink::add(&mut dag, ()).unwrap();
    let eval = Evaluator::new(RuntimeContext::host());
    let values = eval.evaluate_inputs(&dag, &Default::default(), &[observer.node]);
    assert!(values[&observer.node].as_ref().unwrap().get("image").is_none());
    let missing = cpu_copy::add(&mut dag, ()).unwrap();
    dag.connect(missing.output, observer.image).unwrap();
    assert!(
        eval.evaluate_inputs(&dag, &Default::default(), &[observer.node])[&observer.node].is_err()
    );
    let source = image(&mut dag);
    dag.connect(source, missing.image).unwrap();
    let values = eval.evaluate_inputs(&dag, &Default::default(), &[observer.node]);
    assert!(values[&observer.node].as_ref().unwrap().get("image").is_some());
}

fn context_contract(
    global: &drip::runtime::GlobalContext,
    _: &(),
) -> Result<(Option<ImageDesc<Color>>,)> {
    Ok((Some(ImageDesc {
        extent: Extent { width: global.scale, height: 1 },
        interpretation: color(),
    }),))
}
#[node(id="test-global-context",contract=context_contract)]
fn context_source(
    context: &KernelContext<'_>,
    _: &(),
    output: Write<'_, Cpu<ColorRgb>>,
) -> Result<()> {
    assert_eq!(output.desc.extent.width, context.global.scale);
    output.data.fill([context.global.scale as f32; 3]);
    Ok(())
}
fn fixed_width_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    image: Option<&ImageDesc<Color>>,
) -> Result<()> {
    if image.is_some_and(|d| d.extent.width != 1) {
        return Err(Error::Contract("requires width 1".into()));
    }
    Ok(())
}
#[node(id="test-fixed-width",contract=fixed_width_contract)]
fn fixed_width(_: &KernelContext<'_>, _: &(), _image: Read<'_, Cpu<ColorRgb>>) -> Result<()> {
    Ok(())
}

#[test]
fn global_context_describes_and_computes_each_request_without_changing_the_dag() {
    use drip::runtime::GlobalContext;
    let mut dag = Dag::new();
    let source = context_source::add(&mut dag, ()).unwrap();
    let copy = cpu_copy::add(&mut dag, ()).unwrap();
    dag.connect(source.output, copy.image).unwrap();
    let fixed = fixed_width::add(&mut dag, ()).unwrap();
    dag.connect(source.output, fixed._image).unwrap(); // Full-detail connection is valid.
    let evaluator = Evaluator::new(RuntimeContext::host());
    let global = GlobalContext { scale: 3 };
    let value = evaluator.evaluate::<Cpu<ColorRgb>>(&dag, &global, copy.output).unwrap();
    assert_eq!(value.desc.extent.width, 3);
    assert_eq!(&**value.data, &[[3.; 3]; 3]);
    assert_eq!(dag.description(copy.output).unwrap().unwrap().extent.width, 1);
    let batch = evaluator.evaluate_inputs(&dag, &global, &[copy.node, fixed.node]);
    let value = batch[&copy.node].as_ref().unwrap().get("image").unwrap();
    assert_eq!(value.get::<Cpu<ColorRgb>>().unwrap().0.extent.width, 3);
    assert!(batch[&fixed.node].as_ref().err().unwrap().to_string().contains("requires width 1"));
    assert!(evaluator.evaluate_node(&dag, &Default::default(), fixed.node).is_ok());
    assert!(
        evaluator
            .evaluate::<Cpu<ColorRgb>>(&dag, &GlobalContext { scale: 0 }, copy.output)
            .is_err()
    );
}
