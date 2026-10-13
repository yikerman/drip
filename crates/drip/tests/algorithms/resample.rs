//! Resample and warps against an independent f64 resampler.
use drip::{
    body::{
        Radial, Step, Warp,
        matrix::{self, Mat3},
    },
    eval::Evaluator,
    graph::Dag,
    image::{Description, Image, Raster},
    node::{
        self,
        data::*,
        filter::Filter,
        perspective,
        resample::{Factor, LongEdge, Settings, Size},
    },
    runtime::{GlobalContext, KernelContext},
};
use std::{f64::consts::PI, sync::Arc};

const FILTERS: [Filter; 5] =
    [Filter::Lanczos3, Filter::Mitchell, Filter::CatmullRom, Filter::Triangle, Filter::Box];

pub fn check(evaluator: &Evaluator) {
    identity_is_exact_for_interpolating_filters(evaluator);
    constants_survive_any_map_and_scale(evaluator);
    box_halving_averages_blocks(evaluator);
    per_channel_maps_match_the_reference(evaluator);
    minification_antialiases(evaluator);
    preview_keeps_framing(evaluator);
    perspective_crops_exactly_and_matches_the_reference(evaluator);
}

fn settings(filter: Filter, factor: f64) -> Settings {
    Settings { filter, size: Size::Factor(Factor { factor }) }
}

pub fn pattern(width: u32, height: u32) -> Vec<[f32; 3]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = ((i % width) as f32, (i / width) as f32);
            [(x * 0.7).sin() * 4.0 + y, -(x * y) / 8.0, ((i * 31) % 17) as f32 / 4.0 - 1.0]
        })
        .collect()
}

/// Source → Resample, evaluated under `global`.
fn resample(
    evaluator: &Evaluator,
    global: &GlobalContext,
    desc: RgbDesc,
    pixels: Vec<[f32; 3]>,
    p: Settings,
) -> (RgbDesc, Vec<[f32; 3]>) {
    let mut dag = Dag::new();
    let source = node::source::<Rgb>(&mut dag, desc, pixels).unwrap();
    let resample = node::resample::add(&mut dag, p).unwrap();
    dag.connect(source, resample.image).unwrap();
    let result = evaluator.evaluate::<Rgb>(&dag, global, resample.output).unwrap();
    (result.desc.clone(), result.data.host().unwrap().to_vec())
}

/// Per-channel maps from the rectangle `origin + [0, size]` into the plane of
/// `source`, then an optional radial part, as the reference resampler reads
/// them.
#[derive(Clone)]
pub struct Warped {
    pub source: RgbDesc,
    pub origin: [f64; 2],
    pub size: [f64; 2],
    pub maps: [Mat3; 3],
    pub radial: Option<Radial>,
}

/// `warped` through a warp whose maps differ per channel, then a Resample by
/// `p`, through the image API. The rectangle's size is whole pixels.
pub fn resample_warped(
    evaluator: &Evaluator,
    warped: &Warped,
    pixels: Vec<[f32; 3]>,
    p: Settings,
) -> (RgbDesc, Vec<[f32; 3]>) {
    let Size::Factor(Factor { factor }) = p.size else { unreachable!() };
    let global = GlobalContext::default();
    let ctx = KernelContext::new(&global, evaluator.runtime());
    let source = Raster::<Rgb>::from_host(warped.source.clone(), pixels.into()).unwrap();
    let [x, y] = warped.origin;
    let shift = [[1.0, 0.0, x], [0.0, 1.0, y], [0.0, 0.0, 1.0]];
    let maps = warped.maps.map(|m| matrix::mul(&m, &shift));
    let warp = match &warped.radial {
        Some(radial) => Warp::radial(maps, radial.clone(), p.filter.body()),
        None => Warp::filtered(maps, p.filter.body()),
    };
    let extent = |scale: f64| {
        let [w, h] = warped.size.map(|v| (v * scale).round() as u32);
        let desc: Description = Arc::new(RgbDesc::new(
            Extent { width: w, height: h },
            warped.source.coordinates.clone(),
        ));
        desc
    };
    let image = Image::from(source).step::<Rgb>(Step::Warp(&warp), extent(1.0)).unwrap();
    let output = extent(factor);
    let image = image.step::<Rgb>(Step::Resample(p.filter.body()), output.clone()).unwrap();
    let raster = image.raster::<Rgb>(&ctx).unwrap();
    (raster.desc().clone(), raster.host().unwrap().to_vec())
}

