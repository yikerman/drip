//! The built-in nodes on synthetic data with independently known results.

use std::sync::Arc;

use drip::color::{self, D65, REC2020};
use drip::eval::Evaluator;
use drip::graph::{NodeId, Port};
use drip::node::{Evaluated, NodeKind, OutputSpec, Registry};
use drip::nodes;
use drip::project::Project;
use drip::value::{Camera, Cfa, Mosaic, PortType, Value};
use drip_libraw::{BlackPattern, Raw};

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
    let m = nodes::normalize(&r, 1).unwrap();
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
    assert_eq!(nodes::normalize(&r, 1).unwrap().data, [0.5; 4]);
}

#[test]
fn large_black_patterns_keep_their_own_common_part() {
    // A 3 × 3 pattern is not folded: common = 100 + min(channels) + min(pattern).
    let mut r = raw(2, 2, vec![600; 4]);
    r.channel_black = [10, 20, 30, 40];
    r.pattern = BlackPattern { height: 3, width: 3, values: vec![4, 5, 6, 7, 8, 9, 10, 11, 12] };
    let m = nodes::normalize(&r, 1).unwrap();
    let denominator = (1100 - (100 + 10 + 4)) as f32;
    assert_eq!(m.data[0], (600 - 100 - 10 - 4) as f32 / denominator);
    assert_eq!(m.data[3], (600 - 100 - 30 - 8) as f32 / denominator);
}

#[test]
fn second_green_keeps_its_own_multiplier() {
    let mut r = raw(2, 2, vec![0; 4]);
    r.as_shot = [2.0, 1.0, 1.5, 2.0];
    assert_eq!(nodes::normalize(&r, 1).unwrap().camera.white_balance, [2.0, 1.0, 1.5, 2.0]);
}

#[test]
fn values_below_black_and_above_white_are_kept() {
    let m = nodes::normalize(&raw(2, 2, vec![50, 2100, 100, 1100]), 1).unwrap();
    assert_eq!(m.data, [-0.05, 2.0, 0.0, 1.0]);
}

#[test]
fn binning_keeps_the_bayer_phase_and_crops_partial_cells() {
    // 9 × 4 sites at scale 2: one whole 4 × 4 binned cell per 4 columns, the
    // ninth column dropped. Each value encodes its site as row * 10 + col.
    let data = (0..4).flat_map(|r| (0..9).map(move |c| 100 + (r * 10 + c) as u16)).collect();
    let m = nodes::normalize(&raw(9, 4, data), 2).unwrap();
    assert_eq!((m.width, m.height, m.scale), (4, 2, 2));
    let mean = |sites: [(u16, u16); 4]| {
        sites.iter().map(|&(r, c)| (r * 10 + c) as f32).sum::<f32>() / 4000.0
    };
    // Output site (0, 1) is green at phase (0, 1): input rows 0, 2, columns 1, 3.
    assert_eq!(m.data[1], mean([(0, 1), (0, 3), (2, 1), (2, 3)]));
    // Output site (1, 2) is red of the second cell: rows 1, 3, columns 4, 6.
    assert_eq!(m.data[4 + 2], mean([(1, 4), (1, 6), (3, 4), (3, 6)]));
}

#[test]
fn unusable_raw_metadata_is_an_error() {
    for as_shot in
        [[0.0; 4], [f32::NAN, 1.0, 1.0, 1.0], [1.0, 0.0, 1.0, 1.0], [f32::INFINITY, 1.0, 1.0, 1.0]]
    {
        let mut r = raw(2, 2, vec![0; 4]);
        r.as_shot = as_shot;
        assert!(nodes::normalize(&r, 1).is_err(), "{as_shot:?}");
    }
    for matrix in [[[0.0; 3]; 3], [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]] {
        let mut r = raw(2, 2, vec![0; 4]);
        r.xyz_to_cam = matrix;
        assert!(nodes::normalize(&r, 1).is_err(), "{matrix:?}");
    }
    let mut r = raw(2, 2, vec![0; 4]);
    r.maximum = 100;
    assert!(nodes::normalize(&r, 1).is_err());
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
    version: 1,
    params: &[],
    inputs: &[],
    outputs: &[OutputSpec { name: "mosaic", ty: PortType::Mosaic }],
    eval: |_, _, _| {
        let camera =
            Arc::new(Camera { xyz_to_cam: [[0.0; 3]; 3], white_balance: [2.0, 1.0, 4.0, 3.0] });
        let cfa = Cfa { size: 2, colors: vec![0, 1, 3, 2] };
        let data = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let mosaic = Mosaic { width: 4, height: 2, scale: 1, cfa, data, camera };
        Ok(Evaluated { outputs: vec![Value::Mosaic(Arc::new(mosaic))], view: None })
    },
    actions: &[],
    migrate: None,
};

fn registry() -> Registry {
    nodes::registry().with(&MOSAIC)
}

/// Builds source → kinds… and returns the project and the last node.
fn chain(source: &'static NodeKind, kinds: &[&'static NodeKind]) -> (Project, NodeId) {
    let reg = registry();
    let mut p = Project::default();
    let mut last = p.graph.add_node(source);
    for kind in kinds {
        let id = p.graph.add_node(kind);
        let output = reg.get(&p.graph.node(last).unwrap().kind).unwrap().outputs[0].name;
        p.graph
            .connect(&reg, Port(last, output.into()), Port(id, kind.inputs[0].name.into()))
            .unwrap();
        last = id;
    }
    (p, last)
}

fn evaluate(p: &Project, id: NodeId) -> Evaluated {
    let mut ev = Evaluator::default();
    ev.evaluate(p, &registry(), 0, &[id]);
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
