use super::{density::vectorscope, histogram::histogram};
use std::sync::{Arc, LazyLock};

use bevy_reflect::Reflect;
use drip::color::{self, D65, P3};
use drip::graph::Graph;
use drip::image::{
    Colorimetry, LinearRgb, LinearRgbColorSpace, Linearity, RealMat, Rec2020Mat, Rgb,
};
use drip::node::EvalContext;
use drip::nodes::scopes::{ExposureSettings, HISTOGRAM};
use drip::param::{Parameters, Params};
use drip::ports::ReadMat;
use drip::value::Value;
use serde_json::json;

use super::ScopeAxes;

#[test]
fn histogram_bins_by_stops() {
    let value = Value::new(Arc::new(Rec2020Mat::from(Arc::new(Rgb {
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
        let image = value.borrow::<ReadMat<3, dyn Linearity>>().unwrap();
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

#[drip::interpretation(LinearRgb)]
#[derive(Debug, Default, Reflect)]
struct P3Interpretation;
impl Linearity for P3Interpretation {}
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

#[test]
fn vectorscope_uses_the_connected_color_space_witness() {
    let source = RealMat::<3, P3Interpretation>::from(Arc::new(Rgb {
        width: 1,
        height: 1,
        scale: 1,
        pixels: vec![[1.0, 0.0, 0.0]],
    }));
    let value = Value::new(Arc::new(source));
    let image = value.borrow::<ReadMat<3, dyn LinearRgb>>().unwrap();
    let resources = Default::default();
    let context = EvalContext::new(0, &resources).unwrap();
    let scope = vectorscope((), (image,), &context).unwrap();
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
