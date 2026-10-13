//! Radial warps: exact where they move nothing, equal to the affine warp a
//! constant ratio describes, equal to the f64 reference where the ratios
//! curve, equal on windows and whole images, and composed with the geometry
//! after them.
use crate::resample::{Warped, bits, pattern, reference, resample_warped, rotation, shear};
use drip::{
    body::{
        Radial, Spline, Step, Warp,
        matrix::{IDENTITY, Mat3, mul},
    },
    eval::Evaluator,
    image::{Description, Image, Raster},
    lattice::Rect,
    node::{
        data::*,
        exposure,
        filter::Filter,
        resample::{Factor, Settings, Size},
    },
    runtime::{GlobalContext, KernelContext, RuntimeContext},
};
use std::{sync::Arc, time::Instant};

pub fn check(evaluator: &Evaluator) {
    ratios_of_one_reproduce_the_input(evaluator);
    a_constant_ratio_is_a_scaling_about_the_centre(evaluator);
    a_constant_gain_multiplies(evaluator);
    curved_ratios_match_the_reference(evaluator);
    windows_equal_the_whole_image(evaluator);
    later_geometry_composes_after_the_radial_part(evaluator);
}

fn desc(extent: [u32; 2], full: [u32; 2]) -> RgbDesc {
    let ([width, height], [full_width, full_height]) = (extent, full);
    RgbDesc {
        extent: Extent { width, height },
        full: Extent { width: full_width, height: full_height },
        window: None,
        coordinates: Coordinates::Space(REC2020_D65),
    }
}

fn rgb(extent: [u32; 2], full: [u32; 2]) -> Description {
    Arc::new(desc(extent, full))
}

/// `pixels` on a whole lattice of `extent` samples over `full` pixels.
fn source(extent: [u32; 2], full: [u32; 2], pixels: Vec<[f32; 3]>) -> Image {
    Image::from(Raster::<Rgb>::from_host(desc(extent, full), pixels.into()).unwrap())
}

fn host(image: &Image, evaluator: &Evaluator) -> Vec<[f32; 3]> {
    let global = GlobalContext::default();
    let ctx = KernelContext::new(&global, evaluator.runtime());
    image.raster::<Rgb>(&ctx).unwrap().host().unwrap().to_vec()
}

fn spline(start: f64, step: f64, values: &[f64]) -> Spline {
    Spline::new(start, step, values.to_vec()).unwrap()
}

/// A radial part about `centre` with one constant ratio per channel.
fn constant(centre: [f64; 2], radius: f64, ratios: [f64; 3], gain: Option<f64>) -> Radial {
    Radial {
        centre,
        radius,
        ratios: ratios.map(|k| Spline::constant(k).unwrap()),
        gain: gain.map(|g| Spline::constant(g).unwrap()),
    }
}

/// Ratios that bend outward, inward and both, and a gain that brightens
/// the corners.
fn curved(centre: [f64; 2], radius: f64) -> Radial {
    Radial {
        centre,
        radius,
        ratios: [
            spline(0.0, 0.4, &[1.0, 1.05, 1.15, 1.3]),
            spline(0.05, 0.2, &[1.0, 0.99, 0.97, 0.96, 0.98, 1.02]),
            spline(0.1, 0.3, &[1.02, 0.95, 0.9, 0.88]),
        ],
        gain: Some(spline(0.2, 0.3, &[1.0, 1.1, 1.3, 1.6])),
    }
}

/// Scaling by `k` about `centre`, as a projective map.
fn scaling(k: f64, [cx, cy]: [f64; 2]) -> Mat3 {
    [[k, 0.0, cx * (1.0 - k)], [0.0, k, cy * (1.0 - k)], [0.0, 0.0, 1.0]]
}

fn lanczos(maps: [Mat3; 3], radial: Option<Radial>) -> Warp {
    match radial {
        Some(radial) => Warp::radial(maps, radial, Filter::Lanczos3.body()),
        None => Warp::filtered(maps, Filter::Lanczos3.body()),
    }
}

fn ratios_of_one_reproduce_the_input(evaluator: &Evaluator) {
    let mut pixels = pattern(7, 5);
    pixels[3] = [f32::NAN, f32::INFINITY, -1e30];
    pixels[17] = [f32::MAX, -f32::MAX, -0.0];
    let warp = lanczos([IDENTITY; 3], Some(constant([3.1, 2.4], 4.0, [1.0; 3], None)));
    let image = source([7, 5], [7, 5], pixels.clone());
    let warped = image.step::<Rgb>(Step::Warp(&warp), rgb([7, 5], [7, 5])).unwrap();
    assert_eq!(bits(&host(&warped, evaluator)), bits(&pixels));
}

