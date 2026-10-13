// SPDX-License-Identifier: GPL-3.0-or-later
// Portions Copyright (C) 2020-2026 darktable developers.
//! Embedded lens corrections as radial maps, after darktable's
//! embedded-metadata lens correction, an algorithm by Freddie Witherden
//! \[1\], \[2\]. Every vendor's values become `_init_coeffs_md_v2`'s
//! per-channel ratios of source to destination radius and a vignetting
//! divisor of the source radius, whose reciprocal is the gain.
//!
//! Deviations, each where darktable approximates or ignores data:
//!
//! - Polynomials are sampled at 256 knots to `ρ = 1.25`, not 16 to 1.
//! - Fujifilm's remap from source to destination radius is sampled at 256
//!   knots, and evaluates CA at the radius the applied distortion gives.
//! - DNG maps sit at the opcode's optical centre with the DNG
//!   specification's normalization \[3\], where darktable uses the frame
//!   centre and half diagonal. Red and blue planes act as CA relative to
//!   green, so each switch and fine-tune has its usual meaning. A vignette
//!   opcode takes the warp's centre when both are present. Tangential terms
//!   are ignored, as darktable ignores them.
//! - Panasonic's inversion iterates to convergence. darktable's two
//!   iterations leave up to 0.5 % of the radius on the DMC-TZ70 fixture.
//! - Olympus CA uses the limit at `ρ = 0` where darktable divides by zero.
//! - darktable's autoscale is the node's fill zoom.
//!
//! \[1\] darktable developers, “lens.cc” (`_init_coeffs_md_v2`), commit
//! 61dea294bedb3ab6c7cca1a45530b1ab5c0461f3, 2026. \[Online\]. Available:
//! <https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/lens.cc>
//! \[2\] F. Witherden, “Embedded lens correction,” darktable pull request
//! 7092, 2020. \[Online\]. Available:
//! <https://github.com/darktable-org/darktable/pull/7092>
//! \[3\] Adobe Inc., “Digital Negative (DNG) Specification,” ver. 1.7.1.0,
//! 2023, ch. 7, `WarpRectilinear` and `FixVignetteRadial`.
//! License: GPL-3.0-or-later; see THIRD_PARTY.md for credits and the upstream license.
use super::{KNOTS, Strength, centred, sampled, undo};
use crate::{
    Error, Result,
    body::{Radial, Spline},
};
use drip_raw::{Correction, DngVignette, DngWarp};

pub(super) fn radial(correction: &Correction, k: &Strength, size: [f64; 2]) -> Result<Radial> {
    match correction {
        Correction::Sony { distortion, ca_red, ca_blue, vignetting } => {
            sony(distortion, [ca_red, ca_blue], vignetting, k, size)
        }
        Correction::Fuji { knots, distortion, ca_red, ca_blue, vignetting, crop_mode } => {
            // Crop modes 2 and 4 crop the sensor by 1.25.
            let crop = if matches!(crop_mode, Some(2 | 4)) { 1.25 } else { 1.0 };
            let knots: Vec<_> = knots.iter().map(|&x| crop * f64::from(x)).collect();
            let f64s = |v: &[f32]| v.iter().map(|&v| f64::from(v)).collect::<Vec<_>>();
            let tables = [distortion, ca_red, ca_blue, vignetting].map(|t| f64s(t));
            fuji(&knots, &tables, k, size)
        }
        Correction::Dng { warp, vignette } => dng(warp.as_ref(), vignette.as_ref(), k, size),
        Correction::Olympus { distortion, ca } => olympus(*distortion, *ca, k, size),
        Correction::Panasonic { a, b, c, scale } => panasonic([*a, *b, *c], *scale, k, size),
    }
}

/// Tables on knots `ρ_i = (i + 0.5) / (n − 1)`, which are uniform.
fn sony(
    distortion: &[i16],
    ca: [&[i16]; 2],
    vignetting: &[i16],
    k: &Strength,
    size: [f64; 2],
) -> Result<Radial> {
    let step = 1.0 / (distortion.len() - 1) as f64;
    let spline = |values: Vec<f64>| Spline::new(0.5 * step, step, values);
    let green: Vec<_> =
        distortion.iter().map(|&d| 1.0 + k.distortion * f64::from(d) * 2f64.powi(-14)).collect();
    let fringe = |table: &[i16], tune: f64| {
        let values = green.iter().zip(table);
        spline(values.map(|(g, &ca)| g * (1.0 + tune * f64::from(ca) * 2f64.powi(-21))).collect())
    };
    let gain = vignetting.iter().map(|&v| {
        undo(2f64.powf(0.5 - 2f64.powf(k.vignetting * f64::from(v) * 2f64.powi(-13) - 1.0)))
    });
    let ratios = [fringe(ca[0], k.red)?, spline(green.clone())?, fringe(ca[1], k.blue)?];
    Ok(centred(size, ratios, spline(gain.collect())?))
}

