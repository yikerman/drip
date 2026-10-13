use super::{histogram, vectorscope, waveform};
use std::sync::Arc;

use super::{SIZE, ScopeAxes};
use crate::model::Graph;
use crate::node_ui::data::{HostImage, Placement, PrepareContext};
use crate::node_ui::scopes::{ExposureRange, Scale, histogram::Settings};
use crate::node_ui::{GuiNode, Prepare};
use drip::{eval::Evaluator, graph::Dag, node::data::*, ports::NodeId, runtime::RuntimeContext};
use serde_json::json;

/// What scope node `G`, added to `dag` as `node`, prepares from its counts,
/// evaluated at full detail on `ctx`'s runtime.
fn prepared<G: GuiNode>(dag: &Dag, node: NodeId, ctx: &PrepareContext) -> G::Presentation {
    let evaluator = Evaluator::new(ctx.runtime.clone());
    let mut values = evaluator.evaluate_inputs(dag, &ctx.global, &[node]);
    let values = values.remove(&node).unwrap().unwrap();
    let Some(Prepare::Outputs(prepare)) = G::PREPARE else { panic!("scopes prepare counts") };
    prepare(G::of(dag.node(node).unwrap()).unwrap(), values.bind_outputs::<G>().unwrap(), ctx)
        .unwrap()
}

/// Scope node `G` with `parameters` over `image`'s samples as Rec.2020/D65.
fn scope<G: GuiNode>(
    parameters: &G::Parameters,
    image: &HostImage,
    ctx: &PrepareContext,
) -> G::Presentation {
    let mut dag = Dag::new();
    let extent = Extent { width: image.width as u32, height: image.height as u32 };
    let desc = RgbDesc::new(extent, Coordinates::Space(REC2020_D65));
    let source = drip::node::source::<Rgb>(&mut dag, desc, image.pixels.to_vec()).unwrap();
    let node = (G::KIND.build)(serde_json::to_value(parameters).unwrap()).unwrap();
    let node = dag.add(node).unwrap();
    dag.connect_ids(source.id(), dag.input_port(node, "image").unwrap()).unwrap();
    prepared::<G>(&dag, node, ctx)
}

#[test]
#[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
fn exposure_scopes_use_samples_in_both_rgb_interpretations() {
    let pixels = vec![[0.18, 0.0, -1.0], [0.09, 1e6, 0.36], [1.0, 0.5, 2.0], [0.25; 3]];
    let mut dag = Dag::new();
    let extent = Extent { width: 2, height: 2 };
    let working = drip::node::source::<Rgb>(
        &mut dag,
        RgbDesc::new(extent.clone(), Coordinates::Space(REC2020_D65)),
        pixels.clone(),
    )
    .unwrap();
    let response = Response {
        camera: Camera { make: "Native".into(), model: "camera".into() },
        balance: None,
    };
    let camera = drip::node::source::<Rgb>(
        &mut dag,
        RgbDesc::new(extent, Coordinates::Camera(response)),
        pixels,
    )
    .unwrap();
    // One histogram kind plots either coordinate system without conversion.
    let range = ExposureRange { min_ev: -12, max_ev: 4 };
    let settings = Settings { range: range.clone(), scale: Scale::Log };
    let waveform_settings = waveform::Settings { range, scale: Scale::Log };
    let mut scopes = Vec::new();
    for input in [working, camera] {
        let histogram = histogram::observe::add(&mut dag, settings.clone()).unwrap();
        let waveform = waveform::observe::add(&mut dag, waveform_settings.clone()).unwrap();
        dag.connect(input, histogram.image).unwrap();
        dag.connect(input, waveform.image).unwrap();
        scopes.push((histogram.node, waveform.node));
    }
    let ctx = context();
    let prepare = |(histogram, waveform)| {
        (
            prepared::<histogram::observe::Definition>(&dag, histogram, &ctx),
            prepared::<waveform::observe::Definition>(&dag, waveform, &ctx),
        )
    };
    let (histogram, waveform) = prepare(scopes[0]);
    assert_eq!(histogram.counts.iter().flatten().sum::<u32>(), 12);
    assert_eq!(waveform.counts.iter().flatten().sum::<u32>(), 12);
    let (camera_histogram, camera_waveform) = prepare(scopes[1]);
    assert_eq!(histogram.counts, camera_histogram.counts);
    assert_eq!(waveform.counts, camera_waveform.counts);
    assert_eq!(histogram.color_space, "Rec.2020 / D65");
    assert_eq!(camera_histogram.color_space, "Camera RGB");
}

