use drip::{
    Error, Result,
    eval::{Evaluator, InputValues},
    graph::Dag,
    node,
    node::{color, data::*},
    ports::*,
    runtime::{KernelContext, RuntimeContext},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A camera whose responses are linear Rec.2020/D65, so its matrix is identity.
fn metadata(model: &str) -> Arc<drip_raw::Metadata> {
    let mut m =
        drip_raw::Metadata { make: "Fixture".into(), model: model.into(), ..Default::default() };
    let cam_from_xyz =
        color::to_f32(&color::inverse(&color::rgb_to_xyz(color::REC2020, color::D65)));
    m.calibration.xyz_to_cam[..3].copy_from_slice(&cam_from_xyz);
    Arc::new(m)
}
fn camera(m: &drip_raw::Metadata) -> Response {
    Response { camera: Camera::of(m), balance: None }
}
fn image_desc() -> RgbDesc {
    RgbDesc::new(Extent { width: 2, height: 1 }, Coordinates::Space(REC2020_D65))
}
fn image(dag: &mut Dag) -> Output<Rgb> {
    node::source::<Rgb>(dag, image_desc(), vec![[0.1, -0.2, 2.0]; 2]).unwrap()
}
fn capture(dag: &mut Dag, m: Arc<drip_raw::Metadata>) -> Output<CaptureMetadata> {
    node::source::<CaptureMetadata>(dag, m.clone(), m).unwrap()
}
/// A node's contract on input descriptions, as evaluation checks it.
fn contract(d: &Dag, id: NodeId, inputs: &[Option<Description>]) -> Result<Vec<Description>> {
    d.node(id)?.contract(&Default::default(), inputs)
}
fn described<T: Send + Sync + 'static>(desc: T) -> Option<Description> {
    Some(Arc::new(desc))
}

fn copy_contract(_: &drip::runtime::GlobalContext, _: &(), i: &RgbDesc) -> Result<(RgbDesc,)> {
    Ok((i.clone(),))
}
/// CPU identity for checking transport; no color reinterpretation.
#[node(id="test-cpu-copy", contract=copy_contract, references=[("contract", "https://example.invalid/contract")])]
fn cpu_copy(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<Rgb>>,
    output: Write<'_, Cpu<Rgb>>,
) -> Result<()> {
    output.data.copy_from_slice(image.data);
    Ok(())
}