/// Constant ratios, equal and per channel, match the affine scaling about
/// the centre at full detail and at a preview lattice with unequal rates.
fn a_constant_ratio_is_a_scaling_about_the_centre(evaluator: &Evaluator) {
    let full = [23, 17];
    let pixels = pattern(23, 17);
    let magnitude = pixels.iter().flatten().fold(1.0f32, |m, v| m.max(v.abs()));
    let centre = [10.3, 9.1];
    for ratios in [[0.93; 3], [0.93, 1.0, 1.06], [1.2, 0.8, 1.0]] {
        for extent in [full, [12, 9]] {
            let image = source(full, full, pixels.clone());
            let radial = lanczos([IDENTITY; 3], Some(constant(centre, 12.0, ratios, None)));
            let affine = lanczos(ratios.map(|k| scaling(k, centre)), None);
            let run = |warp: &Warp| {
                host(&image.step::<Rgb>(Step::Warp(warp), rgb(extent, full)).unwrap(), evaluator)
            };
            for (i, (a, b)) in run(&radial).iter().zip(run(&affine)).enumerate() {
                for c in 0..3 {
                    let error = (a[c] - b[c]).abs();
                    assert!(error < 1e-4 * magnitude, "{ratios:?} {extent:?} {i}.{c}: {a:?} {b:?}");
                }
            }
        }
    }
}

fn a_constant_gain_multiplies(evaluator: &Evaluator) {
    let pixels = pattern(9, 6);
    let warp = lanczos([IDENTITY; 3], Some(constant([4.0, 3.0], 5.0, [1.0; 3], Some(1.7))));
    let image = source([9, 6], [9, 6], pixels.clone());
    let warped = image.step::<Rgb>(Step::Warp(&warp), rgb([9, 6], [9, 6])).unwrap();
    let expected: Vec<_> = pixels.iter().map(|p| p.map(|v| v * 1.7)).collect();
    assert_eq!(bits(&host(&warped, evaluator)), bits(&expected));
}

/// Curved ratios and a gain after per-channel rotation, perspective and
/// shear, on a reduced source, then a Resample.
fn curved_ratios_match_the_reference(evaluator: &Evaluator) {
    let (width, height) = (9, 7);
    let pixels = pattern(width, height);
    let source = RgbDesc {
        extent: Extent { width, height },
        full: Extent { width: 18, height: 14 },
        window: None,
        coordinates: Coordinates::Space(REC2020_D65),
    };
    let perspective = [[1.0, 0.05, -0.4], [0.02, 0.95, 0.3], [0.004, -0.003, 1.0]];
    let maps = [rotation(0.3, [9.0, 7.0]), perspective, shear(-0.2)];
    let radial = Some(curved([9.3, 6.8], 11.4));
    let warped = Warped { source, origin: [1.5, -2.0], size: [16.0, 15.0], maps, radial };
    let magnitude = pixels.iter().flatten().fold(1.0f32, |m, v| m.max(v.abs())) * 1.6;
    for factor in [0.3, 0.6, 1.7] {
        let p = Settings { filter: Filter::Lanczos3, size: Size::Factor(Factor { factor }) };
        let (out, data) = resample_warped(evaluator, &warped, pixels.clone(), p);
        let expected = reference(&warped, &pixels, &out.extent, Filter::Lanczos3);
        for (i, (actual, expected)) in data.iter().zip(&expected).enumerate() {
            for c in 0..3 {
                let error = (f64::from(actual[c]) - expected[c]).abs();
                assert!(
                    error < 1e-4 * f64::from(magnitude),
                    "×{factor} sample {i}.{c}: {} vs {}",
                    actual[c],
                    expected[c]
                );
            }
        }
    }
}

/// A radial warp after a map cuts, so its input is computed only over the
/// window the plan reads. Windows by the centre, along each edge, in the
/// corners and far from the centre then equal the whole image bit for bit,
/// at full detail and at a preview lattice. The maps are affine: a
/// projective map's footprint bound is the clamp, which would hide a
/// radial bound that falls short.
fn windows_equal_the_whole_image(evaluator: &Evaluator) {
    let global = GlobalContext::default();
    let ctx = KernelContext::new(&global, evaluator.runtime());
    let full = [61, 47];
    let centre = [29.0, 24.5];
    let maps = [
        mul(&rotation(0.05, centre), &scaling(0.95, centre)),
        [[1.0, 0.02, 0.3], [-0.01, 1.0, -0.2], [0.0, 0.0, 1.0]],
        shear(0.03),
    ];
    // Red reads up to a third farther from the centre and blue a quarter
    // nearer, which at ρ = 0.75 moves taps 5 to 7 samples, more than the
    // margin of a bound without the radial part.
    let radial = Radial {
        ratios: [
            spline(0.0, 0.4, &[1.25, 1.28, 1.3, 1.35]),
            spline(0.05, 0.2, &[1.0, 0.99, 0.97, 0.96, 0.98, 1.02]),
            spline(0.1, 0.3, &[0.8, 0.78, 0.75, 0.72]),
        ],
        ..curved(centre, 30.0)
    };
    let warp = lanczos(maps, Some(radial));
    // A new chain per evaluation, since a materialized image serves every
    // later crop from its raster.
    let image = |extent| {
        source(full, full, pattern(61, 47))
            .step::<Rgb>(Step::Map(&exposure::compute::BODY, &[1.5]), rgb(full, full))
            .and_then(|i| i.step::<Rgb>(Step::Warp(&warp), rgb(extent, full)))
            .unwrap()
    };
    for extent in [full, [31, 24]] {
        let whole = host(&image(extent), evaluator);
        let [w, h] = extent;
        let windows = [
            ([w / 2 - 3, h / 2 - 2], [6, 5]),
            ([0, h / 2], [4, 3]),
            ([w - 5, h / 3], [5, 4]),
            ([w / 3, 0], [6, 2]),
            ([w / 2, h - 3], [5, 3]),
            ([0, 0], [3, 3]),
            ([w - 4, h - 4], [4, 4]),
            ([w / 6, h / 5], [3, 3]),
            ([w * 4 / 5, h * 3 / 4], [2, 3]),
        ];
        for (origin, size) in windows {
            let window = Rect { origin, extent: size };
            let crop = image(extent).crop(window, &ctx).unwrap();
            let crop = crop.raster::<Rgb>(&ctx).unwrap().host().unwrap().to_vec();
            let expected: Vec<_> = (origin[1]..origin[1] + size[1])
                .flat_map(|y| (origin[0]..origin[0] + size[0]).map(move |x| (x, y)))
                .map(|(x, y)| whole[(y * w + x) as usize])
                .collect();
            assert_eq!(bits(&crop), bits(&expected), "{extent:?} {window:?}");
        }
    }
}