#[test]
#[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
fn histogram_bins_by_stops() {
    let image = HostImage {
        width: 2,
        height: 1,
        plane: Placement::whole([2.0, 1.0]),
        requested_scale: 1,
        color_space: "Rec.2020 / D65".into(),
        pixels: vec![[0.18, 0.0, -1.0], [0.09, 1e6, 0.36]].into(),
    };
    let mut graph = Graph::default();
    let id = graph.add_node(&histogram::observe::KIND).unwrap();
    let context = context();
    let prepare = |graph: &Graph| {
        let p: Settings = serde_json::from_value(graph.node(id).unwrap().params).unwrap();
        scope::<histogram::observe::Definition>(&p, &image, &context)
    };
    let h = prepare(&graph);
    let bin = |v: f32| ((v.log2() - h.min_stop) / (h.max_stop - h.min_stop) * 256.0) as usize;
    assert_eq!(h.counts.iter().flatten().sum::<u32>(), 6);
    assert_eq!(h.counts[0], [0, 1, 1], "zero and negative values");
    assert_eq!(h.counts[255], [0, 1, 0], "values beyond the range");
    assert_eq!(h.counts[bin(0.18)][0], 1);
    assert_eq!(h.counts[bin(0.36)][2], 1);
    assert!(!h.log, "linear scale by default");

    graph.set_param(id, "min_ev", json!(-2)).unwrap();
    graph.set_param(id, "max_ev", json!(1)).unwrap();
    graph.set_param(id, "scale", json!("log")).unwrap();
    let h = prepare(&graph);
    assert_eq!((h.min_stop, h.max_stop, h.log), (-2.0, 1.0, true));
    assert_eq!(h.counts[0], [2, 1, 1], "0.18 and 0.09 now lie below the range");
    assert_eq!(h.counts[((0.36f32.log2() + 2.0) / 3.0 * 256.0) as usize][2], 1);
}

#[test]
#[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
fn vectorscope_uses_working_rec2020_coordinates() {
    let image = HostImage {
        width: 1,
        height: 1,
        plane: Placement::whole([1.0, 1.0]),
        requested_scale: 1,
        color_space: "Rec.2020 / D65".into(),
        pixels: vec![[1.0, 0.0, 0.0]].into(),
    };
    let context = context();
    let scope = scope::<vectorscope::observe::Definition>(&(), &image, &context);
    let ScopeAxes::Vectorscope { primaries } = scope.axes else { panic!("chromaticity") };
    assert_eq!(scope.color_space, "Rec.2020 / D65");
    // Independently derive CIE u'v' for Rec.2020 red (x=.708, y=.292), relative to D65.
    let expected: [f64; 2] = [
        0.5 + 4.0 * 0.708 / (-2.0 * 0.708 + 12.0 * 0.292 + 3.0) - 0.1978300066,
        0.5 - 9.0 * 0.292 / (-2.0 * 0.708 + 12.0 * 0.292 + 3.0) + 0.4683199949,
    ];
    for (actual, expected) in primaries[0].into_iter().zip(expected) {
        assert!((f64::from(actual) - expected).abs() < 1e-6);
    }
    let [x, y] = expected.map(|v| (v * SIZE as f64) as usize);
    assert_eq!(scope.counts[y * SIZE + x][0], 1);
    assert_eq!(scope.counts.iter().map(|count| count[0]).sum::<u32>(), 1);
}

pub(crate) fn context() -> PrepareContext {
    let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
    PrepareContext { runtime: evaluator.runtime().clone(), ..Default::default() }
}

/// What Vectorscope prepares from `image`'s samples, for its tests.
pub(crate) fn vectorscope_of(image: &HostImage, ctx: &PrepareContext) -> Arc<super::Scope> {
    scope::<vectorscope::observe::Definition>(&(), image, ctx)
}

#[test]
#[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
fn device_reductions_match_reference() {
    let ctx = context();
    let settings = Settings::default();
    let edges = std::array::from_fn(|i| 2f32.powf(-12.0 + i as f32 / 16.0));
    let mut pixels = Vec::new();
    for &edge in &edges {
        pixels.extend([[edge.next_down(), edge, edge.next_up()]; 33]);
    }
    pixels.extend([
        [f32::NAN, f32::NEG_INFINITY, f32::INFINITY],
        [-1.0, 0.0, f32::MIN_POSITIVE],
        [16.0, 1e20, f32::MAX],
    ]);
    // Odd width, partial workgroups, and empty waveform columns.
    for width in [1, 17, 257] {
        let height = pixels.len().div_ceil(width);
        let mut input = pixels.clone();
        input.resize(width * height, [0.18; 3]);
        let image = HostImage {
            width,
            height,
            plane: Placement::whole([width as f32, height as f32]),
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: input.into(),
        };
        assert_eq!(
            scope::<histogram::observe::Definition>(&settings, &image, &ctx).counts,
            super::histogram::count(&image.pixels, &edges)
        );
        let waveform_settings =
            waveform::Settings { range: settings.range.clone(), ..Default::default() };
        assert_eq!(
            scope::<waveform::observe::Definition>(&waveform_settings, &image, &ctx).counts,
            super::waveform::waveform_counts(&image, -12.0, 4.0)
        );
    }
}