/// Tables of the source radius on non-uniform `knots`, in percent for
/// distortion and vignetting. darktable adds a neutral knot at 0 and
/// resamples the ratios onto destination radii: the source radius `r`
/// lies at destination radius `r / m(r)`, where `m` is the distortion
/// ratio. Ratios and gain are then resampled onto uniform knots.
fn fuji(knots: &[f64], tables: &[Vec<f64>; 4], k: &Strength, size: [f64; 2]) -> Result<Radial> {
    let [distortion, red, blue, vignetting] = tables;
    let lead = knots[0] > 0.0;
    let column = |neutral: f64, values: Vec<f64>| {
        if lead { [vec![neutral], values].concat() } else { values }
    };
    let knots = column(0.0, knots.to_vec());
    let m = column(1.0, distortion.iter().map(|d| 1.0 + k.distortion * d / 100.0).collect());
    let red = column(0.0, red.iter().map(|ca| k.red * ca).collect());
    let blue = column(0.0, blue.iter().map(|ca| k.blue * ca).collect());
    let vignetting =
        column(1.0, vignetting.iter().map(|v| 1.0 - k.vignetting * (1.0 - v / 100.0)).collect());

    let sources: Vec<_> = (0..KNOTS).map(|i| i as f64 / (KNOTS - 1) as f64).collect();
    let at = |values: &[f64], r: f64| interpolate(&knots, values, r);
    let destinations: Vec<_> = sources.iter().map(|&r| r / at(&m, r)).collect();
    let ratio = |fringe: Option<&[f64]>| {
        let values = sources.iter().map(|&r| at(&m, r) * (1.0 + fringe.map_or(0.0, |f| at(f, r))));
        resampled(&destinations, &values.collect::<Vec<_>>())
    };
    let ratios = [ratio(Some(&red))?, ratio(None)?, ratio(Some(&blue))?];
    let gain = resampled(&knots, &vignetting.iter().map(|&v| undo(v)).collect::<Vec<_>>())?;
    Ok(centred(size, ratios, gain))
}

/// Radial polynomials of `r`, the distance from the opcode's optical centre
/// over its greatest distance to a corner: `Σ kr_i r^2i` for the warp, which
/// maps destination to source, and `1 + Σ k_i r^(2i+2)` for the vignette,
/// which multiplies the source.
fn dng(
    warp: Option<&DngWarp>,
    vignette: Option<&DngVignette>,
    k: &Strength,
    [w, h]: [f64; 2],
) -> Result<Radial> {
    // drip-raw keeps a correction only when one of the two is present.
    let [fx, fy] = warp.map_or_else(|| vignette.map_or([0.5; 2], |v| v.centre), |w| w.centre);
    if !((0.0..=1.0).contains(&fx) && (0.0..=1.0).contains(&fy)) {
        return Err(Error::Contract("the DNG's optical centre lies outside the image".into()));
    }
    let centre = [fx * w, fy * h];
    let radius = fx.max(1.0 - fx) * w;
    let radius = radius.hypot(fy.max(1.0 - fy) * h);
    let polynomial = |c: &[f64; 6], r: f64| {
        let r2 = r * r;
        c[0] + r2 * (c[1] + r2 * (c[2] + r2 * c[3]))
    };
    // One plane serves every channel, three are red, green and blue.
    let plane = |c: usize| warp.map(|w| w.planes[c.min(w.planes.len() - 1)]);
    let green = |r: f64| 1.0 + k.distortion * (plane(1).map_or(1.0, |p| polynomial(&p, r)) - 1.0);
    let fringe = |c: usize, tune: f64| {
        let relative = move |r: f64| match (plane(c), plane(1)) {
            (Some(own), Some(green)) => polynomial(&own, r) / polynomial(&green, r),
            _ => 1.0,
        };
        sampled(move |r| green(r) * (1.0 + tune * (relative(r) - 1.0)))
    };
    let ratios = [fringe(0, k.red)?, sampled(green)?, fringe(2, k.blue)?];
    let gain = sampled(|r| {
        let falloff = vignette.map_or(0.0, |v| {
            let r2 = r * r;
            r2 * (v.k[0] + r2 * (v.k[1] + r2 * (v.k[2] + r2 * (v.k[3] + r2 * v.k[4]))))
        });
        undo(1.0 / (1.0 + k.vignetting * falloff))
    })?;
    Ok(Radial { centre, radius, ratios, gain: Some(gain) })
}

