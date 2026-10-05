//! The built-in nodes on synthetic data with independently known results.

use std::sync::Arc;

use drip::color::{self, D65, REC2020};
use drip::eval::Evaluator;
use drip::graph::{NodeId, Port};
use drip::node::{Evaluated, NodeKind, OutputSpec};
use drip::nodes;
use drip::project::Project;
use drip::value::{Camera, Cfa, Mosaic, PortType, Rgb, Value, View};
use drip_libraw::{BlackPattern, Raw};
use serde_json::json;

fn raw(width: usize, height: usize, data: Vec<u16>) -> Raw {
    Raw {
        width,
        height,
        data,
        cfa: [[0, 1], [3, 2]],
        black: 100,
        channel_black: [0; 4],
        pattern: BlackPattern::default(),
        maximum: 1100,
        xyz_to_cam: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        as_shot: [2.0, 1.0, 1.5, 1.0],
        metadata: Default::default(),
    }
}

#[test]
fn normalization_subtracts_every_black_component() {
    let mut r = raw(4, 2, vec![600, 600, 600, 600, 600, 600, 600, 600]);
    r.channel_black = [10, 20, 30, 40];
    r.pattern = BlackPattern { height: 1, width: 2, values: vec![5, 7] };
    let m = nodes::normalize(&r).unwrap();
    // Common black = 100 + min(10..40) + min(5, 7) = 115, so 1 ↔ 1100 - 115.
    let site = |black: u32| (600 - 100 - black) as f32 / 985.0;
    let expected =
        [site(10 + 5), site(20 + 7), site(10 + 5), site(20 + 7), site(40 + 5), site(30 + 7)];
    let close = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6);
    assert!(close(&m.data[..4], &expected[..4]), "{:?}", m.data);
    assert!(close(&m.data[4..6], &expected[4..]), "{:?}", m.data);
    assert_eq!(m.cfa, Cfa { size: 2, colors: vec![0, 1, 3, 2] });
    assert_eq!(
        m.camera.white_balance,
        [2.0, 1.0, 1.5, 1.0],
        "second green falls back to the first"
    );
}

#[test]
fn small_black_patterns_fold_into_channels_before_the_common_part() {
    // Every site's black is 200 (LibRaw's adjust_bl() example from review):
    // R gets 100 from the pattern, the others from their channel offsets.
    let mut r = raw(2, 2, vec![650; 4]);
    r.channel_black = [0, 100, 100, 100];
    r.pattern = BlackPattern { height: 2, width: 2, values: vec![100, 0, 0, 0] };
    assert_eq!(nodes::normalize(&r).unwrap().data, [0.5; 4]);
}

#[test]
fn large_black_patterns_keep_their_own_common_part() {
    // A 3 × 3 pattern is not folded: common = 100 + min(channels) + min(pattern).
    let mut r = raw(2, 2, vec![600; 4]);
    r.channel_black = [10, 20, 30, 40];
    r.pattern = BlackPattern { height: 3, width: 3, values: vec![4, 5, 6, 7, 8, 9, 10, 11, 12] };
    let m = nodes::normalize(&r).unwrap();
    let denominator = (1100 - (100 + 10 + 4)) as f32;
    assert_eq!(m.data[0], (600 - 100 - 10 - 4) as f32 / denominator);
    assert_eq!(m.data[3], (600 - 100 - 30 - 8) as f32 / denominator);
}

#[test]
fn second_green_keeps_its_own_multiplier() {
    let mut r = raw(2, 2, vec![0; 4]);
    r.as_shot = [2.0, 1.0, 1.5, 2.0];
    assert_eq!(nodes::normalize(&r).unwrap().camera.white_balance, [2.0, 1.0, 1.5, 2.0]);
}

#[test]
fn values_below_black_and_above_white_are_kept() {
    let m = nodes::normalize(&raw(2, 2, vec![50, 2100, 100, 1100])).unwrap();
    assert_eq!(m.data, [-0.05, 2.0, 0.0, 1.0]);
}

