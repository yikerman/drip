//! Contract checking and actual borrows share generated trait evidence.
use bevy_reflect::Reflect;
use drip::color::{self, D65, P3};
use drip::eval::Evaluator;
use drip::graph::{Graph, Port};
use drip::image::{
    Colorimetry, LinearRgb, LinearRgbColorSpace, Linearity, RealMat, Rec2020, Rec2020Mat,
    Rec2020Rgb, Rgb, ScaleInvariant,
};
use drip::node::{EvalContext, KernelError};
use drip::nodes;

use drip::ports::{Input, Read, ReadMat};
use drip::value::{TypeDescriptor, Value};
use std::sync::{Arc, LazyLock};

fn pixels() -> Arc<Rgb> {
    Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[1.0, 0.0, 0.0]] })
}

#[drip::interpretation()]
#[derive(Debug, Default, Reflect)]
struct Encoded;
type EncodedMat = RealMat<3, Encoded>;

// No edits to a capability catalogue or consuming nodes accompany this type.
#[drip::interpretation(ScaleInvariant)]
#[derive(Debug, Default, Reflect)]
pub struct P3Interpretation;
impl Linearity for P3Interpretation {}
impl ScaleInvariant for P3Interpretation {}
impl Colorimetry for P3Interpretation {
    fn to_xyz_d65(&self, sample: [f32; 3]) -> [f32; 3] {
        color::apply(&self.color_space().to_xyz_d65, sample.map(f64::from)).map(|v| v as f32)
    }
}
impl LinearRgb for P3Interpretation {
    fn color_space(&self) -> &LinearRgbColorSpace {
        static SPACE: LazyLock<LinearRgbColorSpace> = LazyLock::new(|| LinearRgbColorSpace {
            name: "Display P3",
            to_xyz_d65: color::rgb_to_xyz(P3, D65),
        });
        &SPACE
    }
}
type LinearP3 = RealMat<3, P3Interpretation>;

#[drip::node(kind = P3_SOURCE, id = "test.p3", category = "test", name = "P3", outputs = ["image"])]
fn p3_source(_: (), (): (), _: &EvalContext<'_>) -> Result<(Arc<LinearP3>,), KernelError> {
    Ok((Arc::new(LinearP3::from(pixels())),))
}

#[test]
fn generated_ancestor_evidence_agrees_with_borrows_without_copying_samples() {
    let data = pixels();
    let value = Value::new(Arc::new(Rec2020Mat::from(data.clone())));
    let exact = value.borrow::<Read<Rec2020Mat>>().unwrap();
    assert!(Arc::ptr_eq(exact.buffer(), &data));
    let linear = value.borrow::<ReadMat<3, dyn Linearity>>().unwrap();
    let color = value.borrow::<ReadMat<3, dyn Colorimetry>>().unwrap();
    let rgb = value.borrow::<ReadMat<3, dyn LinearRgb>>().unwrap();
    let preview = value.borrow::<ReadMat<3, dyn Rec2020Rgb>>().unwrap();
    assert!(Arc::ptr_eq(linear.rgb(), &data));
    assert_eq!(color.interpretation.to_xyz_d65([1.0; 3]), Rec2020.to_xyz_d65([1.0; 3]));
    assert_eq!(rgb.interpretation.color_space().name, "Rec.2020");
    assert!(Arc::ptr_eq(preview.rgb(), &data));
    assert!(value.borrow::<ReadMat<1, dyn Linearity>>().is_none());
    let encoded = Value::new(Arc::new(EncodedMat::from(data)));
    assert!(encoded.borrow::<Read<Rec2020Mat>>().is_none());
    assert!(encoded.borrow::<ReadMat<3, dyn Linearity>>().is_none());
    for kind in [
        nodes::HISTOGRAM.kind(),
        nodes::WAVEFORM.kind(),
        nodes::VECTORSCOPE.kind(),
        nodes::PREVIEW.kind(),
        nodes::EXPOSURE.kind(),
    ] {
        assert!(!kind.input("image").unwrap().requirement.accepts(encoded.descriptor()));
    }
}

#[test]
fn camera_and_mosaic_do_not_claim_colorimetric_preview_support() {
    let camera = TypeDescriptor::of::<drip::image::CameraRgb>();
    for kind in [nodes::HISTOGRAM.kind(), nodes::WAVEFORM.kind()] {
        assert!(kind.input("image").unwrap().requirement.accepts(&camera));
        for ty in [
            TypeDescriptor::of::<drip::image::Mosaic>(),
            TypeDescriptor::of::<drip::image::RawMetadata>(),
        ] {
            assert!(!kind.input("image").unwrap().requirement.accepts(&ty));
        }
    }
    assert!(!ReadMat::<3, dyn LinearRgb>::REQUIREMENT.accepts(&camera));
    assert!(!ReadMat::<3, dyn Rec2020Rgb>::REQUIREMENT.accepts(&camera));
}

#[test]
fn node_declaration_is_discovered_without_editing_a_registry() {
    assert_eq!(nodes::registry().get("test.p3").unwrap().id, P3_SOURCE.id);
}

