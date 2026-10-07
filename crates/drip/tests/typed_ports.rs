//! Connection checking and runtime binding agree on concrete Rust payloads.
use drip::color::{self, D65, P3, REC2020};
use drip::eval::Evaluator;
use drip::graph::{Graph, GraphError, Port};
use drip::image::{CameraRgb, Interpretation, Mosaic, RawMetadata, RealMat, Rec2020Mat, Rgb};
use drip::node::{EvalContext, KernelError};
use drip::nodes;
use drip::ports::{Either, Input, Optional, Read, ReadEither};
use drip::value::{TypeDescriptor, Value};
use std::sync::Arc;

fn pixels() -> Arc<Rgb> {
    Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[1.0, 0.0, 0.0]] })
}

#[derive(Debug, Default)]
pub struct P3Interpretation;
impl Interpretation for P3Interpretation {
    const NAME: &'static str = "Linear P3 RGB";
}
type LinearP3 = RealMat<3, P3Interpretation>;

#[drip::node(kind = P3_SOURCE, id = "test.p3", category = "test", name = "P3", outputs = ["image"])]
fn p3_source(_: (), (): (), _: &EvalContext<'_>) -> Result<(Arc<LinearP3>,), KernelError> {
    Ok((Arc::new(LinearP3::from(pixels())),))
}

/// Explicit conversion makes a locally defined interpretation usable by ordinary nodes.
#[drip::node(kind = TO_WORKING, id = "test.p3_to_working", category = "test", name = "P3 to working RGB", outputs = ["image"])]
fn to_working(
    _: (),
    (image,): (&LinearP3,),
    _: &EvalContext<'_>,
) -> Result<(Arc<Rec2020Mat>,), KernelError> {
    let matrix =
        color::mul(&color::inverse(&color::rgb_to_xyz(REC2020, D65)), &color::rgb_to_xyz(P3, D65));
    Ok((Arc::new(Rec2020Mat::from(Arc::new(
        image.buffer().map(|v| color::apply(&matrix, v.map(f64::from)).map(|v| v as f32)),
    ))),))
}

#[test]
fn concrete_binding_shares_storage_and_does_not_trust_labels() {
    let data = pixels();
    let value = Value::new(Arc::new(Rec2020Mat::from(data.clone())));
    let input = value.borrow::<Read<Rec2020Mat>>().unwrap();
    assert!(Arc::ptr_eq(input.buffer(), &data));
    assert!(value.borrow::<Read<CameraRgb>>().is_none());
    #[derive(Debug, Default)]
    struct Impostor;
    impl Interpretation for Impostor {
        const NAME: &'static str = "Rec.2020 RGB";
    }
    let impostor = Value::new(Arc::new(RealMat::<3, Impostor>::from(data)));
    assert_eq!(value.descriptor().name, impostor.descriptor().name);
    assert!(impostor.borrow::<Read<Rec2020Mat>>().is_none());
}

#[test]
fn diagnostic_alternatives_check_and_bind_the_same_concrete_types() {
    type ScopeInput = ReadEither<Rec2020Mat, CameraRgb>;
    let working = Value::new(Arc::new(Rec2020Mat::from(pixels())));
    let camera = Value::new(Arc::new(CameraRgb::new(
        pixels(),
        drip::image::Camera {
            xyz_to_cam: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            white_balance: [1.0; 4],
        },
    )));
    assert!(matches!(working.borrow::<ScopeInput>(), Some(Either::First(_))));
    assert!(matches!(camera.borrow::<ScopeInput>(), Some(Either::Second(_))));
    assert!(<Optional<ScopeInput> as Input>::read_slot(None).is_none());
    for kind in [nodes::HISTOGRAM.kind(), nodes::WAVEFORM.kind()] {
        let requirement = kind.input("image").unwrap().requirement;
        for value in [&working, &camera] {
            assert!(requirement.accepts(value.descriptor()));
        }
        for ty in [
            TypeDescriptor::of::<Mosaic>(),
            TypeDescriptor::of::<RawMetadata>(),
            TypeDescriptor::of::<LinearP3>(),
        ] {
            assert!(!requirement.accepts(&ty));
        }
    }
    for kind in [
        nodes::PREVIEW.kind(),
        nodes::TIFF.kind(),
        nodes::SIGMOID.kind(),
        nodes::EXPOSURE.kind(),
        nodes::VECTORSCOPE.kind(),
    ] {
        assert!(!kind.input("image").unwrap().requirement.accepts(camera.descriptor()));
        assert!(kind.input("image").unwrap().requirement.accepts(working.descriptor()));
    }
}

