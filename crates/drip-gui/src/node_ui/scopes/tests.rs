use super::{
    density::{vectorscope, waveform},
    histogram::histogram,
};
use std::sync::Arc;

use drip::graph::Graph;
use drip::image::{Camera, CameraRgb, ColorImage, Rgb};
use drip::node::EvalContext;
use drip::nodes::scopes::{ExposureSettings, HISTOGRAM, Scale};
use drip::param::{Parameters, Params};
use drip::ports::{Read, ReadEither};
use drip::value::Value;
use serde_json::json;

use super::ScopeAxes;

#[test]
fn exposure_scopes_use_samples_in_both_rgb_interpretations() {
    let pixels = Arc::new(Rgb {
        width: 2,
        height: 2,
        scale: 1,
        pixels: vec![[0.18, 0.0, -1.0], [0.09, 1e6, 0.36], [1.0, 0.5, 2.0], [0.25; 3]],
    });
    let working = Value::new(Arc::new(ColorImage::from(pixels.clone())));
    let camera = Value::new(Arc::new(CameraRgb::new(
        pixels,
        Camera {
            xyz_to_cam: [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]],
            white_balance: [2.0, 1.0, 1.5, 1.0],
        },
    )));
    let resources = Default::default();
    let context = EvalContext::new(0, &resources).unwrap();
    let prepare = |value: &Value| {
        let image = value.borrow::<ReadEither<ColorImage, CameraRgb>>().unwrap();
        let settings = || ExposureSettings { min_ev: -12, max_ev: 4, scale: Scale::Log };
        (
            histogram(settings(), (image,), &context).unwrap(),
            waveform(settings(), (image,), &context).unwrap(),
        )
    };
    let (histogram, waveform) = prepare(&working);
    assert_eq!(histogram.counts.iter().flatten().sum::<u32>(), 12);
    assert_eq!(waveform.counts.iter().flatten().sum::<u32>(), 12);
    // Exposure scopes inspect native channels, without applying camera color or WB transforms.
    assert_eq!((histogram, waveform), prepare(&camera));
}

#[test]
fn histogram_bins_by_stops() {
    let value = Value::new(Arc::new(ColorImage::from(Arc::new(Rgb {
        width: 2,
        height: 1,
        scale: 1,
        pixels: vec![[0.18, 0.0, -1.0], [0.09, 1e6, 0.36]],
    }))));
    let mut graph = Graph::default();
    let id = graph.add_node(&HISTOGRAM);
    let resources = Default::default();
    let context = EvalContext::new(0, &resources).unwrap();
    let prepare = |graph: &Graph| {
        let params = ExposureSettings::read(Params::validated(&graph.node(id).unwrap().params));
        let image = value.borrow::<ReadEither<ColorImage, CameraRgb>>().unwrap();
        histogram(params, (image,), &context).unwrap()
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
fn vectorscope_uses_working_rec2020_coordinates() {
    let source = ColorImage::from(Arc::new(Rgb {
        width: 1,
        height: 1,
        scale: 1,
        pixels: vec![[1.0, 0.0, 0.0]],
    }));
    let value = Value::new(Arc::new(source));
    let image = value.borrow::<Read<ColorImage>>().unwrap();
    let resources = Default::default();
    let context = EvalContext::new(0, &resources).unwrap();
    let scope = vectorscope((), (image,), &context).unwrap();
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

#[test]
fn vectorscope_rejects_unsupported_coordinates_but_accepts_rendered_color() {
    use drip::image::{ColorCoordinates, ColorMeaning, SceneRelationship};
    let pixels = Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[0.18; 3]] });
    let resources = Default::default();
    let context = EvalContext::new(0, &resources).unwrap();
    for coordinates in [ColorCoordinates::Oklab, ColorCoordinates::Unspecified] {
        let image = ColorImage::new(
            pixels.clone(),
            ColorMeaning { coordinates, scene: SceneRelationship::Unspecified },
        );
        assert!(vectorscope((), (&image,), &context).is_err());
        // Raw channel diagnostics stay meaningful without that color refinement.
        let settings = || ExposureSettings { min_ev: -12, max_ev: 4, scale: Scale::Linear };
        let channel_input = drip::ports::Either::First(&image);
        assert_eq!(
            histogram(settings(), (channel_input,), &context)
                .unwrap()
                .counts
                .iter()
                .flatten()
                .sum::<u32>(),
            3
        );
        assert_eq!(
            waveform(settings(), (channel_input,), &context)
                .unwrap()
                .counts
                .iter()
                .flatten()
                .sum::<u32>(),
            3
        );
    }
    let rendered = ColorImage::new(pixels, ColorMeaning::rec2020().after_rendering());
    let result = vectorscope((), (&rendered,), &context).unwrap();
    assert_eq!(result.counts.iter().flatten().sum::<u32>(), 1);
}