/// Distortion `[k2, k4, k6, s]`: a destination radius `r` reads the source
/// at `s r (1 + k2 (s r)² + k4 (s r)⁴ + k6 (s r)⁶)`. CA `[r0, r2, r4, b0,
/// b2, b4]` moves red and blue from the distorted radius `d` by
/// `d (c0 + c2 d² + c4 d⁴)`.
fn olympus(
    distortion: Option<[f32; 4]>,
    ca: Option<[f32; 6]>,
    k: &Strength,
    size: [f64; 2],
) -> Result<Radial> {
    let [k2, k4, k6, scale] = distortion.unwrap_or([0.0, 0.0, 0.0, 1.0]).map(f64::from);
    let ca = ca.unwrap_or_default().map(f64::from);
    let green = |r: f64| {
        let s2 = (r * scale).powi(2);
        1.0 + k.distortion * (scale * (1.0 + s2 * (k2 + s2 * (k4 + s2 * k6))) - 1.0)
    };
    // darktable adds `tune · d · c(d) / r` with `d = g r`, which is
    // `tune · g · c(d)` and stays finite at `r = 0`.
    let fringe = |c: &[f64], tune: f64| {
        sampled(move |r| {
            let g = green(r);
            let d2 = (g * r).powi(2);
            g * (1.0 + tune * (c[0] + d2 * (c[1] + d2 * c[2])))
        })
    };
    let ratios = [fringe(&ca[..3], k.red)?, sampled(green)?, fringe(&ca[3..], k.blue)?];
    Ok(centred(size, ratios, Spline::constant(1.0)?))
}

/// Signed Q15 words: `a`, `b` and `c` are `/ 2¹⁵`, the scale is
/// `1 / (1 + w / 2¹⁵)`. A source radius `d` lands at destination radius
/// `d f(d)` with `f(d) = 1 + scale (a d² + b d⁴ + c d⁶)`, which darktable
/// inverts by the fixed point `d = r / f(d)`.
fn panasonic(words: [i16; 3], scale: i16, k: &Strength, size: [f64; 2]) -> Result<Radial> {
    let q15 = |w: i16| f64::from(w) / 32768.0;
    let [a, b, c] = words.map(q15);
    let scale = 1.0 / (1.0 + q15(scale));
    let f = |d: f64| {
        let d2 = d * d;
        1.0 + scale * d2 * (a + d2 * (b + d2 * c))
    };
    let green = |r: f64| {
        let mut d = r;
        for _ in 0..100 {
            let next = if f(d) > 1e-6 { r / f(d) } else { r };
            if next == d {
                break;
            }
            d = next;
        }
        let ratio = if r > 0.0 { d / r } else { 1.0 };
        1.0 + k.distortion * (ratio - 1.0)
    };
    let green = sampled(green)?;
    Ok(centred(size, [green.clone(), green.clone(), green], Spline::constant(1.0)?))
}

/// darktable's `_interpolate_linear_spline`: linear between knots, the end
/// values held beyond them.
fn interpolate(knots: &[f64], values: &[f64], x: f64) -> f64 {
    if x < knots[0] {
        return values[0];
    }
    for i in 1..knots.len() {
        let (x0, x1) = (knots[i - 1], knots[i]);
        if x0 < x1 && (x0..=x1).contains(&x) {
            return values[i - 1] + (x - x0) * (values[i] - values[i - 1]) / (x1 - x0);
        }
    }
    values[values.len() - 1]
}

/// The piecewise-linear curve through `knots` and `values` on [`KNOTS`]
/// uniform knots over the same span.
fn resampled(knots: &[f64], values: &[f64]) -> Result<Spline> {
    let (first, last) = (knots[0], knots[knots.len() - 1]);
    let step = (last - first) / (KNOTS - 1) as f64;
    let uniform = (0..KNOTS).map(|i| interpolate(knots, values, first + i as f64 * step));
    Spline::new(first, step, uniform.collect())
}