#[test]
fn sources_check_actual_storage_at_admission() {
    let mut d = Dag::new();
    assert!(node::source::<Rgb>(&mut d, image_desc(), vec![[0.; 3]]).is_err());
    let mut desc = image_desc();
    desc.extent.width = 0;
    assert!(node::source::<Rgb>(&mut d, desc, vec![]).is_err());
}
#[test]
fn connections_check_payloads_and_contracts_check_interpretations() {
    let mut d = Dag::new();
    let a = metadata("A");
    let desc = RgbDesc::new(image_desc().extent, Coordinates::Camera(camera(&a)));
    let camera_image = node::source::<Rgb>(&mut d, desc.clone(), vec![[0.5; 3]; 2]).unwrap();
    let other = capture(&mut d, metadata("B"));
    let n = node::camera_to_rgb::add(&mut d, ()).unwrap();
    let sigmoid = node::sigmoid::apply::add(&mut d, Default::default()).unwrap();
    // Whether a source is loaded does not change which connections are accepted.
    d.connect(camera_image, n.image).unwrap();
    d.connect(other, n.metadata).unwrap();
    d.connect(camera_image, sigmoid.image).unwrap();
    let error = contract(&d, n.node, &[described(desc.clone()), described(metadata("B"))]);
    assert!(error.err().unwrap().to_string().contains("differs from the capture metadata"));
    assert!(contract(&d, n.node, &[described(desc.clone()), described(a)]).is_ok());
    let error = contract(&d, sigmoid.node, &[described(desc)]).err().unwrap().to_string();
    assert!(error.contains("image requires Rec.2020 / D65"), "{error}");
}
#[test]
fn requirements_compare_spaces_exactly_and_parameters_at_admission() {
    let mut d = Dag::new();
    let sigmoid = node::sigmoid::apply::add(&mut d, Default::default()).unwrap();
    let exposure = node::exposure::add(&mut d, node::exposure::Exposure { ev: 0. }).unwrap();
    let mut desc = image_desc();
    assert!(contract(&d, sigmoid.node, &[described(desc.clone())]).is_ok());
    desc.coordinates = Coordinates::Space(RgbSpace { white: [0.3127, 0.33], ..REC2020_D65 });
    assert!(contract(&d, sigmoid.node, &[described(desc.clone())]).is_err());
    assert!(contract(&d, exposure.node, &[described(desc)]).is_ok()); // Exposure scales any RGB.
    assert!(node::exposure::add(&mut d, node::exposure::Exposure { ev: f32::NAN }).is_err());
}
#[test]
fn typed_macro_supplies_docs_ports_and_discovery() {
    let registry = drip::registry::Registry::new(&[drip::registry::NODES, &[&cpu_copy::KIND]]);
    let n = registry.build("test-cpu-copy", serde_json::Value::Null).unwrap();
    assert!(n.kind().help.contains("CPU identity"));
    assert_eq!(n.kind().references.len(), 1);
    assert_eq!(n.kind().inputs[0].name, "image");
    assert_eq!(n.kind().outputs[0].name, "output");
    assert_eq!(n.kind().inputs[0].label(), "RGB");
    assert!(n.kind().inputs[0].is::<Rgb>());
    assert!(n.kind().outputs[0].is::<Rgb>());
    let sigmoid = node::sigmoid::apply::definition(Default::default()).unwrap();
    assert_eq!(sigmoid.kind().inputs[0].label(), "RGB · Rec.2020 / D65");
    assert!(n.kind().parameters.is_empty());
    assert!(registry.build("exposure", serde_json::json!({"ev":"bad"})).is_err());
}
#[test]
fn parameter_schema_comes_from_the_function_parameter_type() {
    let n = relabel::definition(Relabel::default()).unwrap();
    let [parameter] = n.kind().parameters else { panic!("expected one parameter") };
    assert_eq!(parameter.name, "camera");
    assert_eq!(parameter.kind, drip::param::ParamKind::Bool { default: false });
}
#[test]
fn incomplete_nodes_do_not_make_unrelated_targets_unevaluable() {
    let mut d = Dag::new();
    let source = image(&mut d);
    let incomplete = cpu_copy::add(&mut d, ()).unwrap();
    let eval = Evaluator::new(RuntimeContext::host());
    assert!(eval.evaluate::<Cpu<Rgb>>(&d, &Default::default(), source).is_ok());
    assert!(matches!(
        eval.evaluate::<Cpu<Rgb>>(&d, &Default::default(), incomplete.output),
        Err(Error::MissingInput { .. })
    ));
    let downstream = cpu_copy::add(&mut d, ()).unwrap();
    d.connect(incomplete.output, downstream.image).unwrap();
    assert!(eval.evaluate::<Cpu<Rgb>>(&d, &Default::default(), downstream.output).is_err());
    d.connect(source, incomplete.image).unwrap();
    assert!(eval.evaluate::<Cpu<Rgb>>(&d, &Default::default(), downstream.output).is_ok());
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
            .evaluate::<Cpu<Rgb>>(&d, &Default::default(), b.output)
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
        escaped = Some(cpu_copy::Ports::from_node(e.add(cpu_copy::definition(())?)));
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
#[derive(serde::Serialize, serde::Deserialize, drip::Parameters)]
struct Relabel {
    #[param(default = false)]
    camera: bool,
}
fn relabel_contract(
    _: &drip::runtime::GlobalContext,
    p: &Relabel,
    i: &RgbDesc,
) -> Result<(RgbDesc,)> {
    let coordinates = if p.camera {
        Coordinates::Camera(camera(&metadata("relabel")))
    } else {
        Coordinates::Space(REC2020_D65)
    };
    Ok((RgbDesc { coordinates, ..i.clone() },))
}
/// Explicit test-only assignment of coordinates, without numeric conversion.
#[node(id="test-relabel",contract=relabel_contract)]
fn relabel(
    _: &KernelContext<'_>,
    _: &Relabel,
    image: Read<'_, Cpu<Rgb>>,
    output: Write<'_, Cpu<Rgb>>,
) -> Result<()> {
    output.data.copy_from_slice(image.data);
    Ok(())
}
/// Relabels on the device without copying: the output shares the input's
/// allocation. `half` describes the output with half the extent, which the
/// adapter rejects because the storage no longer matches.
#[node(id="test-forward",contract=forward_contract)]
fn forward(
    _: &KernelContext<'_>,
    _: &Relabel,
    image: Read<'_, Device<Rgb>>,
    output: Forward<'_, Device<Rgb>>,
) -> Result<()> {
    output.set(image.data.share_as());
    Ok(())
}
fn forward_contract(
    global: &drip::runtime::GlobalContext,
    p: &Relabel,
    i: &RgbDesc,
) -> Result<(RgbDesc,)> {
    let (mut output,) = relabel_contract(global, &Relabel { camera: false }, i)?;
    if p.camera {
        output.extent.width /= 2;
        output.full.width /= 2;
    }
    Ok((output,))
}
#[cfg(feature = "cpu")]
#[test]
fn cpu_forwarded_outputs_share_storage_under_a_checked_description() {
    forwarded_outputs(RuntimeContext::cpu());
}
#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_forwarded_outputs_share_storage_under_a_checked_description() {
    forwarded_outputs(RuntimeContext::from_env().unwrap());
}
fn forwarded_outputs(runtime: RuntimeContext) {
    let mut d = Dag::new();
    let a = metadata("forward");
    let desc = RgbDesc::new(image_desc().extent, Coordinates::Camera(camera(&a)));
    let source = node::source::<Rgb>(&mut d, desc, vec![[0.1, -0.2, 2.0]; 2]).unwrap();
    let relabeled = forward::add(&mut d, Relabel { camera: false }).unwrap();
    let mismatched = forward::add(&mut d, Relabel { camera: true }).unwrap();
    d.connect(source, relabeled.image).unwrap();
    d.connect(source, mismatched.image).unwrap();
    let eval = Evaluator::new(runtime);
    let output = eval.evaluate::<Cpu<Rgb>>(&d, &Default::default(), relabeled.output).unwrap();
    assert_eq!(output.desc.coordinates, Coordinates::Space(REC2020_D65));
    assert_eq!(&output.data[..], &[[0.1, -0.2, 2.0]; 2]);
    let Err(error) = eval.evaluate::<Cpu<Rgb>>(&d, &Default::default(), mismatched.output) else {
        panic!("a forward must match its description")
    };
    assert!(error.to_string().contains("RGB extent/storage mismatch"), "{error}");
}

