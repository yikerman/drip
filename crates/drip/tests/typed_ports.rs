//! Capabilities must describe both graph compatibility and the actual borrow.

use std::sync::{Arc, LazyLock};

use drip::color::{self, D65, P3};
use drip::eval::Evaluator;
use drip::graph::{Graph, Port};
use drip::image::{
    ColorspaceRgbMatrix, DisplayRec2020, LinearRgbColorSpace, LinearThreeChannelMatrix, Rec2020,
    Rgb, RgbIn, SceneRec2020, ThreeChannelMatrix,
};
use drip::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use drip::nodes;
use drip::param::Params;
use drip::ports::{Input, InputTuple, OutputTuple, Read};
use drip::value::{Describe, EdgeValue, TypeDescriptor, Value};
use drip::view::{PreviewImage, ScopeAxes, View};

fn pixels() -> Arc<Rgb> {
    Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[1.0, 0.0, 0.0]] })
}

#[derive(Debug)]
struct EncodedChannels(Arc<Rgb>);
impl ThreeChannelMatrix for EncodedChannels {
    fn rgb(&self) -> &Arc<Rgb> {
        &self.0
    }
}
impl EdgeValue for EncodedChannels {
    const TYPE: TypeDescriptor = Describe::<Self>::new("encoded channels").channels().build();
}

// A new color space needs neither payload equality nor edits to consuming nodes.
#[derive(Debug)]
struct LinearP3(Arc<Rgb>);
impl ThreeChannelMatrix for LinearP3 {
    fn rgb(&self) -> &Arc<Rgb> {
        &self.0
    }
}
impl LinearThreeChannelMatrix for LinearP3 {}
impl ColorspaceRgbMatrix for LinearP3 {
    fn color_space(&self) -> &LinearRgbColorSpace {
        static SPACE: LazyLock<LinearRgbColorSpace> = LazyLock::new(|| LinearRgbColorSpace {
            name: "Display P3",
            to_xyz_d65: color::rgb_to_xyz(P3, D65),
        });
        &SPACE
    }
}
impl EdgeValue for LinearP3 {
    const TYPE: TypeDescriptor = Describe::<Self>::new("linear P3").color().build();
}

struct P3Source;
impl NodeKernel for P3Source {
    type Inputs = ();
    type Outputs = (Arc<LinearP3>,);
    fn eval(
        _: Params<'_>,
        (): (),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        Ok(Evaluated::new((Arc::new(LinearP3(pixels())),)))
    }
}
static P3_SOURCE: NodeKind =
    NodeKind::new::<P3Source>("test.p3", "test", "P3", &[], &[], &["image"]);

#[test]
fn capabilities_preserve_semantics_and_share_pixels() {
    let data = pixels();
    let scene = Arc::new(SceneRec2020::from(data.clone()));
    let display = Arc::new(DisplayRec2020::from(data.clone()));
    let outputs = (scene.clone(), display.clone()).erase();
    let slots: Vec<_> = outputs.iter().cloned().map(Some).collect();
    let (scene_ref, display_ref) = <(Read<SceneRec2020>, Read<DisplayRec2020>)>::read(&slots);
    assert!(std::ptr::eq(scene_ref, &*scene));
    assert!(std::ptr::eq(display_ref, &*display));
    assert!(outputs[0].borrow::<Read<DisplayRec2020>>().is_none());
    assert!(outputs[1].borrow::<Read<SceneRec2020>>().is_none());
    for value in &outputs {
        let channels = value.borrow::<Read<dyn ThreeChannelMatrix>>().unwrap();
        let linear = value.borrow::<Read<dyn LinearThreeChannelMatrix>>().unwrap();
        let color = value.borrow::<Read<dyn ColorspaceRgbMatrix>>().unwrap();
        let preview = value.borrow::<Read<dyn RgbIn<Rec2020>>>().unwrap();
        assert!(Arc::ptr_eq(channels.rgb(), &data));
        assert!(Arc::ptr_eq(linear.rgb(), &data));
        assert_eq!(color.color_space().name, "Rec.2020");
        assert!(Arc::ptr_eq(PreviewImage::new(preview).rgb(), &data));
    }
    let encoded = Value::new(Arc::new(EncodedChannels(data)));
    assert!(encoded.borrow::<Read<dyn ThreeChannelMatrix>>().is_some());
    assert!(encoded.borrow::<Read<dyn LinearThreeChannelMatrix>>().is_none());
    for kind in [&nodes::HISTOGRAM, &nodes::WAVEFORM, &nodes::VECTORSCOPE, &nodes::PREVIEW] {
        assert!(!kind.input("image").unwrap().requirement.accepts(encoded.descriptor()));
    }
}

#[test]
fn camera_metadata_and_mosaic_do_not_claim_colorimetric_preview_support() {
    let camera = nodes::RCD.outputs().next().unwrap().ty;
    for kind in [&nodes::HISTOGRAM, &nodes::WAVEFORM] {
        assert!(kind.input("image").unwrap().requirement.accepts(camera));
        for output in nodes::READ.outputs() {
            assert!(!kind.input("image").unwrap().requirement.accepts(output.ty));
        }
    }
    assert!(!Read::<dyn ColorspaceRgbMatrix>::REQUIREMENT.accepts(camera));
    assert!(!Read::<dyn RgbIn<Rec2020>>::REQUIREMENT.accepts(camera));
}

#[test]
fn scopes_use_connected_color_space_and_cache_the_result() {
    let mut graph = Graph::default();
    let source = graph.add_node(&P3_SOURCE);
    let probes: Vec<_> = [&nodes::HISTOGRAM, &nodes::WAVEFORM, &nodes::VECTORSCOPE]
        .into_iter()
        .map(|kind| graph.add_node(kind))
        .collect();
    for &probe in &probes {
        graph.connect(Port(source, "image".into()), Port(probe, "image".into())).unwrap();
    }
    for kind in [&nodes::PREVIEW, &nodes::EXPOSURE, &nodes::TIFF] {
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
    let result = evaluator.result(probes[2]).unwrap().as_ref().unwrap();
    let Some(View::Scope(scope)) = &result.view else { panic!("vectorscope") };
    let ScopeAxes::Vectorscope { primaries, color_space } = scope.axes else {
        panic!("chromaticity")
    };
    assert_eq!(color_space, "Display P3");
    // Independently derive CIE u'v' for P3 red (x=.68, y=.32), relative to D65.
    let expected: [f64; 2] = [
        0.5 + 4.0 * 0.68 / (-2.0 * 0.68 + 12.0 * 0.32 + 3.0) - 0.1978300066,
        0.5 - 9.0 * 0.32 / (-2.0 * 0.68 + 12.0 * 0.32 + 3.0) + 0.4683199949,
    ];
    for (actual, expected) in primaries[0].into_iter().zip(expected) {
        assert!((f64::from(actual) - expected).abs() < 1e-6);
    }
    let [x, y] = expected.map(|v| (v * scope.size as f64) as usize);
    assert_eq!(scope.counts[y * scope.size + x][0], 1);
    assert_eq!(scope.counts.iter().map(|count| count[0]).sum::<u32>(), 1);
}
