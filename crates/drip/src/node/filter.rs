//! Resampling filter kernels, one source for every backend and the host.
//!
//! Each kernel takes a signed distance in filter units (source pixels divided
//! by the footprint scale) and returns an unnormalized weight. Callers
//! normalize the weights of each output sample, which also absorbs Lanczos'
//! small deviation from a partition of unity. Kernels are symmetric and zero
//! outside their support. [`Filter::body`] registers one with the fused
//! kernel's resampling stage (`plan.rs`).
//!
//! \[1\] D. P. Mitchell and A. N. Netravali, “Reconstruction filters in
//! computer graphics,” *Comput. Graph. (Proc. SIGGRAPH)*, vol. 22, no. 4,
//! pp. 221–228, Aug. 1988, doi: 10.1145/378456.378514.
//!
//! \[2\] C. E. Duchon, “Lanczos filtering in one and two dimensions,” *J. Appl.
//! Meteorol.*, vol. 18, no. 8, pp. 1016–1022, Aug. 1979,
//! doi: 10.1175/1520-0450(1979)018<1016:LFIOAT>2.0.CO;2.
//!
//! \[3\] W3C, *WebGPU Shading Language*, sec. 15.7.4, “Floating point
//! accuracy.” \[Online\]. Available: <https://www.w3.org/TR/WGSL/>
use crate::body::FilterBody;
pub use crate::body::{BOX_SUPPORT, box_filter};
use cubecl::prelude::*;
use std::f32::consts::PI;

pub const TRIANGLE_SUPPORT: f32 = 1.0;
pub const MITCHELL_NETRAVALI_SUPPORT: f32 = 2.0;
pub const LANCZOS3_SUPPORT: f32 = 3.0;

/// A filter by name, for parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, crate::Choice)]
pub enum Filter {
    #[default]
    #[choice("lanczos3")]
    #[label("Lanczos-3")]
    Lanczos3,
    #[choice("mitchell")]
    #[label("Mitchell")]
    Mitchell,
    #[choice("catmull_rom")]
    #[label("Catmull–Rom")]
    CatmullRom,
    #[choice("triangle")]
    #[label("Triangle")]
    Triangle,
    #[choice("box")]
    #[label("Box")]
    Box,
}

impl Filter {
    /// The kernel as the fused resampling stage calls it.
    pub fn body(self) -> &'static FilterBody {
        match self {
            Self::Lanczos3 => &LANCZOS3_BODY,
            Self::Mitchell => &MITCHELL_BODY,
            Self::CatmullRom => &CATMULL_ROM_BODY,
            Self::Triangle => &TRIANGLE_BODY,
            Self::Box => &crate::body::BOX,
        }
    }
}

static LANCZOS3_BODY: FilterBody = FilterBody {
    name: "lanczos3",
    support: LANCZOS3_SUPPORT,
    interpolating: true,
    expand: lanczos3::expand,
};
static MITCHELL_BODY: FilterBody = FilterBody {
    name: "mitchell",
    support: MITCHELL_NETRAVALI_SUPPORT,
    interpolating: false,
    expand: mitchell::expand,
};
static CATMULL_ROM_BODY: FilterBody = FilterBody {
    name: "catmull_rom",
    support: MITCHELL_NETRAVALI_SUPPORT,
    interpolating: true,
    expand: catmull_rom::expand,
};
static TRIANGLE_BODY: FilterBody = FilterBody {
    name: "triangle",
    support: TRIANGLE_SUPPORT,
    interpolating: true,
    expand: triangle::expand,
};
/// Triangle (tent): `1 − |x|` on `[−1, 1]`; bilinear interpolation when
/// magnifying.
#[cube]
pub fn triangle(x: f32) -> f32 {
    f32::max(1.0f32 - x.abs(), 0.0f32)
}