#[test]
fn binning_keeps_the_bayer_phase_and_crops_partial_cells() {
    // 9 × 4 sites at scale 2: one whole 4 × 4 binned cell per 4 columns, the
    // ninth column dropped. Each value encodes its site as row * 10 + col.
    let data = (0..4).flat_map(|r| (0..9).map(move |c| 100 + (r * 10 + c) as u16)).collect();
    let m = nodes::downsample(&nodes::normalize(&raw(9, 4, data)).unwrap());
    assert_eq!((m.width, m.height, m.scale), (4, 2, 2));
    let mean = |sites: [(u16, u16); 4]| {
        sites.iter().map(|&(r, c)| (r * 10 + c) as f32).sum::<f32>() / 4000.0
    };
    // Output site (0, 1) is green at phase (0, 1): input rows 0, 2, columns 1, 3.
    assert!((m.data[1] - mean([(0, 1), (0, 3), (2, 1), (2, 3)])).abs() < 1e-7);
    // Output site (1, 2) is red of the second cell: rows 1, 3, columns 4, 6.
    assert!((m.data[4 + 2] - mean([(1, 4), (1, 6), (3, 4), (3, 6)])).abs() < 1e-7);
}

#[test]
fn repeated_downsampling_matches_direct_averages_with_patterned_black_and_all_bayer_phases() {
    let data = (0..19 * 17).map(|i| (i * 137 % 1500) as u16).collect();
    let mut r = raw(19, 17, data);
    r.channel_black = [4, 8, 12, 16];
    r.pattern = BlackPattern { height: 3, width: 3, values: (1..=9).collect() };
    for cfa in [[[0, 1], [3, 2]], [[1, 0], [2, 3]], [[3, 2], [0, 1]], [[2, 3], [1, 0]]] {
        r.cfa = cfa;
        let mut m = nodes::normalize(&r).unwrap();
        for level in 0..5 {
            let scale = 1 << level;
            assert_eq!((m.width, m.height), (19 / (2 * scale) * 2, 17 / (2 * scale) * 2));
            assert_eq!(m.scale, scale as u32);
            assert_eq!(m.cfa.colors, cfa.concat());
            for row in 0..m.height {
                for col in 0..m.width {
                    let mut sum = 0.0f64;
                    for y in 0..scale {
                        for x in 0..scale {
                            let y = row / 2 * 2 * scale + row % 2 + y * 2;
                            let x = col / 2 * 2 * scale + col % 2 + x * 2;
                            let black = 100
                                + r.channel_black[cfa[y % 2][x % 2] as usize]
                                + r.pattern.at(y, x);
                            sum += f64::from(r.data[y * 19 + x]) - f64::from(black);
                        }
                    }
                    let expected = sum / (995.0 * (scale * scale) as f64);
                    assert!((f64::from(m.data[row * m.width + col]) - expected).abs() < 2e-7);
                }
            }
            m = nodes::downsample(&m);
        }
    }
}

#[test]
fn unusable_raw_metadata_is_an_error() {
    for as_shot in
        [[0.0; 4], [f32::NAN, 1.0, 1.0, 1.0], [1.0, 0.0, 1.0, 1.0], [f32::INFINITY, 1.0, 1.0, 1.0]]
    {
        let mut r = raw(2, 2, vec![0; 4]);
        r.as_shot = as_shot;
        assert!(nodes::normalize(&r).is_err(), "{as_shot:?}");
    }
    for matrix in [[[0.0; 3]; 3], [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]] {
        let mut r = raw(2, 2, vec![0; 4]);
        r.xyz_to_cam = matrix;
        assert!(nodes::normalize(&r).is_err(), "{matrix:?}");
    }
    let mut r = raw(2, 2, vec![0; 4]);
    r.maximum = 100;
    assert!(nodes::normalize(&r).is_err());
}

#[test]
fn rec2020_matrix_matches_the_standard() {
    // Published RGB-to-XYZ matrix for BT.2020 primaries and D65.
    let expected =
        [[0.636958, 0.144617, 0.168881], [0.262700, 0.677998, 0.059302], [0.0, 0.028073, 1.060985]];
    let m = color::rgb_to_xyz(REC2020, D65);
    for (row, expected) in m.iter().zip(expected) {
        for (v, e) in row.iter().zip(expected) {
            assert!((v - e).abs() < 1e-6, "{m:?}");
        }
    }
}

#[test]
fn camera_matrix_is_neutral_preserving_and_ignores_channel_gains() {
    let to_xyz = color::rgb_to_xyz(REC2020, D65);
    // A camera that is Rec.2020 up to per-channel sensitivities.
    let gains = [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.5]];
    let xyz_to_cam = color::mul(&gains, &color::inverse(&to_xyz));
    let m = color::camera_to_rgb(&xyz_to_cam, &to_xyz).unwrap();
    for (i, row) in m.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            assert!((v - f64::from(i == j)).abs() < 1e-9, "{m:?}");
        }
    }
}

