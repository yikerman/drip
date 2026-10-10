use super::{histogram::histogram, waveform::waveform};
use crate::node_ui::vectorscope::vectorscope;
use std::sync::Arc;

use super::ScopeAxes;
use crate::model::{Graph, Registry};
use crate::node_ui::data::{PrepareContext, Rgb};
use crate::node_ui::scopes::{ExposureSettings, Scale};
use drip::{eval::Evaluator, graph::Dag, node::data::*, runtime::RuntimeContext};
use serde_json::json;

#[test]
#[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
fn exposure_scopes_use_samples_in_both_rgb_interpretations() {
    let pixels = Arc::new(Rgb {
        width: 2,
        height: 2,
        requested_scale: 1,
        color_space: "Rec.2020 / D65".into(),
        pixels: std::sync::Arc::new(
            vec![[0.18, 0.0, -1.0], [0.09, 1e6, 0.36], [1.0, 0.5, 2.0], [0.25; 3]].into(),
        ),
    });
    let mut dag = Dag::new();
    let working = drip::node::source::<ColorRgb>(
        &mut dag,
        ImageDesc {
            extent: Extent { width: 2, height: 2 },
            interpretation: drip::node::raw::working_color(),
        },
        pixels.pixels.to_vec(),
    )
    .unwrap();
    let camera = drip::node::source::<CameraRgb>(
        &mut dag,
        ImageDesc {
            extent: Extent { width: 2, height: 2 },
            interpretation: Camera {
                coordinates: "native camera".into(),
                scale: "relative".into(),
            },
        },
        pixels.pixels.to_vec(),
    )
    .unwrap();
    let histogram = crate::node_ui::histogram::add(&mut dag, ExposureSettings::default()).unwrap();
    let camera_histogram =
        crate::node_ui::camera_histogram::add(&mut dag, ExposureSettings::default()).unwrap();
    dag.connect(working, histogram.image).unwrap();
    dag.connect(camera, camera_histogram.image).unwrap();
    assert!(dag.connect_ids(camera.id(), histogram.image.id()).is_err());
    let targets = [histogram.image.node(), camera_histogram.image.node()];
    let values = Evaluator::new(RuntimeContext::from_env().unwrap()).evaluate_inputs(
        &dag,
        &Default::default(),
        &targets,
    );
    let ctx = context();
    let a = ctx.device_image::<Color>(values[&targets[0]].as_ref().unwrap()).unwrap();
    let b = ctx.device_image::<Camera>(values[&targets[1]].as_ref().unwrap()).unwrap();
    fn prepare<I: crate::node_ui::data::DisplayInterpretation>(
        image: &crate::node_ui::data::DeviceRgb<I>,
        ctx: &PrepareContext,
    ) -> (Arc<super::Histogram>, Arc<super::Scope>) {
        let settings = || ExposureSettings { min_ev: -12, max_ev: 4, scale: Scale::Log };
        (
            super::histogram::histogram(settings(), (image,), ctx).unwrap(),
            waveform(settings(), (image,), ctx).unwrap(),
        )
    }
    let (histogram, waveform) = prepare(&a, &ctx);
    assert_eq!(histogram.counts.iter().flatten().sum::<u32>(), 12);
    assert_eq!(waveform.counts.iter().flatten().sum::<u32>(), 12);
    let (camera_histogram, camera_waveform) = prepare(&b, &ctx);
    assert_eq!(histogram.counts, camera_histogram.counts);
    assert_eq!(waveform.counts, camera_waveform.counts);
    assert_eq!(histogram.color_space, "Rec.2020 / D65");
    assert_eq!(camera_histogram.color_space, "Camera RGB");
}

#[test]
#[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
fn histogram_bins_by_stops() {
    let image = Rgb {
        width: 2,
        height: 1,
        requested_scale: 1,
        color_space: "Rec.2020 / D65".into(),
        pixels: std::sync::Arc::new(vec![[0.18, 0.0, -1.0], [0.09, 1e6, 0.36]].into()),
    };
    let mut graph = Graph::default();
    let id = graph.add_node(Registry.get("view.histogram").unwrap()).unwrap();
    let context = context();
    let prepare = |graph: &Graph| {
        let params = serde_json::from_value(graph.node(id).unwrap().params).unwrap();
        histogram(params, (&device(&image, &context),), &context).unwrap()
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
    let image = Rgb {
        width: 1,
        height: 1,
        requested_scale: 1,
        color_space: "Rec.2020 / D65".into(),
        pixels: std::sync::Arc::new(vec![[1.0, 0.0, 0.0]].into()),
    };
    let context = context();
    let scope = vectorscope((), (&device(&image, &context),), &context).unwrap();
    let ScopeAxes::Vectorscope { primaries, color_space } = scope.axes else {
        panic!("chromaticity")
    };
    assert_eq!(color_space, "Rec.2020");
    // Independently derive CIE u'v' for Rec.2020 red (x=.708, y=.292), relative to D65.
    let expected: [f64; 2] = [
        0.5 + 4.0 * 0.708 / (-2.0 * 0.708 + 12.0 * 0.292 + 3.0) - 0.1978300066,
        0.5 - 9.0 * 0.292 / (-2.0 * 0.708 + 12.0 * 0.292 + 3.0) + 0.4683199949,
    ];
    for (actual, expected) in primaries[0].into_iter().zip(expected) {
        assert!((f64::from(actual) - expected).abs() < 1e-6);
    }
    let [x, y] = expected.map(|v| (v * scope.size as f64) as usize);
    assert_eq!(scope.counts[y * scope.size + x][0], 1);
    assert_eq!(scope.counts.iter().map(|count| count[0]).sum::<u32>(), 1);
}

pub(crate) fn context() -> PrepareContext {
    let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
    PrepareContext { client: Some(evaluator.client().unwrap().clone()), ..Default::default() }
}

pub(crate) fn device(image: &Rgb, ctx: &PrepareContext) -> crate::node_ui::data::DeviceRgb<Color> {
    let desc = ImageDesc {
        extent: Extent { width: image.width as u32, height: image.height as u32 },
        interpretation: drip::node::raw::working_color(),
    };
    let runtime = RuntimeContext::from_client(ctx.client().unwrap().clone());
    let pixels = ColorRgb::upload(&desc, &image.pixels, &runtime).unwrap();
    crate::node_ui::data::DeviceRgb { desc, pixels: Arc::new(pixels) }
}

#[test]
#[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
fn device_reductions_match_reference() {
    let ctx = context();
    let settings = ExposureSettings::default();
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
        let image = Rgb {
            width,
            height,
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: Arc::new(input.into()),
        };
        let gpu = device(&image, &ctx);
        assert_eq!(
            histogram(settings.clone(), (&gpu,), &ctx).unwrap().counts,
            super::histogram::count(&image.pixels, &edges)
        );
        assert_eq!(
            waveform(settings.clone(), (&gpu,), &ctx).unwrap().counts,
            super::waveform::waveform_counts(&image, -12.0, 4.0)
        );
    }
}