/// The Mitchell–Netravali cubic family \[1\]. `B = C = 1/3` is the authors'
/// recommended compromise between blur and ringing ("Mitchell"); `B = 0,
/// C = 1/2` is Catmull–Rom, which interpolates. Every member with these
/// coefficients sums to one over integer shifts, so flat regions stay flat
/// before normalization.
#[cube]
pub fn mitchell_netravali(x: f32, b: f32, c: f32) -> f32 {
    let t = x.abs();
    let mut weight = 0.0f32;
    if t < 1.0f32 {
        weight = ((12.0f32 - 9.0f32 * b - 6.0f32 * c) * t + (-18.0f32 + 12.0f32 * b + 6.0f32 * c))
            * t
            * t
            + (6.0f32 - 2.0f32 * b);
    } else if t < 2.0f32 {
        // The paper's power form, rewritten in u = 2 − t: the outer piece has a
        // double root at the support edge, and this avoids f32 cancellation
        // between terms near 20 in the expanded coefficients.
        let u = 2.0f32 - t;
        weight = u * u * ((b + 6.0f32 * c) * u - 6.0f32 * c);
    }
    weight / 6.0f32
}

/// Mitchell–Netravali with `B = C = 1/3`.
#[cube]
pub fn mitchell(x: f32) -> f32 {
    mitchell_netravali(x, 1.0f32 / 3.0f32, 1.0f32 / 3.0f32)
}

/// Catmull–Rom: Mitchell–Netravali with `B = 0, C = 1/2`.
#[cube]
pub fn catmull_rom(x: f32) -> f32 {
    mitchell_netravali(x, 0.0f32, 0.5f32)
}

/// Lanczos-3: `sinc(x) sinc(x / 3)` for `|x| < 3` \[2\]. It interpolates:
/// exactly one at zero and exactly zero at the other integers, so an aligned
/// identity resampling reproduces its input. Negative lobes ring around strong
/// edges, which in scene-linear HDR shows as signed halos near highlights.
#[cube]
pub fn lanczos3(x: f32) -> f32 {
    let t = x.abs();
    let mut weight = 0.0f32;
    if t < 1e-4f32 {
        // 1 − (5π²/27)t² rounds to one in f32 here, and the quotients below
        // would lose their meaning as t underflows.
        weight = 1.0f32;
    } else if t < 3.0f32 {
        weight = sin_pi(t) / (PI * t) * (sin_pi(t / 3.0f32) / (PI * t / 3.0f32));
    }
    weight
}