/// A source node emitting a fixed 4 × 2 RGGB mosaic.
static MOSAIC: NodeKind = NodeKind {
    name: "test.mosaic",
    label: "mosaic",
    params: &[],
    inputs: &[],
    outputs: &[OutputSpec { name: "mosaic", ty: PortType::Mosaic }],
    eval: |_, _, _| {
        let camera =
            Arc::new(Camera { xyz_to_cam: [[0.0; 3]; 3], white_balance: [2.0, 1.0, 4.0, 3.0] });
        let cfa = Cfa { size: 2, colors: vec![0, 1, 3, 2] };
        let data = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let mosaic = Mosaic { width: 4, height: 2, scale: 1, cfa, data, camera, white: [1.0; 4] };
        Ok(Evaluated { outputs: vec![Value::Mosaic(Arc::new(mosaic))], view: None })
    },
    actions: &[],
};

/// A source node emitting the scene-referred pixels given as `pixels`.
static SCENE: NodeKind = NodeKind {
    name: "test.scene",
    label: "scene",
    params: &[],
    inputs: &[],
    outputs: &[OutputSpec { name: "image", ty: PortType::SceneRec2020 }],
    eval: |_, _, _| {
        let pixels = vec![[0.18, 0.0, -1.0], [0.09, 1e6, 0.36]];
        Ok(Evaluated {
            outputs: vec![Value::SceneRec2020(Arc::new(Rgb {
                width: 2,
                height: 1,
                scale: 1,
                pixels,
            }))],
            view: None,
        })
    },
    actions: &[],
};

/// Builds source → kinds… and returns the project and the last node.
fn chain(source: &'static NodeKind, kinds: &[&'static NodeKind]) -> (Project, NodeId) {
    let mut p = Project::default();
    let mut last = p.graph.add_node(source);
    for kind in kinds {
        let id = p.graph.add_node(kind);
        let output = p.graph.node(last).unwrap().kind.outputs[0].name;
        p.graph.connect(Port(last, output.into()), Port(id, kind.inputs[0].name.into())).unwrap();
        last = id;
    }
    (p, last)
}

fn evaluate(p: &Project, id: NodeId) -> Evaluated {
    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, 0, &[id]);
    ev.result(id).unwrap().clone().unwrap()
}

#[test]
fn white_balance_scales_each_site_by_its_color() {
    let (p, wb) = chain(&MOSAIC, &[&nodes::WHITE_BALANCE]);
    let out = evaluate(&p, wb);
    // Rows alternate R G R G / G2 B G2 B.
    let expected = [0.1 * 2.0, 0.2, 0.3 * 2.0, 0.4, 0.5 * 3.0, 0.6 * 4.0, 0.7 * 3.0, 0.8 * 4.0];
    assert_eq!(out.outputs[0].mosaic().data, expected);
}

#[test]
fn binning_debayer_averages_greens_and_halves_resolution() {
    let (p, bin) = chain(&MOSAIC, &[&nodes::BIN_2X2]);
    let out = evaluate(&p, bin);
    let image = out.outputs[0].rgb();
    assert_eq!((image.width, image.height, image.scale), (2, 1, 2));
    // Cells: [0.1 0.2 / 0.5 0.6] and [0.3 0.4 / 0.7 0.8], RGGB.
    assert_eq!(image.pixels, [[0.1, (0.2 + 0.5) / 2.0, 0.6], [0.3, (0.4 + 0.7) / 2.0, 0.8]]);
}

#[test]
fn exposure_is_scene_linear_and_sigmoid_outputs_finite_display_values() {
    let (mut p, exposure) = chain(&SCENE, &[&nodes::EXPOSURE]);
    p.graph.set_param(exposure, "ev", json!(1.0)).unwrap();
    let rgb = evaluate(&p, exposure).outputs[0].rgb().clone();
    assert_eq!(rgb.pixels, [[0.36, 0.0, -2.0], [0.18, 2e6, 0.72]]);
    let sigmoid = p.graph.add_node(&nodes::SIGMOID);
    p.graph.connect(Port(exposure, "image".into()), Port(sigmoid, "image".into())).unwrap();
    let out = evaluate(&p, sigmoid);
    assert_eq!(out.outputs[0].port_type(), PortType::DisplayRec2020);
    assert!(out.outputs[0].rgb().pixels.iter().flatten().all(|v| v.is_finite()));
}