#[test]
fn exposure_preserves_concrete_p3_interpretation_and_propagates_it_before_eval() {
    let mut graph = Graph::default();
    let source = graph.add_node(&P3_SOURCE);
    let exposure = graph.add_node(nodes::EXPOSURE.kind());
    let input = Port(exposure, "image".into());
    assert!(graph.output_type(&input).is_none());
    graph.connect(Port(source, "image".into()), input.clone()).unwrap();
    assert!(graph.output_type(&input).unwrap().is::<LinearP3>());
    graph.set_param(exposure, "ev", 1.0.into()).unwrap();
    let mut eval = Evaluator::default();
    eval.evaluate(&graph, 0, &[exposure]);
    let value = &eval.result(exposure).unwrap().as_ref().unwrap()[0];
    let image = value.downcast_ref::<LinearP3>().unwrap();
    assert_eq!(image.rgb().pixels[0], [2.0, 0.0, 0.0]);
    assert_eq!(image.interpretation().color_space().name, "Display P3");
}

#[test]
fn resolving_a_pending_preserved_output_rechecks_descendants_atomically() {
    let mut graph = Graph::default();
    let source = graph.add_node(&P3_SOURCE);
    let exposure = graph.add_node(nodes::EXPOSURE.kind());
    let preview = graph.add_node(nodes::PREVIEW.kind());
    graph.connect(Port(exposure, "image".into()), Port(preview, "image".into())).unwrap();
    let err =
        graph.connect(Port(source, "image".into()), Port(exposure, "image".into())).unwrap_err();
    assert!(
        matches!(err, drip::graph::GraphError::TypeMismatch { input, .. } if input.0 == preview)
    );
    assert!(graph.source(&Port(exposure, "image".into())).is_none());
    assert_eq!(graph.edges().count(), 1);
}

#[test]
fn scope_declarations_accept_p3_and_cache_input_validation() {
    let mut graph = Graph::default();
    let source = graph.add_node(&P3_SOURCE);
    let probes: Vec<_> =
        [nodes::HISTOGRAM.kind(), nodes::WAVEFORM.kind(), nodes::VECTORSCOPE.kind()]
            .into_iter()
            .map(|kind| graph.add_node(kind))
            .collect();
    for &probe in &probes {
        graph.connect(Port(source, "image".into()), Port(probe, "image".into())).unwrap();
    }
    for kind in [nodes::PREVIEW.kind(), nodes::TIFF.kind()] {
        let incompatible = graph.add_node(kind);
        assert!(
            graph
                .connect(Port(source, "image".into()), Port(incompatible, "image".into()))
                .is_err()
        );
    }
    let mut evaluator = Evaluator::default();
    assert_eq!(evaluator.evaluate(&graph, 0, &probes).len(), 4);
    assert!(evaluator.evaluate(&graph, 0, &probes).is_empty());
    for &probe in &probes {
        assert!(evaluator.result(probe).unwrap().is_ok());
    }
}

/// Every channel lies in [0, 1]; this promise is not closed under exposure.
#[drip::capability]
trait UnitRange: Rec2020Rgb {}
#[drip::interpretation(UnitRange)]
#[derive(Debug, Default, Reflect)]
pub struct BoundedRec2020;
impl Linearity for BoundedRec2020 {}
impl Colorimetry for BoundedRec2020 {
    fn to_xyz_d65(&self, sample: [f32; 3]) -> [f32; 3] {
        Rec2020.to_xyz_d65(sample)
    }
}
impl LinearRgb for BoundedRec2020 {
    fn color_space(&self) -> &LinearRgbColorSpace {
        Rec2020.color_space()
    }
}
impl Rec2020Rgb for BoundedRec2020 {}
impl UnitRange for BoundedRec2020 {}
#[drip::node(kind = BOUNDED, id = "test.bounded", category = "test", name = "Bounded", outputs = ["image"])]
fn bounded(
    _: (),
    (): (),
    _: &EvalContext<'_>,
) -> Result<(Arc<RealMat<3, BoundedRec2020>>,), KernelError> {
    Ok((Arc::new(RealMat::new(pixels(), BoundedRec2020)),))
}

#[test]
fn exposure_cannot_silently_preserve_a_stronger_range_law() {
    let mut graph = Graph::default();
    let source = graph.add_node(&BOUNDED);
    let preview = graph.add_node(nodes::PREVIEW.kind());
    let exposure = graph.add_node(nodes::EXPOSURE.kind());
    let port = |id| Port(id, "image".into());
    graph.connect(port(source), port(preview)).unwrap();
    let error = graph.connect(port(source), port(exposure)).unwrap_err();
    assert!(matches!(error, drip::graph::GraphError::TypeMismatch { mismatch, .. }
        if mismatch.expected == "ScaleInvariant"));
    let mut evaluator = Evaluator::default();
    evaluator.evaluate(&graph, 0, &[preview]);
    assert!(evaluator.result(preview).unwrap().is_ok());
}

#[test]
fn sigmoid_accepts_rec2020_refinements_and_establishes_only_its_output_laws() {
    let mut graph = Graph::default();
    let source = graph.add_node(&BOUNDED);
    let sigmoid = graph.add_node(nodes::SIGMOID.kind());
    let exposure = graph.add_node(nodes::EXPOSURE.kind());
    let port = |id| Port(id, "image".into());
    graph.connect(port(source), port(sigmoid)).unwrap();
    graph.connect(port(sigmoid), port(exposure)).unwrap();
    assert!(graph.output_type(&port(sigmoid)).unwrap().is::<Rec2020Mat>());
    let mut evaluator = Evaluator::default();
    evaluator.evaluate(&graph, 0, &[exposure]);
    let value = &evaluator.result(sigmoid).unwrap().as_ref().unwrap()[0];
    assert!(value.borrow::<ReadMat<3, dyn UnitRange>>().is_none());
    assert!(value.borrow::<ReadMat<3, dyn Rec2020Rgb>>().is_some());
    assert!(evaluator.result(exposure).unwrap().is_ok());
}
