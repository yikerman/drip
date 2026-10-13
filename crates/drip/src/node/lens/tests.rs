use super::*;
use crate::body::Projective;
use drip_raw::{DngVignette, DngWarp};

const SONY: [f64; 2] = [7968.0, 5320.0];

fn fixture(name: &str) -> Correction {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw").join(name);
    drip_raw::decode(&path).unwrap().metadata.lens.correction.expect("embedded correction")
}

fn embedded(tune: [f64; 4]) -> Settings {
    let [distortion, chromatic_red, chromatic_blue, vignetting] = tune;
    let source =
        Source::Embedded(Embedded { distortion, chromatic_red, chromatic_blue, vignetting });
    Settings { source, ..Default::default() }
}

fn keep(p: Settings) -> Settings {
    Settings { fit: Fit::Keep, ..p }
}

/// The radial part and the zoom of the node's warp.
fn corrected(p: &Settings, size: [f64; 2], correction: Option<&Correction>) -> (Radial, f64) {
    let warp = warp(p, size, correction).unwrap();
    (warp.radial.expect("a radial warp"), 1.0 / warp.maps[0][0][0])
}

fn identity() -> Warp {
    Warp::reindex(matrix::IDENTITY)
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn nothing_to_correct_is_an_identity_reindex() {
    let sony = fixture("sony-ilce-7rm3.arw");
    let p = Settings::default();
    assert_eq!(warp(&p, SONY, None).unwrap(), identity());
    let off = Settings { distortion: false, chromatic_aberration: false, vignetting: false, ..p };
    assert_eq!(warp(&off, SONY, Some(&sony)).unwrap(), identity());
    assert_eq!(warp(&embedded([0.0; 4]), SONY, Some(&sony)).unwrap(), identity());
    let manual = Settings { source: Source::Manual(Default::default()), ..Default::default() };
    assert_eq!(warp(&manual, SONY, Some(&sony)).unwrap(), identity());
}

#[test]
fn sony_tables_follow_darktable() {
    let correction = fixture("sony-ilce-7rm3.arw");
    let Correction::Sony { distortion, ca_red, ca_blue, vignetting } = &correction else {
        panic!("Sony tables")
    };
    let (radial, _) = corrected(&keep(Settings::default()), SONY, Some(&correction));
    assert_eq!(radial.centre, [3984.0, 2660.0]);
    assert_eq!(radial.radius, 3984f64.hypot(2660.0));
    let n = distortion.len();
    for i in 0..n {
        let knot = (i as f64 + 0.5) / (n - 1) as f64;
        let green = 1.0 + f64::from(distortion[i]) * 2f64.powi(-14);
        let fringe = |ca: i16| green * (1.0 + f64::from(ca) * 2f64.powi(-21));
        let vig = 2f64.powf(0.5 - 2f64.powf(f64::from(vignetting[i]) * 2f64.powi(-13) - 1.0));
        let at = |c: usize| radial.ratios[c].at(knot);
        assert!(close(at(1), green, 1e-15), "{i}");
        assert!(close(at(0), fringe(ca_red[i]), 1e-15), "{i}");
        assert!(close(at(2), fringe(ca_blue[i]), 1e-15), "{i}");
        assert!(close(radial.gain.as_ref().unwrap().at(knot), 1.0 / vig, 1e-15), "{i}");
    }
    // A fine-tune of zero leaves out that part alone.
    let ones = |s: &Spline| s.values().iter().all(|&v| v == 1.0);
    let parts = |tune| corrected(&keep(embedded(tune)), SONY, Some(&correction)).0;
    let without = parts([0.0, 1.0, 1.0, 1.0]);
    assert!(ones(&without.ratios[1]) && !ones(&without.ratios[0]) && without.gain.is_some());
    let without = parts([1.0, 0.0, 0.0, 1.0]);
    assert_eq!(without.ratios[0], without.ratios[1]);
    assert_eq!(without.ratios[2], without.ratios[1]);
    assert!(parts([1.0, 1.0, 1.0, 0.0]).gain.is_none());
}

/// The embedded table and Lensfun's profile of the fixture lens at 28 mm
/// (`mil-tamron.xml`, ptlens) describe the same distortion: the corner's
/// ratio over the centre's agrees within 0.2 %.
#[test]
fn sony_distortion_agrees_with_lensfun() {
    let correction = fixture("sony-ilce-7rm3.arw");
    let (sony, _) = corrected(&keep(Settings::default()), SONY, Some(&correction));
    let profile = Manual { a: 0.00611142, b: -0.0180555, c: 0.00453851, ..Default::default() };
    let p = keep(Settings { source: Source::Manual(profile), ..Default::default() });
    let (lensfun, _) = corrected(&p, SONY, None);
    let falloff = |r: &Radial| r.ratios[1].at(1.0) / r.ratios[1].at(0.0);
    let (sony, lensfun) = (falloff(&sony), falloff(&lensfun));
    assert!(close(sony / lensfun, 1.0, 2e-3), "{sony} {lensfun}");
}

/// darktable's `_init_coeffs_md_v2` for Fujifilm, before its autoscale, at
/// its 16 knots: destination radii and the red, green and blue ratios.
fn darktable_fuji(knots: &[f64], tables: &[Vec<f64>; 4]) -> Vec<(f64, [f64; 3])> {
    let lead = knots[0] > 0.0;
    let column = |neutral: f64, values: Vec<f64>| {
        if lead { [vec![neutral], values].concat() } else { values }
    };
    let knots = column(0.0, knots.to_vec());
    let m = column(1.0, tables[0].iter().map(|d| d / 100.0 + 1.0).collect());
    let [red, blue] = [&tables[1], &tables[2]].map(|t| column(0.0, t.clone()));
    (0..16)
        .map(|i| {
            let r = i as f64 / 15.0;
            let m = embedded_interpolate(&knots, &m, r);
            let fringe = |t: &[f64]| m * (embedded_interpolate(&knots, t, r) + 1.0);
            (r / m, [fringe(&red), m, fringe(&blue)])
        })
        .collect()
}

fn embedded_interpolate(knots: &[f64], values: &[f64], x: f64) -> f64 {
    if x < knots[0] {
        return values[0];
    }
    (1..knots.len())
        .find(|&i| x >= knots[i - 1] && x <= knots[i])
        .map(|i| {
            let slope = (values[i] - values[i - 1]) / (knots[i] - knots[i - 1]);
            values[i - 1] + (x - knots[i - 1]) * slope
        })
        .unwrap_or(values[values.len() - 1])
}

fn fuji_tables(c: &Correction) -> (Vec<f64>, [Vec<f64>; 4], Option<u16>) {
    let Correction::Fuji { knots, distortion, ca_red, ca_blue, vignetting, crop_mode } = c else {
        panic!("Fujifilm tables")
    };
    let f64s = |v: &[f32]| v.iter().map(|&v| f64::from(v)).collect::<Vec<_>>();
    (f64s(knots), [distortion, ca_red, ca_blue, vignetting].map(|t| f64s(t)), *crop_mode)
}

fn check_fuji(correction: &Correction, size: [f64; 2]) {
    let (knots, tables, crop_mode) = fuji_tables(correction);
    let crop = if matches!(crop_mode, Some(2 | 4)) { 1.25 } else { 1.0 };
    let knots: Vec<_> = knots.iter().map(|k| k * crop).collect();
    let (radial, _) = corrected(&keep(Settings::default()), size, Some(correction));
    for (r, expected) in darktable_fuji(&knots, &tables) {
        for (c, expected) in expected.into_iter().enumerate() {
            let actual = radial.ratios[c].at(r);
            assert!(close(actual, expected, 1e-6), "{r} {c}: {actual} {expected}");
        }
    }
    // Uniform knots cut the table's corners by up to its change in slope
    // times half a step.
    for (knot, v) in knots.iter().zip(&tables[3]) {
        let gain = radial.gain.as_ref().unwrap().at(*knot);
        assert!(close(gain * v / 100.0, 1.0, 3e-3), "{knot}: {gain} {v}");
    }
}

#[test]
fn fujifilm_tables_follow_darktable() {
    check_fuji(&fixture("pixls/fujifilm-gfx50s-ii.raf"), [8256.0, 6192.0]);
    // The X-Trans I–III layout starts at radius 0, with a stronger table
    // than any fixture, under a 1.25 crop.
    let knots: Vec<f32> = (0..11).map(|i| i as f32 / 10.0).collect();
    let distortion: Vec<f32> = (0..11).map(|i| -0.3 * (i * i) as f32).collect();
    let ca_red: Vec<f32> = (0..11).map(|i| 2e-4 * i as f32).collect();
    let ca_blue: Vec<f32> = (0..11).map(|i| -1e-4 * i as f32).collect();
    let vignetting: Vec<f32> = (0..11).map(|i| 100.0 - 3.0 * i as f32).collect();
    let correction =
        Correction::Fuji { knots, distortion, ca_red, ca_blue, vignetting, crop_mode: Some(2) };
    check_fuji(&correction, [6000.0, 4000.0]);
}

#[test]
fn dng_warps_about_their_own_centre() {
    let correction = fixture("pixls/google-pixel-4-xl.dng");
    let Correction::Dng { warp: Some(DngWarp { planes, centre }), vignette: None } = &correction
    else {
        panic!("a DNG warp")
    };
    let size = [4032.0, 3024.0];
    let (radial, _) = corrected(&keep(Settings::default()), size, Some(&correction));
    let pixels = [centre[0] * size[0], centre[1] * size[1]];
    assert_eq!(radial.centre, pixels);
    let corners = [[0.0, 0.0], [size[0], 0.0], [0.0, size[1]], size];
    let far =
        corners.iter().map(|c| (c[0] - pixels[0]).hypot(c[1] - pixels[1])).fold(0.0, f64::max);
    assert!(close(radial.radius, far, 1e-9));
    let k = planes[0];
    for knot in [0.0, 0.3, 0.7, 1.0] {
        let r2: f64 = knot * knot;
        let expected = k[0] + k[1] * r2 + k[2] * r2 * r2 + k[3] * r2 * r2 * r2;
        for c in 0..3 {
            assert!(close(radial.ratios[c].at(knot), expected, 1e-6), "{knot}");
        }
    }
    assert!(radial.gain.is_none());

    // Three planes and a vignette at the frame centre equal darktable's.
    let planes = vec![
        [1.0, 0.01, -0.005, 0.001, 0.0, 0.0],
        [0.99, 0.02, -0.01, 0.002, 0.0, 0.0],
        [0.98, 0.03, -0.01, 0.001, 0.0, 0.0],
    ];
    let k = [0.4, -0.1, 0.05, 0.0, 0.01];
    let correction = Correction::Dng {
        warp: Some(DngWarp { planes: planes.clone(), centre: [0.5; 2] }),
        vignette: Some(DngVignette { k, centre: [0.5; 2] }),
    };
    let (radial, _) = corrected(&keep(Settings::default()), size, Some(&correction));
    assert_eq!(radial.radius, 2016f64.hypot(1512.0));
    for i in 0..16 {
        let r = i as f64 / 15.0;
        let r2 = r * r;
        for (c, p) in planes.iter().enumerate() {
            let expected = p[0] + p[1] * r2 + p[2] * r2.powi(2) + p[3] * r2.powi(3);
            assert!(close(radial.ratios[c].at(r), expected, 1e-6), "{r} {c}");
        }
        let dvig: f64 = (0..5).map(|j| k[j] * r2.powi(j as i32 + 1)).sum();
        assert!(close(radial.gain.as_ref().unwrap().at(r), 1.0 + dvig, 1e-5), "{r}");
    }
}

#[test]
fn olympus_polynomials_follow_darktable() {
    let correction = fixture("pixls/om-system-tg-7.orf");
    let Correction::Olympus { distortion: Some(d), ca: Some(ca) } = &correction else {
        panic!("Olympus distortion and CA")
    };
    let [d, ca] =
        [d.as_slice(), ca.as_slice()].map(|v| v.iter().map(|&v| f64::from(v)).collect::<Vec<_>>());
    let (radial, _) = corrected(&keep(Settings::default()), [4000.0, 3000.0], Some(&correction));
    for i in 1..16 {
        let r = i as f64 / 15.0;
        let rs2 = (r * d[3]).powi(2);
        let green = d[3] * (1.0 + rs2 * (d[0] + rs2 * (d[1] + rs2 * d[2])));
        let rd = green * r;
        let rd2 = rd * rd;
        let red = green + rd * (ca[0] + rd2 * (ca[1] + rd2 * ca[2])) / r;
        let blue = green + rd * (ca[3] + rd2 * (ca[4] + rd2 * ca[5])) / r;
        for (c, expected) in [red, green, blue].into_iter().enumerate() {
            assert!(close(radial.ratios[c].at(r), expected, 1e-6), "{r} {c}");
        }
    }
    assert!(radial.gain.is_none());
}

#[test]
fn panasonic_inverts_its_polynomial() {
    let correction = fixture("pixls/panasonic-dmc-tz70.rw2");
    let Correction::Panasonic { a, b, c, scale } = correction else { panic!("Panasonic words") };
    let [a, b, c] = [a, b, c].map(|w| f64::from(w) / 32768.0);
    let scale = 1.0 / (1.0 + f64::from(scale) / 32768.0);
    let f = |d: f64| 1.0 + scale * (a * d * d + b * d.powi(4) + c * d.powi(6));
    let (radial, _) = corrected(&keep(Settings::default()), [4896.0, 3264.0], Some(&correction));
    for i in 1..16 {
        let r = i as f64 / 15.0;
        // The source radius lands back on r.
        let d = radial.ratios[1].at(r) * r;
        assert!(close(d * f(d), r, 1e-5), "{r}");
        // darktable's two iterations fall short by up to 0.53 % of r here.
        let twice = (0..2).fold(r, |d, _| r / f(d));
        assert!(close(d / twice, 1.0, 6e-3), "{r}: {d} {twice}");
    }
}

#[test]
fn manual_models_follow_lensfun() {
    let profile = Manual {
        a: 0.01,
        b: -0.03,
        c: 0.005,
        red: 1.0004,
        blue: 0.9993,
        k1: -0.4,
        k2: 0.1,
        k3: -0.05,
    };
    let size = [6000.0, 4000.0];
    let p = keep(Settings { source: Source::Manual(profile.clone()), ..Default::default() });
    let (radial, zoom) = corrected(&p, size, None);
    assert_eq!(zoom, 1.0);
    let Manual { a, b, c, red, blue, k1, k2, k3 } = profile;
    let (half_short, half_diagonal) = (2000.0, 3000f64.hypot(2000.0));
    for r_u in [0.1, 0.5, 1.0, 1.4] {
        // A point at Hugin radius r_u on the diagonal.
        let t = r_u * half_short;
        let q = [3000.0 + t * 3000.0 / half_diagonal, 2000.0 + t * 2000.0 / half_diagonal];
        let r_d = r_u * (a * r_u.powi(3) + b * r_u.powi(2) + c * r_u + 1.0 - a - b - c);
        for (channel, k) in [(0, red), (1, 1.0), (2, blue)] {
            let source = radial.source(channel, q);
            let radius = (source[0] - 3000.0).hypot(source[1] - 2000.0) / half_short;
            assert!(close(radius, k * r_d, 1e-6), "{r_u} {channel}");
            // pa vignetting at the source, with r = 1 at the corner.
            let r = radius * half_short / half_diagonal;
            let expected = 1.0 / (1.0 + k1 * r.powi(2) + k2 * r.powi(4) + k3 * r.powi(6));
            assert!(close(radial.gain(channel, q), expected, 1e-5), "{r_u} {channel}");
        }
    }
}

/// 256 knots hold the fixture lens's Lensfun profile within 0.01 px of
/// its polynomial across a 7968 × 5320 frame.
#[test]
fn sampled_models_err_by_under_a_hundredth_of_a_pixel() {
    let profile = Manual { a: 0.00611142, b: -0.0180555, c: 0.00453851, ..Default::default() };
    let p = keep(Settings { source: Source::Manual(profile.clone()), ..Default::default() });
    let (radial, _) = corrected(&p, SONY, None);
    let hugin = radial.radius / 2660.0;
    let Manual { a, b, c, .. } = profile;
    let worst = (0..=10_000)
        .map(|i| {
            let rho = i as f64 / 10_000.0;
            let r = hugin * rho;
            let exact = a * r.powi(3) + b * r.powi(2) + c * r + 1.0 - a - b - c;
            (radial.ratios[1].at(rho) - exact).abs() * rho * radial.radius
        })
        .fold(0.0, f64::max);
    assert!(worst < 0.01, "{worst}");
}

/// Whether the map sends every channel of the output plane's boundary inside
/// the input, within `slack` pixels.
fn inside(radial: &Radial, map: &Projective, [w, h]: [f64; 2], slack: f64) -> bool {
    let steps = 400;
    let boundary = (0..=steps).flat_map(|i| {
        let t = i as f64 / steps as f64;
        [[t * w, 0.0], [t * w, h], [0.0, t * h], [w, t * h]]
    });
    boundary.into_iter().all(|p| {
        let q = matrix::project(map, p).unwrap();
        (0..3).all(|c| {
            let [x, y] = radial.source(c, q);
            (-slack..=w + slack).contains(&x) && (-slack..=h + slack).contains(&y)
        })
    })
}

#[test]
fn fill_is_the_least_zoom_that_stays_inside() {
    let size = [6000.0, 4000.0];
    // Lensfun's ptlens keeps radius 1, half the shorter side, in place, so
    // its barrel distortion never zooms out. A camera table can.
    let barrel = Correction::Sony {
        distortion: vec![0, -100, -300, -600],
        ca_red: vec![0, 64, 128, 256],
        ca_blue: vec![0; 4],
        vignetting: vec![0; 4],
    };
    let pincushion = Manual { b: 0.02, blue: 0.9995, ..Default::default() };
    let pincushion = Settings { source: Source::Manual(pincushion), ..Default::default() };
    for (p, correction, zoomed_out) in
        [(Settings::default(), Some(&barrel), true), (pincushion, None, false)]
    {
        let warp = warp(&p, size, correction).unwrap();
        let radial = warp.radial.unwrap();
        let zoom = 1.0 / warp.maps[0][0][0];
        assert_eq!(zoom < 1.0, zoomed_out, "{zoom}");
        assert!(inside(&radial, &warp.maps[0], size, 1e-6), "{zoom}");
        let [cx, cy] = radial.centre;
        let k = 1.0 / (0.999 * zoom);
        let smaller = [[k, 0.0, cx * (1.0 - k)], [0.0, k, cy * (1.0 - k)], [0.0, 0.0, 1.0]];
        assert!(!inside(&radial, &smaller, size, 0.0), "{zoom}");

        assert_eq!(super::warp(&keep(p), size, correction).unwrap().maps[0], matrix::IDENTITY);
    }
}

#[cfg(feature = "cpu")]
#[test]
fn cpu_graph() {
    graph(crate::runtime::RuntimeContext::cpu());
}

#[test]
#[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
fn selected_backend_graph() {
    graph(crate::runtime::RuntimeContext::from_env().unwrap());
}

/// Without metadata the node launches nothing and returns its input. With
/// a Sony table it resamples once, and the vignetting gain brightens the
/// corners of a flat image.
fn graph(runtime: crate::runtime::RuntimeContext) {
    use crate::{eval::Evaluator, graph::Dag, node::source};
    use std::sync::Arc;
    let evaluator = Evaluator::new(runtime);
    let extent = Extent { width: 96, height: 64 };
    let desc = RgbDesc::new(extent.clone(), Coordinates::Space(REC2020_D65));
    let samples = vec![[0.5f32; 3]; extent.pixels()];
    let run = |metadata: Option<drip_raw::Metadata>| {
        let mut dag = Dag::new();
        let input = source::<Rgb>(&mut dag, desc.clone(), samples.clone()).unwrap();
        let node = add(&mut dag, Default::default()).unwrap();
        dag.connect(input, node.image).unwrap();
        if let Some(metadata) = metadata.map(Arc::new) {
            let metadata = source::<CaptureMetadata>(&mut dag, metadata.clone(), metadata).unwrap();
            dag.connect(metadata, node.metadata).unwrap();
        }
        evaluator.evaluate::<Rgb>(&dag, &Default::default(), node.output).unwrap()
    };
    let unchanged = run(None);
    assert_eq!(unchanged.statistics.fused, 0);
    assert_eq!(&**unchanged.data.host().unwrap(), samples.as_slice());

    let mut metadata = drip_raw::Metadata::default();
    metadata.lens.correction = Some(Correction::Sony {
        distortion: vec![0, -100, -200],
        ca_red: vec![0, 512, 1024],
        ca_blue: vec![0, -512, -1024],
        vignetting: vec![0, 2000, 6000],
    });
    let corrected = run(Some(metadata));
    assert_eq!(corrected.statistics.fused, 1);
    let pixels = corrected.data.host().unwrap();
    assert!(pixels.iter().flatten().all(|v| v.is_finite()));
    let (centre, corner) = (pixels[32 * 96 + 48][1], pixels[0][1]);
    assert!(corner > centre * 1.1, "{corner} {centre}");
}