#[test]
fn all_outputs_are_known_before_evaluation_and_failed_edits_are_atomic() {
    let mut graph = Graph::default();
    let p3 = graph.add_node(&P3_SOURCE);
    let convert = graph.add_node(&TO_WORKING);
    let exposure = graph.add_node(&nodes::EXPOSURE);
    let preview = graph.add_node(&nodes::PREVIEW);
    let port = |id| Port(id, "image".into());
    assert!(graph.output_type(&port(exposure)).unwrap().is::<Rec2020Mat>());
    graph.connect(port(exposure), port(preview)).unwrap();
    assert!(matches!(
        graph.connect(port(p3), port(exposure)),
        Err(GraphError::TypeMismatch { .. })
    ));
    assert!(graph.source(&port(exposure)).is_none());
    graph.connect(port(p3), port(convert)).unwrap();
    graph.connect(port(convert), port(exposure)).unwrap();
    assert!(graph.connect(port(p3), port(exposure)).is_err());
    assert_eq!(graph.source(&port(exposure)), Some(&port(convert)));
    let mut evaluator = Evaluator::default();
    evaluator.evaluate(&graph, 0, &[preview]);
    assert!(evaluator.result(preview).unwrap().is_ok());
    let value = &evaluator.result(exposure).unwrap().as_ref().unwrap()[0];
    let output = value.downcast_ref::<Rec2020Mat>().unwrap();
    let xyz = color::apply(&color::rgb_to_xyz(P3, D65), [1.0, 0.0, 0.0]);
    let expected = color::apply(&color::inverse(&color::rgb_to_xyz(REC2020, D65)), xyz);
    for (actual, expected) in output.pixels[0].into_iter().zip(expected) {
        assert!((f64::from(actual) - expected).abs() < 1e-6);
    }
}

#[test]
fn creative_ordering_is_allowed_and_uses_the_working_rgb_convention() {
    let mut graph = Graph::default();
    let source = graph.add_node(&P3_SOURCE);
    let convert = graph.add_node(&TO_WORKING);
    let tone = graph.add_node(&nodes::SIGMOID);
    let exposure = graph.add_node(&nodes::EXPOSURE);
    let second_tone = graph.add_node(&nodes::SIGMOID);
    let chain = [source, convert, tone, exposure, second_tone];
    for pair in chain.windows(2) {
        graph.connect(Port(pair[0], "image".into()), Port(pair[1], "image".into())).unwrap();
    }
    graph.set_param(exposure, "ev", 1.0.into()).unwrap();
    let mut eval = Evaluator::default();
    eval.evaluate(&graph, 0, &[second_tone]);
    let image =
        |id| eval.result(id).unwrap().as_ref().unwrap()[0].downcast_ref::<Rec2020Mat>().unwrap();
    assert_eq!(image(exposure).pixels[0], image(tone).pixels[0].map(|v| v * 2.0));
    assert_eq!(
        image(second_tone).pixels[0],
        nodes::sigmoid::Sigmoid::new(1.5, -0.2, 0.0).pixel(image(exposure).pixels[0])
    );
    assert!(drip::nodes::registry().get(P3_SOURCE.id).is_some());
}