#[test]
fn parameter_edits_ignore_downstream_and_batch_edits_commit_together() {
    let mut d = Dag::new();
    let source = image(&mut d);
    let a = relabel::add(&mut d, Relabel { camera: false }).unwrap();
    let b = node::sigmoid::apply::add(&mut d, Default::default()).unwrap();
    d.connect(source, a.image).unwrap();
    d.connect(a.output, b.image).unwrap();
    let eval = Evaluator::new(RuntimeContext::host());
    let coordinates = |d: &Dag| {
        eval.evaluate::<Cpu<Rgb>>(d, &Default::default(), a.output).unwrap().desc.coordinates
    };
    let relabeled = || relabel::definition(Relabel { camera: true });
    let occupied = d.edit(|e| {
        e.replace(a.node, relabeled()?)?;
        e.connect(source, a.image)
    });
    assert!(occupied.is_err());
    assert_eq!(coordinates(&d), Coordinates::Space(REC2020_D65));
    // Sigmoid now receives camera RGB; it reports that when evaluated.
    d.replace(a.node, relabeled().unwrap()).unwrap();
    assert!(matches!(coordinates(&d), Coordinates::Camera(_)));
}

static PRODUCED: AtomicUsize = AtomicUsize::new(0);
const LEVELS: [f32; 4] = [1., 2., 3., 4.];
fn split_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    i: &RgbDesc,
) -> Result<(RgbDesc, BayerLevelsDesc)> {
    let levels = BayerLevelsDesc { phase: BayerPhase::Rggb, response: camera(&metadata("split")) };
    Ok((i.clone(), levels))
}
/// Produce independent image and levels ports to exercise multi-output adapters.
#[node(id="test-split",contract=split_contract)]
fn split(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<Rgb>>,
    copy: Write<'_, Cpu<Rgb>>,
    levels: Write<'_, Cpu<BayerLevels>>,
) -> Result<()> {
    PRODUCED.fetch_add(1, Ordering::Relaxed);
    copy.data.copy_from_slice(image.data);
    *levels.data = LEVELS;
    Ok(())
}
fn consume_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    image: &RgbDesc,
    _: &BayerLevelsDesc,
) -> Result<(RgbDesc,)> {
    Ok((image.clone(),))
}
#[node(id="test-consume",contract=consume_contract)]
fn consume(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<Rgb>>,
    _levels: Read<'_, Cpu<BayerLevels>>,
    output: Write<'_, Cpu<Rgb>>,
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
    d.connect(split.levels, consumer.levels).unwrap();
    let eval = Evaluator::new(RuntimeContext::host());
    for expected in 1..=2 {
        let result = eval.evaluate::<Cpu<Rgb>>(&d, &Default::default(), consumer.output).unwrap();
        assert_eq!(result.statistics.nodes, 3);
        assert_eq!(PRODUCED.load(Ordering::Relaxed), expected);
    }
    let levels = eval.evaluate::<Cpu<BayerLevels>>(&d, &Default::default(), split.levels).unwrap();
    assert_eq!(*levels.data, LEVELS); // image output may be unused
}