/// `sin(πx)` for the filter range (`|x|` up to a few units). WGSL guarantees
/// `sin` only to 2⁻¹¹ absolute error and only on `[−π, π]` \[3\], too coarse
/// for weights and outside the range Lanczos-3 needs. Reducing to
/// `r = x − n ∈ [−½, ½)` and summing the Taylor series of `sin(πr)` through
/// `r¹¹` (next term below 5.7e-8) keeps every backend within f32 rounding of
/// the true value, up to the driver's freedom to fuse or reorder arithmetic.
/// Integer `x` gives `r = 0` and an exact zero.
#[cube]
fn sin_pi(x: f32) -> f32 {
    let n = (x + 0.5f32).floor();
    let r = x - n;
    let r2 = r * r;
    // π^(2k+1) / (2k+1)! for k = 0..=5.
    let series = r
        * (PI
            - r2 * (5.167_712_8f32
                - r2 * (2.550_164f32
                    - r2 * (0.599_264_5f32 - r2 * (0.082_145_89f32 - r2 * 0.007_370_431f32)))));
    let half = n * 0.5f32;
    if half == half.floor() { series } else { -series }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{payload::DeviceBuffer, runtime::RuntimeContext};

    const MITCHELL: (f32, f32) = (1.0 / 3.0, 1.0 / 3.0);
    const CATMULL_ROM: (f32, f32) = (0.0, 0.5);

    /// The paper's power form [1], independent of the factored kernel.
    fn mitchell_netravali_f64(x: f64, (b, c): (f32, f32)) -> f64 {
        let (b, c, t) = (f64::from(b), f64::from(c), x.abs());
        let p = if t < 1.0 {
            (12.0 - 9.0 * b - 6.0 * c) * t.powi(3)
                + (-18.0 + 12.0 * b + 6.0 * c) * t * t
                + (6.0 - 2.0 * b)
        } else if t < 2.0 {
            (-b - 6.0 * c) * t.powi(3)
                + (6.0 * b + 30.0 * c) * t * t
                + (-12.0 * b - 48.0 * c) * t
                + (8.0 * b + 24.0 * c)
        } else {
            0.0
        };
        p / 6.0
    }

    fn lanczos3_f64(x: f64) -> f64 {
        let sinc = |x: f64| {
            if x == 0.0 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            }
        };
        if x.abs() < 3.0 { sinc(x) * sinc(x / 3.0) } else { 0.0 }
    }

    /// Distances covering both supports, their knots and points outside.
    fn distances() -> Vec<f32> {
        (-1400..=1400).map(|i| i as f32 / 400.0).collect()
    }

    #[test]
    fn mitchell_netravali_knots_symmetry_and_unit_sum() {
        let w = |x, (b, c)| mitchell_netravali(x, b, c);
        assert!((w(0.0, MITCHELL) - 8.0 / 9.0).abs() < 1e-7);
        assert!((w(1.0, MITCHELL) - 1.0 / 18.0).abs() < 1e-7);
        assert_eq!(w(0.0, CATMULL_ROM), 1.0);
        assert_eq!(w(1.0, CATMULL_ROM), 0.0);
        for filter in [MITCHELL, CATMULL_ROM] {
            assert_eq!(w(MITCHELL_NETRAVALI_SUPPORT, filter), 0.0);
            for x in distances() {
                let error = f64::from(w(x, filter)) - mitchell_netravali_f64(f64::from(x), filter);
                assert!(error.abs() < 2e-7, "{filter:?} at {x}: error {error}");
                assert_eq!(w(x, filter).to_bits(), w(-x, filter).to_bits());
                if x.abs() >= MITCHELL_NETRAVALI_SUPPORT {
                    assert_eq!(w(x, filter), 0.0);
                }
            }
            for i in 0..=100 {
                let offset = i as f32 / 100.0;
                let sum: f32 = (-3..=3).map(|k| w(offset - k as f32, filter)).sum();
                assert!((sum - 1.0).abs() < 1e-6, "{filter:?} at {offset}: {sum}");
            }
        }
        // Pieces meet at the inner knot.
        for filter in [MITCHELL, CATMULL_ROM] {
            assert!((w(1.0 - 1e-6, filter) - w(1.0, filter)).abs() < 1e-5);
        }
    }

    #[test]
    fn lanczos3_matches_reference_and_interpolates() {
        assert_eq!(lanczos3(0.0), 1.0);
        for k in 1..=4 {
            assert_eq!(lanczos3(k as f32), 0.0);
            assert_eq!(lanczos3(-(k as f32)), 0.0);
        }
        for x in distances() {
            let error = (f64::from(lanczos3(x)) - lanczos3_f64(f64::from(x))).abs();
            assert!(error < 3e-7, "lanczos3({x}) error {error}");
            assert_eq!(lanczos3(x).to_bits(), lanczos3(-x).to_bits());
        }
        for x in [1e-5f32, 9.9e-5, 1.0001e-4, 2e-4] {
            assert!((f64::from(lanczos3(x)) - lanczos3_f64(f64::from(x))).abs() < 1e-7);
        }
    }

    #[test]
    fn box_and_triangle_sum_to_one() {
        assert_eq!((box_filter(0.5), box_filter(-0.5), box_filter(0.5001)), (1.0, 1.0, 0.0));
        assert_eq!((triangle(0.0), triangle(0.25), triangle(1.0)), (1.0, 0.75, 0.0));
        for i in 1..100 {
            let offset = i as f32 / 100.0;
            let sum = |w: fn(f32) -> f32| (-2..=2).map(|k| w(offset - k as f32)).sum::<f32>();
            assert!((sum(triangle) - 1.0).abs() < 1e-6);
            if offset != 0.5 {
                assert_eq!(sum(box_filter), 1.0);
            }
        }
        for x in distances() {
            assert_eq!(triangle(x), triangle(-x));
            assert_eq!(box_filter(x), box_filter(-x));
        }
    }

    const FILTERS: [Filter; 5] =
        [Filter::Lanczos3, Filter::Mitchell, Filter::CatmullRom, Filter::Triangle, Filter::Box];

    /// The host evaluation of each filter's kernel, in [`FILTERS`] order.
    fn weights(x: f32) -> [f32; 5] {
        [lanczos3(x), mitchell(x), catmull_rom(x), triangle(x), box_filter(x)]
    }

    #[test]
    fn bodies_declare_their_support_and_interpolation() {
        for x in distances() {
            for (filter, weight) in FILTERS.into_iter().zip(weights(x)) {
                if x.abs() > filter.body().support {
                    assert_eq!(weight, 0.0, "{filter:?} at {x}");
                }
            }
        }
        assert_eq!(mitchell(0.5), mitchell_netravali(0.5, MITCHELL.0, MITCHELL.1));
        assert_eq!(catmull_rom(0.5), mitchell_netravali(0.5, CATMULL_ROM.0, CATMULL_ROM.1));
        for (filter, at_zero) in FILTERS.into_iter().zip(weights(0.0)) {
            let at_integers = (1..=4).flat_map(|k| [k as f32, -(k as f32)]).map(weights);
            let zeros = at_integers.map(|w| w[filter as usize]).all(|w| w == 0.0);
            assert_eq!(filter.body().interpolating, at_zero == 1.0 && zeros, "{filter:?}");
        }
    }

    #[test]
    fn sin_pi_matches_reference() {
        for i in -1600..=1600 {
            let x = i as f32 / 400.0;
            let expected = (std::f64::consts::PI * f64::from(x)).sin();
            assert!((f64::from(sin_pi(x)) - expected).abs() < 2e-7, "sin_pi({x})");
        }
    }

    #[cube(launch)]
    fn evaluate(x: &[f32], output: &mut [f32]) {
        let i = ABSOLUTE_POS;
        if i < x.len() {
            output[i * 5] = lanczos3(x[i]);
            output[i * 5 + 1] = mitchell(x[i]);
            output[i * 5 + 2] = catmull_rom(x[i]);
            output[i * 5 + 3] = triangle(x[i]);
            output[i * 5 + 4] = box_filter(x[i]);
        }
    }

    /// Device results agree with the host evaluation of the same source.
    /// Drivers may contract or reorder arithmetic, so equality is not required.
    fn device_matches_host(runtime: RuntimeContext) {
        let x = distances();
        let client = runtime.client().unwrap();
        let input =
            DeviceBuffer::<()>::new(client.create(cubecl::bytes::Bytes::from_elems(x.clone())));
        let output = DeviceBuffer::<()>::new(client.empty(x.len() * 5 * size_of::<f32>()));
        let (count, dim) = crate::runtime::dispatch_dims(x.len());
        evaluate::launch(client, count, dim, input.argument(), output.argument());
        let bytes = client.read_one(output.handle.clone()).unwrap();
        let actual: &[f32] = bytemuck::cast_slice(&bytes);
        for (i, &x) in x.iter().enumerate() {
            for (actual, expected) in actual[i * 5..i * 5 + 5].iter().zip(weights(x)) {
                assert!((actual - expected).abs() < 1e-6, "x = {x}: {actual} vs {expected}");
            }
        }
    }

    #[cfg(feature = "cpu")]
    #[test]
    fn cpu_device_matches_host() {
        device_matches_host(RuntimeContext::cpu());
    }

    #[test]
    #[ignore = "requires the DRIP_BACKEND runtime and device; run explicitly"]
    fn selected_backend_device_matches_host() {
        device_matches_host(RuntimeContext::from_env().unwrap());
    }
}
