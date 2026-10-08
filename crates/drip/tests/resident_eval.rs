//! Real compute exercises the scheduler's transfer and refinement contracts.
use drip::{
    compute::Compute,
    eval::{Evaluator, Request},
    graph::{Graph, Port},
    image::{ColorCoordinates, ColorImage, ColorMeaning, Cpu, Gpu, Rgb},
    node::KernelError,
    nodes,
};
use std::sync::Arc;

/// A small image with negative values and highlights, already in working coordinates.
#[drip::node(kind=SOURCE,id="resident.source",category="test",name="Source",outputs=["image"])]
fn source() -> Result<Arc<ColorImage<Cpu>>, KernelError> {
    Ok(Arc::new(ColorImage::try_new(
        Arc::new(Rgb {
            width: 37,
            height: 19,
            scale: 1,
            pixels: (0..37 * 19).map(|i| [i as f32 / 1000.0 - 0.1, 0.2, 1.5]).collect(),
        }),
        ColorMeaning::rec2020(),
    )?))
}

/// A host-only edit forces an explicit readback and later upload. Its offset
/// changes the signal relationship, so it declares only resulting color coordinates.
#[drip::node(kind=HOST_EDIT,id="resident.host",category="test",name="Host edit",outputs=["image"])]
fn host_edit(image: &ColorImage<Cpu>) -> Result<Arc<ColorImage<Cpu>>, KernelError> {
    image.require_additive_color()?;
    Ok(Arc::new(ColorImage::try_new(
        Arc::new(image.rgb().map(|v| v.map(|x| x + 0.25))),
        ColorMeaning::rec2020(),
    )?))
}

#[drip::node(kind=SINK,id="resident.sink",category="test",name="Sink",outputs=[])]
fn sink(image: &ColorImage<Cpu>) -> Result<(), KernelError>;

#[drip::node(kind=OKLAB,id="resident.oklab",category="test",name="Oklab",outputs=["image"])]
fn oklab() -> Result<Arc<ColorImage<Cpu>>, KernelError> {
    let mut meaning = ColorMeaning::rec2020();
    meaning.coordinates = ColorCoordinates::Oklab;
    Ok(Arc::new(ColorImage::try_new(
        Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[0.5, 0.0, 0.0]] }),
        meaning,
    )?))
}

fn link(graph: &mut Graph, from: drip::graph::NodeId, to: drip::graph::NodeId) {
    graph.connect(Port(from, "image".into()), Port(to, "image".into())).unwrap();
}

#[test]
fn residency_shared_readback_cpu_island_and_runtime_refinement() {
    let compute = Compute::new().expect("real GPU or software adapter");
    let evaluator = Evaluator::with_compute(compute.clone());
    let mut graph = Graph::default();
    let src = graph.add_node(&SOURCE);
    let gain = graph.add_node(&nodes::EXPOSURE);
    let host = graph.add_node(&HOST_EDIT);
    let next = graph.add_node(&nodes::EXPOSURE);
    let a = graph.add_node(&SINK);
    let b = graph.add_node(&SINK);
    for (x, y) in [(src, gain), (gain, host), (host, next), (next, a), (next, b)] {
        link(&mut graph, x, y);
    }
    graph.set_param(gain, "ev", 1.0.into()).unwrap();
    graph.set_param(next, "ev", (-1.0).into()).unwrap();
    let report = evaluator.request(&graph, 0, &[Request::Inputs(a), Request::Inputs(b)]);
    assert_eq!(report.executed, [src, gain, host, next]);
    assert_eq!(report.transfers, 4, "fanout shares the final readback");
    let read =
        |id| report.with_inputs(id, &SINK, |_, (image,), _| Ok(image.rgb().clone())).unwrap();
    let (first, second) = (read(a), read(b));
    assert!(Arc::ptr_eq(&first, &second));
    for (i, pixel) in first.pixels.iter().enumerate() {
        let expected = [i as f32 / 1000.0 - 0.1 + 0.125, 0.325, 1.625];
        for (a, b) in pixel.iter().zip(expected) {
            assert!((a - b).abs() < 3e-7);
        }
    }
    assert!(report.result(src).is_none(), "source was not retained as an intermediate");
    drop(report);
    graph.set_param(next, "ev", 0.0.into()).unwrap();
    let edited = evaluator.request(&graph, 0, &[Request::Inputs(a)]);
    assert_eq!(edited.executed, [src, gain, host, next], "no cross-request node cache");
    edited
        .with_inputs(a, &SINK, |_, (image,), _| {
            assert!((image.pixels[0][0] - 0.05).abs() < 1e-6);
            Ok(())
        })
        .unwrap();
    drop(edited);

    let wrong = graph.add_node(&OKLAB);
    // Same concrete payload family is a valid connection. The runtime coordinate
    // refinement, not nominal RGB storage, rejects direct exposure multiplication.
    graph.connect(Port(wrong, "image".into()), Port(gain, "image".into())).unwrap();
    let report = evaluator.request(&graph, 0, &[Request::Output(Port(gain, "image".into()))]);
    let error = report.output(&Port(gain, "image".into())).unwrap_err().to_string();
    assert!(error.contains("additive"), "{error}");
    assert!(
        drip::value::TypeDescriptor::of::<ColorImage<Cpu>>()
            .compatible_with(&drip::value::TypeDescriptor::of::<ColorImage<Gpu>>())
    );
}