fn identity_is_exact_for_interpolating_filters(evaluator: &Evaluator) {
    let extent = Extent { width: 7, height: 5 };
    let mut pixels = pattern(7, 5);
    pixels[3] = [f32::NAN, f32::INFINITY, -1e30];
    pixels[17] = [f32::MAX, -f32::MAX, 0.0];
    let desc = RgbDesc::new(extent.clone(), Coordinates::Space(REC2020_D65));
    for filter in [Filter::Lanczos3, Filter::CatmullRom, Filter::Triangle, Filter::Box] {
        let (out, data) = resample(
            evaluator,
            &Default::default(),
            desc.clone(),
            pixels.clone(),
            settings(filter, 1.0),
        );
        assert_eq!((out.extent, out.full), (extent.clone(), extent.clone()));
        assert_eq!(out.coordinates, desc.coordinates);
        // Bit-exact, signed zeros included.
        assert_eq!(bits(&data), bits(&pixels), "{filter:?}");
    }
}

fn constants_survive_any_map_and_scale(evaluator: &Evaluator) {
    let (width, height) = (13, 11);
    let value = [0.25, -3.0, 1e4];
    let pixels = vec![value; (width * height) as usize];
    let source = RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65));
    let maps = [rotation(0.2, [6.5, 5.5]), rotation(-0.1, [6.5, 5.5]), shear(0.3)];
    let warped = Warped { source, origin: [3.0, 3.0], size: [7.0, 5.0], maps, radial: None };
    for filter in FILTERS {
        for factor in [0.37, 1.0, 2.5] {
            let (_, data) =
                resample_warped(evaluator, &warped, pixels.clone(), settings(filter, factor));
            for pixel in &data {
                for c in 0..3 {
                    let error = (pixel[c] - value[c]).abs() / value[c].abs();
                    assert!(error < 1e-5, "{filter:?} ×{factor}: {pixel:?}");
                }
            }
        }
    }
}

fn box_halving_averages_blocks(evaluator: &Evaluator) {
    let (width, height) = (8, 6);
    let pixels = pattern(width, height);
    let desc = RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65));
    let (out, data) =
        resample(evaluator, &Default::default(), desc, pixels.clone(), settings(Filter::Box, 0.5));
    assert_eq!(out.extent, Extent { width: 4, height: 3 });
    for (i, pixel) in data.iter().enumerate() {
        let (x, y) = (i % 4 * 2, i / 4 * 2);
        for c in 0..3 {
            let block: f32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .map(|(dx, dy)| pixels[(y + dy) * 8 + x + dx][c])
                .iter()
                .sum();
            assert!((pixel[c] - block / 4.0).abs() < 1e-5, "{i}: {pixel:?}");
        }
    }
}

fn per_channel_maps_match_the_reference(evaluator: &Evaluator) {
    let (width, height) = (9, 7);
    let pixels = pattern(width, height);
    // A reduced source exercises the stored/full ratio.
    let source = RgbDesc {
        extent: Extent { width, height },
        full: Extent { width: 18, height: 14 },
        window: None,
        coordinates: Coordinates::Space(REC2020_D65),
    };
    let perspective = [[1.0, 0.05, -0.4], [0.02, 0.95, 0.3], [0.004, -0.003, 1.0]];
    let maps = [rotation(0.3, [9.0, 7.0]), perspective, shear(-0.2)];
    let warped = Warped { source, origin: [1.5, -2.0], size: [16.0, 15.0], maps, radial: None };
    let magnitude = pixels.iter().flatten().fold(1.0f32, |m, v| m.max(v.abs()));
    // Box is discontinuous at its edge: f32 and f64 positions disagree on which
    // taps sit exactly there. The exact tests above cover it.
    for filter in &FILTERS[..4] {
        let filter = *filter;
        for factor in [0.3, 0.6, 1.7] {
            let (out, data) =
                resample_warped(evaluator, &warped, pixels.clone(), settings(filter, factor));
            let expected = reference(&warped, &pixels, &out.extent, filter);
            for (i, (actual, expected)) in data.iter().zip(&expected).enumerate() {
                for c in 0..3 {
                    let error = (f64::from(actual[c]) - expected[c]).abs();
                    assert!(
                        error < 1e-4 * f64::from(magnitude),
                        "{filter:?} ×{factor} sample {i}.{c}: {} vs {}",
                        actual[c],
                        expected[c]
                    );
                }
            }
        }
    }
}

fn minification_antialiases(evaluator: &Evaluator) {
    // Alternating columns: the Nyquist frequency, which point sampling at a
    // quarter of the rate would return as solid 0 or 1.
    let (width, height) = (64, 8);
    let pixels: Vec<_> = (0..width * height).map(|i| [(i % 2) as f32; 3]).collect();
    let desc = RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65));
    for filter in FILTERS {
        let p = settings(filter, 0.25);
        let (out, data) = resample(evaluator, &Default::default(), desc.clone(), pixels.clone(), p);
        // Away from the edges, where repeated edge columns bias the mean.
        let interior = data.iter().enumerate().filter(|(i, _)| (3..13).contains(&(i % 16)));
        assert_eq!(out.extent.width, 16);
        for (i, pixel) in interior {
            assert!((pixel[0] - 0.5).abs() < 0.02, "{filter:?} {i}: {pixel:?}");
        }
    }
}