/// A reindex, a filtering warp and a Resample after a radial warp compose
/// into its front: the chain equals one radial warp through the composed
/// maps followed by the Resample, which a second resampling would not.
fn later_geometry_composes_after_the_radial_part(evaluator: &Evaluator) {
    let full = [23, 17];
    let centre = [11.5, 8.5];
    let radial = curved(centre, 14.0);
    let flip = [[-1.0, 0.0, 23.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let turn = rotation(0.1, centre);
    let image = source(full, full, pattern(23, 17));
    let resample = Step::Resample(Filter::Lanczos3.body());
    let small = rgb([14, 10], [14, 10]);
    let chain = image
        .step::<Rgb>(Step::Warp(&lanczos([IDENTITY; 3], Some(radial.clone()))), rgb(full, full))
        .and_then(|i| i.step::<Rgb>(Step::Warp(&Warp::reindex(flip)), rgb(full, full)))
        .and_then(|i| i.step::<Rgb>(Step::Warp(&lanczos([turn; 3], None)), rgb(full, full)))
        .and_then(|i| i.step::<Rgb>(resample, small.clone()))
        .unwrap();
    let once = image
        .step::<Rgb>(Step::Warp(&lanczos([mul(&flip, &turn); 3], Some(radial))), rgb(full, full))
        .and_then(|i| i.step::<Rgb>(resample, small))
        .unwrap();
    assert_eq!(bits(&host(&chain, evaluator)), bits(&host(&once, evaluator)));
}

/// One launch of a projective Filter stage and of a radial one over a
/// 7968 × 5320 image, with Lanczos-3 and the same rotation: medians of
/// eleven runs after a warm-up, each waiting for the device.
pub fn time_stages(runtime: RuntimeContext) {
    let global = GlobalContext::default();
    let ctx = KernelContext::new(&global, &runtime);
    let full = [7968, 5320];
    let pixels = pattern(full[0], full[1]);
    let input = Raster::<Rgb>::from_host(desc(full, full), pixels.into()).unwrap();
    input.device(&ctx).unwrap();
    let input = Image::from(input);
    let centre = [3984.0, 2660.0];
    let turn = [rotation(0.01, centre); 3];
    // Sony's knots for 16 coefficients, with a quadratic profile.
    let ratios = |a: f64| {
        let values: Vec<_> = (0..16).map(|i| 1.0 + a * f64::from(i * i) / 225.0).collect();
        spline(0.5 / 15.0, 1.0 / 15.0, &values)
    };
    let radial = Radial {
        centre,
        radius: 4790.0,
        ratios: [ratios(-0.012), ratios(-0.013), ratios(-0.014)],
        gain: Some(ratios(0.8)),
    };
    let stages = [
        ("projective", lanczos(turn, None)),
        ("radial", lanczos(turn, Some(Radial { gain: None, ..radial.clone() }))),
        ("radial with gain", lanczos(turn, Some(radial))),
    ];
    for (name, warp) in stages {
        let run = || {
            let start = Instant::now();
            let image = input.step::<Rgb>(Step::Warp(&warp), rgb(full, full)).unwrap();
            image.materialize(&ctx).unwrap();
            pollster::block_on(ctx.client().unwrap().sync()).unwrap();
            start.elapsed().as_secs_f64() * 1e3
        };
        run();
        let mut times: Vec<f64> = (0..11).map(|_| run()).collect();
        times.sort_by(f64::total_cmp);
        eprintln!("{name}: median {:.2} ms, range {:.2} to {:.2}", times[5], times[0], times[10]);
    }
}