#[test]
fn histogram_bins_by_stops() {
    let (mut p, id) = chain(&SCENE, &[&nodes::HISTOGRAM]);
    let Some(View::Histogram(h)) = evaluate(&p, id).view else { panic!("no histogram") };
    let bin = |v: f32| ((v.log2() - h.min_stop) / (h.max_stop - h.min_stop) * 256.0) as usize;
    assert_eq!(h.counts.iter().map(|c| c[0] + c[1] + c[2]).sum::<u32>(), 6);
    assert_eq!(h.counts[0], [0, 1, 1], "zero and negative values");
    assert_eq!(h.counts[255], [0, 1, 0], "values beyond the range");
    assert_eq!(h.counts[bin(0.18)][0], 1);
    assert_eq!(h.counts[bin(0.36)][2], 1);
    assert!(h.log, "log scale by default");

    p.graph.set_param(id, "min_ev", json!(-2)).unwrap();
    p.graph.set_param(id, "max_ev", json!(1)).unwrap();
    p.graph.set_param(id, "scale", json!("linear")).unwrap();
    let Some(View::Histogram(h)) = evaluate(&p, id).view else { panic!("no histogram") };
    assert_eq!((h.min_stop, h.max_stop, h.log), (-2.0, 1.0, false));
    assert_eq!(h.counts[0], [2, 1, 1], "0.18 and 0.09 now lie below the range");
    assert_eq!(h.counts[((0.36f32.log2() + 2.0) / 3.0 * 256.0) as usize][2], 1);
}

#[test]
fn preview_presents_its_input() {
    let (p, v) = chain(&SCENE, &[&nodes::PREVIEW]);
    let out = evaluate(&p, v);
    assert!(out.outputs.is_empty());
    assert!(matches!(out.view, Some(View::Image(Value::SceneRec2020(_)))));
}

#[test]
fn highlights_reconstruct_before_preview_averaging() {
    static SOURCE: NodeKind = NodeKind {
        name: "test.clipped",
        label: "clipped",
        params: &[],
        inputs: &[],
        outputs: &[OutputSpec { name: "mosaic", ty: PortType::Mosaic }],
        actions: &[],
        eval: |_, _, _| {
            let cfa = Cfa { size: 2, colors: vec![0, 1, 3, 2] };
            let mut data: Vec<_> =
                (0..64).map(|i| if cfa.color(i / 8, i % 8) == 0 { 0.2 } else { 2.0 }).collect();
            data[0] = 1.0;
            let camera = Arc::new(Camera { xyz_to_cam: [[0.0; 3]; 3], white_balance: [1.0; 4] });
            Ok(Evaluated {
                outputs: vec![Value::Mosaic(Arc::new(Mosaic {
                    width: 8,
                    height: 8,
                    scale: 1,
                    cfa,
                    data,
                    camera,
                    white: [1.0, 4.0, 4.0, 4.0],
                }))],
                view: None,
            })
        },
    };
    let (mut p, highlights) = chain(&SOURCE, &[&nodes::HIGHLIGHTS]);
    let source = p.graph.find("clipped").unwrap();
    let repaired = p.graph.add_node(&nodes::RCD);
    let bypass = p.graph.add_node(&nodes::RCD);
    p.graph.connect(Port(highlights, "mosaic".into()), Port(repaired, "mosaic".into())).unwrap();
    p.graph.connect(Port(source, "mosaic".into()), Port(bypass, "mosaic".into())).unwrap();
    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, 1, &[repaired, bypass]);
    let result = |id| &ev.result(id).unwrap().as_ref().unwrap().outputs[0];
    assert_eq!(result(highlights).mosaic().scale, 1);
    assert_eq!(result(highlights).mosaic().width, 8);
    assert_eq!(result(repaired).rgb().scale, 2);
    assert_eq!(result(repaired).rgb().width, 4);
    assert!((result(repaired).rgb().pixels[0][0] - 0.65).abs() < 1e-6);
    assert!((result(bypass).rgb().pixels[0][0] - 0.4).abs() < 1e-6);
}

#[test]
fn white_balance_scales_saturation_with_each_channel() {
    let (p, wb) = chain(&MOSAIC, &[&nodes::WHITE_BALANCE]);
    assert_eq!(evaluate(&p, wb).outputs[0].mosaic().white, [2.0, 1.0, 4.0, 3.0]);
    let mut r = raw(4, 4, vec![600; 16]);
    r.channel_black = [10, 20, 30, 40];
    r.pattern = BlackPattern { height: 3, width: 3, values: vec![1, 2, 3, 4, 5, 6, 7, 8, 9] };
    let m = nodes::normalize(&r).unwrap();
    for row in 0..4 {
        for col in 0..4 {
            let c = r.cfa[row % 2][col % 2] as usize;
            let saturation =
                (r.maximum - r.black - r.channel_black[c] - r.pattern.at(row, col)) as f32 / 989.0;
            assert!(m.white[c] <= saturation + 1e-6);
        }
    }
}