fn preview_keeps_framing(evaluator: &Evaluator) {
    let desc = RgbDesc::new(Extent { width: 7, height: 5 }, Coordinates::Space(REC2020_D65));
    let preview = GlobalContext { scale: 2 };
    let (out, data) =
        resample(evaluator, &preview, desc.clone(), pattern(7, 5), settings(Filter::Lanczos3, 1.0));
    assert_eq!((out.extent, out.full), (Extent { width: 4, height: 3 }, desc.extent.clone()));
    assert_eq!(data.len(), 12);
    let long = Settings { filter: Filter::Lanczos3, size: Size::LongEdge(LongEdge { pixels: 14 }) };
    let (out, _) = resample(evaluator, &preview, desc, pattern(7, 5), long);
    assert_eq!(
        (out.extent, out.full),
        (Extent { width: 7, height: 5 }, Extent { width: 14, height: 10 })
    );
}

fn perspective_crops_exactly_and_matches_the_reference(evaluator: &Evaluator) {
    use perspective::Aspect;
    let (width, height) = (8, 6);
    let pixels = pattern(width, height);
    let desc = RgbDesc::new(Extent { width, height }, Coordinates::Space(REC2020_D65));
    // Perspective Transform alone, and followed by Resample at factor one.
    let run = |quad, global: &GlobalContext| {
        let mut dag = Dag::new();
        let source = node::source::<Rgb>(&mut dag, desc.clone(), pixels.clone()).unwrap();
        let p = perspective::Settings::default().with_quad(quad, Aspect::Corners);
        let transform = perspective::add(&mut dag, p.clone()).unwrap();
        let resample = node::resample::add(&mut dag, settings(Filter::Lanczos3, 1.0)).unwrap();
        dag.connect(source, transform.image).unwrap();
        dag.connect(transform.output, resample.image).unwrap();
        // The warp and the Resample run as one launch, with the same filter.
        // Evaluating the warp's output first would store it, and the
        // Resample would then start from that raster.
        let result = evaluator.evaluate::<Rgb>(&dag, global, resample.output).unwrap();
        assert_eq!(result.statistics.fused, 1);
        let alone = evaluator.evaluate::<Rgb>(&dag, global, transform.output).unwrap();
        let data = result.data.host().unwrap().to_vec();
        assert_eq!(alone.desc, result.desc);
        assert_eq!(bits(alone.data.host().unwrap()), bits(&data));
        let frame = perspective::frame(&p, perspective::plane(&desc)).unwrap();
        let warped = Warped {
            source: desc.clone(),
            origin: [0.0, 0.0],
            size: frame.size,
            maps: [frame.map; 3],
            radial: None,
        };
        (warped, result.desc.clone(), data)
    };
    // Columns 2..6, rows 0..3.
    let crop = [[0.25, 0.0], [0.75, 0.0], [0.75, 0.5], [0.25, 0.5]];
    let (_, out, data) = run(crop, &Default::default());
    assert_eq!(out.extent, Extent { width: 4, height: 3 });
    let expected: Vec<_> =
        (0..3).flat_map(|y| (2..6).map(move |x| (x, y))).map(|(x, y)| pixels[y * 8 + x]).collect();
    assert_eq!(data, expected);
    let (_, out, _) = run(crop, &GlobalContext { scale: 2 });
    assert_eq!(
        (out.extent, out.full),
        (Extent { width: 2, height: 2 }, Extent { width: 4, height: 3 })
    );

    let keystone = [[0.1, 0.0], [0.9, 0.05], [1.0, 1.0], [0.0, 0.9]];
    let (warped, out, data) = run(keystone, &Default::default());
    let expected = reference(&warped, &pixels, &out.extent, Filter::Lanczos3);
    for (actual, expected) in data.iter().zip(&expected) {
        for c in 0..3 {
            assert!(
                (f64::from(actual[c]) - expected[c]).abs() < 1e-3,
                "{actual:?} vs {expected:?}"
            );
        }
    }
}

/// Sample bits, with NaN payloads, which arithmetic may canonicalize, as `None`.
pub fn bits(v: &[[f32; 3]]) -> Vec<Option<u32>> {
    v.iter().flatten().map(|v| if v.is_nan() { None } else { Some(v.to_bits()) }).collect()
}