#[node(id="test-value-check",contract=copy_contract)]
fn checked(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<Rgb>>,
    output: Write<'_, Cpu<Rgb>>,
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
    let source = node::source::<Rgb>(&mut d, image_desc(), vec![[0.; 3]; 2]).unwrap();
    let checked = checked::add(&mut d, ()).unwrap();
    d.connect(source, checked.image).unwrap();
    let error = Evaluator::new(RuntimeContext::host())
        .evaluate::<Cpu<Rgb>>(&d, &Default::default(), checked.output)
        .err()
        .unwrap();
    assert!(error.to_string().contains("zero divisor"));
}

fn device_chain(runtime: RuntimeContext) {
    let m = metadata("chain");
    let mut d = Dag::new();
    let source = node::source::<Bayer>(
        &mut d,
        BayerDesc {
            extent: Extent { width: 5, height: 3 },
            phase: BayerPhase::Rggb,
            response: camera(&m),
        },
        vec![1.; 15],
    )
    .unwrap();
    let matrix = capture(&mut d, m);
    let preview = node::bayer_2x2::add(&mut d, ()).unwrap();
    let convert = node::camera_to_rgb::add(&mut d, ()).unwrap();
    let exposure = node::exposure::add(&mut d, node::exposure::Exposure { ev: 1. }).unwrap();
    let island = cpu_copy::add(&mut d, ()).unwrap();
    let after = node::exposure::add(&mut d, node::exposure::Exposure { ev: -2. }).unwrap();
    d.connect(source, preview.image).unwrap();
    d.connect(preview.output, convert.image).unwrap();
    d.connect(matrix, convert.metadata).unwrap();
    d.connect(convert.output, exposure.image).unwrap();
    d.connect(exposure.output, island.image).unwrap();
    d.connect(island.output, after.image).unwrap();
    let eval = Evaluator::new(runtime);
    let result = eval.evaluate::<Cpu<Rgb>>(&d, &Default::default(), after.output).unwrap();
    assert_eq!(result.desc.extent, Extent { width: 2, height: 1 });
    for p in result.data.iter() {
        for v in p {
            assert!((*v - 0.5).abs() < 2e-6);
        }
    }
    assert_eq!(result.statistics.nodes, 7);
    assert_eq!(result.statistics.uploads, 2);
    assert_eq!(result.statistics.downloads, 2);
    let resident = eval.evaluate::<Device<Rgb>>(&d, &Default::default(), exposure.output).unwrap();
    assert_eq!(resident.statistics.downloads, 0);
    assert_eq!(resident.statistics.uploads, 1);

    let observer = sink::add(&mut d, ()).unwrap();
    d.connect(after.output, observer.image).unwrap();
    let mut timings = Vec::new();
    let values = eval.evaluate_batch(&d, &Default::default(), &[observer.node], Some(&mut timings));
    let inputs = values[&observer.node].as_ref().unwrap().bind::<sink::Definition>().unwrap();
    assert_eq!(inputs.image.data, result.data.as_ref());
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

#[node(id = "test-input-only")]
fn sink(_: &KernelContext<'_>, _: &(), image: Read<'_, Cpu<Rgb>>) -> Result<()> {
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
    let sink = sink::add(&mut d, ()).unwrap();
    let demand = |d: &Dag| eval.evaluate_inputs(d, &Default::default(), &[sink.node]);
    let missing = |error: &Error| {
        matches!(error, Error::Node { node, source, .. }
            if *node == sink.node && matches!(**source, Error::MissingInput { .. }))
    };
    assert!(demand(&d)[&sink.node].as_ref().is_err_and(missing));
    d.connect(outputs.output, sink.image).unwrap();
    let inputs = demand(&d).remove(&sink.node).unwrap().unwrap();
    assert_eq!(inputs.bind::<sink::Definition>().unwrap().image.data.len(), 2);
}

#[node(id = "test-bad-producer", contract = copy_contract)]
fn bad_producer(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<Rgb>>,
    output: Write<'_, Cpu<Rgb>>,
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
        .evaluate::<Cpu<Rgb>>(&d, &Default::default(), bad.output)
        .err()
        .unwrap();
    assert!(error.to_string().contains("extent/storage mismatch"));
}

static BATCH_PRODUCED: AtomicUsize = AtomicUsize::new(0);
#[node(id="test-batch-producer", contract=copy_contract)]
fn batch_producer(
    _: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Cpu<Rgb>>,
    output: Write<'_, Cpu<Rgb>>,
) -> Result<()> {
    BATCH_PRODUCED.fetch_add(1, Ordering::Relaxed);
    output.data.copy_from_slice(image.data);
    Ok(())
}

#[test]
fn batch_shares_ancestors_and_isolates_failures() {
    let mut dag = Dag::new();
    let source = image(&mut dag);
    let producer = batch_producer::add(&mut dag, ()).unwrap();
    dag.connect(source, producer.image).unwrap();
    let a = sink::add(&mut dag, ()).unwrap();
    let b = sink::add(&mut dag, ()).unwrap();
    dag.connect(producer.output, a.image).unwrap();
    dag.connect(producer.output, b.image).unwrap();
    let zero = node::source::<Rgb>(&mut dag, image_desc(), vec![[0.; 3]; 2]).unwrap();
    let failure = checked::add(&mut dag, ()).unwrap();
    dag.connect(zero, failure.image).unwrap();
    let downstream = sink::add(&mut dag, ()).unwrap();
    dag.connect(failure.output, downstream.image).unwrap();
    let incomplete = sink::add(&mut dag, ()).unwrap();
    let targets = [producer.node, a.node, b.node, failure.node, downstream.node, incomplete.node];
    let mut timings = Vec::new();
    let values = Evaluator::new(RuntimeContext::host()).evaluate_batch(
        &dag,
        &Default::default(),
        &targets,
        Some(&mut timings),
    );
    assert_eq!(timings.len(), 7); // Shared nodes once; nodes after a failure skipped.
    assert_eq!(timings.iter().filter(|t| t.node == producer.node).count(), 1);
    let position = |id| timings.iter().position(|t| t.node == id).unwrap();
    assert!(position(source.node()) < position(producer.node));
    assert!(position(producer.node) < position(a.node));
    assert!(position(producer.node) < position(b.node));
    let failed: Vec<_> = timings.iter().filter(|t| !t.success).map(|t| (t.node, t.kind)).collect();
    assert_eq!(failed, [(incomplete.node, "test-input-only"), (failure.node, "test-value-check")]);
    assert!(!timings.iter().any(|t| t.node == downstream.node));
    assert_eq!(BATCH_PRODUCED.load(Ordering::Relaxed), 1);
    assert!(values[&producer.node].as_ref().unwrap().bind::<batch_producer::Definition>().is_ok());
    let image = |id| values[id].as_ref().unwrap().bind::<sink::Definition>().unwrap().image.data;
    assert!(std::ptr::eq(image(&a.node), image(&b.node)));
    for id in [failure.node, downstream.node] {
        assert!(matches!(&values[&id], Err(Error::Node { node, .. }) if *node == failure.node));
    }
    assert!(matches!(&values[&incomplete.node], Err(Error::Node { node, source, .. })
        if *node == incomplete.node && matches!(**source, Error::MissingInput { .. })));
}

#[node(id = "test-optional-input")]
fn optional_sink(_: &KernelContext<'_>, _: &(), image: Option<Read<'_, Cpu<Rgb>>>) -> Result<()> {
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
    let bind =
        |values: &InputValues| values.bind::<optional_sink::Definition>().unwrap().image.is_some();
    assert!(!bind(values[&observer.node].as_ref().unwrap()));
    let missing = cpu_copy::add(&mut dag, ()).unwrap();
    dag.connect(missing.output, observer.image).unwrap();
    assert!(
        eval.evaluate_inputs(&dag, &Default::default(), &[observer.node])[&observer.node].is_err()
    );
    let source = image(&mut dag);
    dag.connect(source, missing.image).unwrap();
    let values = eval.evaluate_inputs(&dag, &Default::default(), &[observer.node]);
    assert!(bind(values[&observer.node].as_ref().unwrap()));
}

fn context_contract(global: &drip::runtime::GlobalContext, _: &()) -> Result<(RgbDesc,)> {
    Ok((RgbDesc::new(Extent { width: global.scale, height: 1 }, image_desc().coordinates),))
}
#[node(id="test-global-context",contract=context_contract)]
fn context_source(context: &KernelContext<'_>, _: &(), output: Write<'_, Cpu<Rgb>>) -> Result<()> {
    assert_eq!(output.desc.extent.width, context.global.scale);
    output.data.fill([context.global.scale as f32; 3]);
    Ok(())
}
fn fixed_width_contract(_: &drip::runtime::GlobalContext, _: &(), image: &RgbDesc) -> Result<()> {
    if image.extent.width != 1 {
        return Err(Error::Contract("requires width 1".into()));
    }
    Ok(())
}
#[node(id="test-fixed-width",contract=fixed_width_contract)]
fn fixed_width(_: &KernelContext<'_>, _: &(), _image: Read<'_, Cpu<Rgb>>) -> Result<()> {
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
    dag.connect(source.output, fixed.image).unwrap();
    let evaluator = Evaluator::new(RuntimeContext::host());
    let global = GlobalContext { scale: 3 };
    let value = evaluator.evaluate::<Cpu<Rgb>>(&dag, &global, copy.output).unwrap();
    assert_eq!(value.desc.extent.width, 3);
    assert_eq!(&**value.data, &[[3.; 3]; 3]);
    let full = evaluator.evaluate::<Cpu<Rgb>>(&dag, &Default::default(), copy.output).unwrap();
    assert_eq!(full.desc.extent.width, 1);
    let batch = evaluator.evaluate_inputs(&dag, &global, &[copy.node, fixed.node]);
    let inputs = batch[&copy.node].as_ref().unwrap().bind::<cpu_copy::Definition>().unwrap();
    assert_eq!(inputs.image.desc.extent.width, 3);
    assert!(batch[&fixed.node].as_ref().err().unwrap().to_string().contains("requires width 1"));
    assert!(
        evaluator.evaluate_inputs(&dag, &Default::default(), &[fixed.node])[&fixed.node].is_ok()
    );
    assert!(
        evaluator.evaluate::<Cpu<Rgb>>(&dag, &GlobalContext { scale: 0 }, copy.output).is_err()
    );
}