pub fn rotation(angle: f64, [cx, cy]: [f64; 2]) -> [[f64; 3]; 3] {
    let (s, c) = angle.sin_cos();
    [[c, -s, cx - c * cx + s * cy], [s, c, cy - s * cx - c * cy], [0.0, 0.0, 1.0]]
}

pub fn shear(k: f64) -> [[f64; 3]; 3] {
    [[1.0, k, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

fn weight(filter: Filter, x: f64) -> f64 {
    let t = x.abs();
    let sinc = |x: f64| if x == 0.0 { 1.0 } else { (PI * x).sin() / (PI * x) };
    let cubic = |b: f64, c: f64| {
        let p = if t < 1.0 {
            (12.0 - 9.0 * b - 6.0 * c) * t.powi(3) + (-18.0 + 12.0 * b + 6.0 * c) * t * t + 6.0
                - 2.0 * b
        } else if t < 2.0 {
            (-b - 6.0 * c) * t.powi(3)
                + (6.0 * b + 30.0 * c) * t * t
                + (-12.0 * b - 48.0 * c) * t
                + 8.0 * b
                + 24.0 * c
        } else {
            0.0
        };
        p / 6.0
    };
    match filter {
        Filter::Lanczos3 if t < 3.0 => sinc(x) * sinc(x / 3.0),
        Filter::Lanczos3 => 0.0,
        Filter::Mitchell => cubic(1.0 / 3.0, 1.0 / 3.0),
        Filter::CatmullRom => cubic(0.0, 0.5),
        Filter::Triangle => (1.0 - t).max(0.0),
        Filter::Box => f64::from(u8::from(t <= 0.5)),
    }
}

/// Plane geometry written out directly: output pixel centre → rectangle → map
/// → radial part → source plane → source index, with a numerically
/// differentiated footprint and the radial part's gain.
pub fn reference(
    warped: &Warped,
    pixels: &[[f32; 3]],
    out: &Extent,
    filter: Filter,
) -> Vec<[f64; 3]> {
    let source = &warped.source;
    let (w, h) = (source.extent.width as i64, source.extent.height as i64);
    let support = f64::from(filter.body().support);
    let mut result = vec![[0.0; 3]; out.pixels()];
    for (c, m) in warped.maps.iter().enumerate() {
        // The point the maps take output sample (j, k) to, before the radial
        // part.
        let point = |j: f64, k: f64| -> Option<[f64; 2]> {
            let y = [
                warped.origin[0] + (j + 0.5) * warped.size[0] / f64::from(out.width),
                warped.origin[1] + (k + 0.5) * warped.size[1] / f64::from(out.height),
            ];
            let q = m.map(|r| r[0] * y[0] + r[1] * y[1] + r[2]);
            (q[2] > 0.0).then(|| [q[0] / q[2], q[1] / q[2]])
        };
        let index = |j: f64, k: f64| -> Option<[f64; 2]> {
            let q = point(j, k)?;
            let p = warped.radial.as_ref().map_or(q, |radial| radial.source(c, q));
            Some([
                p[0] * f64::from(source.extent.width) / f64::from(source.full.width) - 0.5,
                p[1] * f64::from(source.extent.height) / f64::from(source.full.height) - 0.5,
            ])
        };
        for (i, result) in result.iter_mut().enumerate() {
            let (j, k) = ((i % out.width as usize) as f64, (i / out.width as usize) as f64);
            let Some([u, v]) = index(j, k) else { continue };
            if u < -0.5 || u > w as f64 - 0.5 || v < -0.5 || v > h as f64 - 0.5 {
                continue;
            }
            let e = 1e-4;
            let [uj, vj] = index(j + e, k).unwrap();
            let [uk, vk] = index(j, k + e).unwrap();
            let footprint =
                |dj: f64, dk: f64| ((dj / e).powi(2) + (dk / e).powi(2)).sqrt().clamp(1.0, 16.0);
            let (su, sv) = (footprint(uj - u, uk - u), footprint(vj - v, vk - v));
            let (mut sum, mut total) = (0.0, 0.0);
            for b in ((v - support * sv).ceil() as i64)..=((v + support * sv).floor() as i64) {
                for a in ((u - support * su).ceil() as i64)..=((u + support * su).floor() as i64) {
                    let weight =
                        weight(filter, (a as f64 - u) / su) * weight(filter, (b as f64 - v) / sv);
                    let pixel = pixels[(b.clamp(0, h - 1) * w + a.clamp(0, w - 1)) as usize];
                    sum += weight * f64::from(pixel[c]);
                    total += weight;
                }
            }
            let gain =
                warped.radial.as_ref().map_or(1.0, |radial| radial.gain(c, point(j, k).unwrap()));
            result[c] = sum / total * gain;
        }
    }
    result
}
