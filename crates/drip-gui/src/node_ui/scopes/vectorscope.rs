//! Vectorscope: chromaticity in CIE 1976 u′v′ \[1\], independent of exposure.
//! It uses u′v′ directly, not darktable's lightness-scaled u\*v\*, and clips
//! negative RGB components only for the plot.
//!
//! \[1\] CIE, “CIE 1976 uniform-chromaticity-scale diagram,” e-ILV,
//! term 17-23-073. \[Online\]. Available: <https://cie.co.at/eilvterm/17-23-073>

use super::{SIZE, Scope, ScopeAxes, scope};
use crate::node_ui::{GuiNode, Prepare};
use drip::node::color::{self, D65, REC2020};
use drip::registry::tag;
use drip::{
    Result,
    body::{BinsBody, matrix},
    node::data::*,
    ports::*,
    runtime::GlobalContext,
};
#[cfg(test)]
use rayon::prelude::*;
use std::sync::Arc;
const RADIUS: f32 = 0.5;
fn contract(_: &GlobalContext, _: &(), image: &RgbDesc) -> Result<(CountsDesc,)> {
    Ok((super::counts(SIZE * SIZE, 1, image),))
}
/// Plot the chromaticity of Rec.2020 RGB in CIE 1976 u′v′, with density on a
/// logarithmic scale. Black has no chromaticity and is left out, and negative
/// channels are clipped for display.
#[drip::node(id = "vectorscope", name = "Vectorscope", tags = [tag::VIEW, tag::SCOPE], contract = contract, bins = VECTORSCOPE)]
pub fn observe(
    _: &(),
    _image: Point<'_, Rgb, Rec2020>,
    _counts: Point<'_, Counts>,
) -> Result<[f32; 11]> {
    let matrix = color::rgb_to_xyz(REC2020, D65).map(|row| row.map(|v| v as f32));
    let mut constants = [0.0; 11];
    constants[..9].copy_from_slice(matrix.as_flattened());
    constants[9..].copy_from_slice(&white_uv());
    Ok(constants)
}

/// Row-major `SIZE × SIZE` u′v′ bins centred on D65, with the RGB to XYZ
/// matrix and D65's u′v′ as constants.
static VECTORSCOPE: BinsBody = BinsBody {
    name: "drip-gui::vectorscope",
    constants: 11,
    bins: SIZE * SIZE,
    channels: 1,
    bin: kernel::chromaticity::expand,
};

mod kernel {
    use cubecl::prelude::*;
    use drip::body::{Bin, Pixel, Position};

    #[cube]
    fn positive(v: f32) -> f32 {
        if v > 0.0f32 { v } else { 0.0f32 }
    }

    /// The u′v′ bin of a pixel, or `SIZE · SIZE`, which counts nothing, for
    /// a pixel without chromaticity.
    #[cube]
    pub fn chromaticity(px: Pixel, _at: Position, k: &[f32]) -> Bin {
        let r = positive(px.r);
        let g = positive(px.g);
        let b = positive(px.b);
        // Normalize first: chromaticity is exposure independent and finite
        // HDR samples must not overflow XYZ or its denominator.
        let peak = f32::max(r, f32::max(g, b));
        let mut bin = 65536u32;
        if peak > 0.0 {
            let r = r / peak;
            let g = g / peak;
            let b = b / peak;
            let x = k[0] * r + k[1] * g + k[2] * b;
            let y = k[3] * r + k[4] * g + k[5] * b;
            let z = k[6] * r + k[7] * g + k[8] * b;
            let denominator = x + 15.0 * y + 3.0 * z;
            let u = 4.0 * x / denominator;
            let v = 9.0 * y / denominator;
            let bx = u32::cast_from(f32::clamp((0.5 + u - k[9]) * 256.0, 0.0, 255.0));
            let by = u32::cast_from(f32::clamp((0.5 - v + k[10]) * 256.0, 0.0, 255.0));
            bin = by * 256 + bx;
            // Exact neutrals belong at D65, including values at f32 extremes.
            if r == g && g == b {
                bin = 128u32 * 256 + 128;
            }
            // As in the host reference, undefined infinite chromaticities
            // enter the top-left bin. NaN and negative channels clip to zero.
            if peak > f32::MAX {
                bin = 0u32;
            }
        }
        Bin { c0: bin, c1: 0u32, c2: 0u32 }
    }
}

impl GuiNode for observe::Definition {
    type Presentation = Arc<Scope>;
    const PREPARE: Option<Prepare<Self>> =
        Some(Prepare::Outputs(|_, outputs, _| vectorscope(&outputs.counts)));
}

pub fn vectorscope(counts: &Read<'_, Counts>) -> Result<Arc<Scope>> {
    let matrix = color::rgb_to_xyz(REC2020, D65);
    let primaries = matrix::transpose(matrix).map(|primary| position(uv(primary)));
    Ok(scope(
        super::per_bin(counts)?,
        ScopeAxes::Vectorscope { primaries },
        true,
        counts.desc.coordinates.to_string(),
    ))
}

fn uv([x, y, z]: [f64; 3]) -> [f32; 2] {
    let denominator = x + 15.0 * y + 3.0 * z;
    [(4.0 * x / denominator) as f32, (9.0 * y / denominator) as f32]
}

fn white_uv() -> [f32; 2] {
    uv([D65[0], D65[1], 1.0 - D65[0] - D65[1]])
}

fn position([u, v]: [f32; 2]) -> [f32; 2] {
    let white = white_uv();
    [0.5 + (u - white[0]) / (2.0 * RADIUS), 0.5 - (v - white[1]) / (2.0 * RADIUS)]
}

/// CIE XYZ at Y = 1 for an occupied vectorscope bin's normalized position.
/// Inverts the u′v′ equations \[1\] so frontends share the scope's coordinates.
pub fn vectorscope_xyz([x, y]: [f32; 2]) -> [f64; 3] {
    let white = white_uv();
    let u = f64::from(white[0] + (x - 0.5) * (2.0 * RADIUS));
    let v = f64::from(white[1] - (y - 0.5) * (2.0 * RADIUS));
    [9.0 * u / (4.0 * v), 1.0, (12.0 - 3.0 * u - 20.0 * v) / (4.0 * v)]
}

#[cfg(test)]
fn vector_counts(pixels: &[[f32; 3]], matrix: &matrix::Mat3) -> Vec<[u32; 3]> {
    pixels
        .par_iter()
        .with_min_len(16384)
        .fold(
            || vec![[0u32; 3]; SIZE * SIZE],
            |mut counts, pixel| {
                let xyz = matrix::apply(matrix, pixel.map(|v| f64::from(v.max(0.0))));
                // Black has no chromaticity, so it must not create a neutral spike.
                if xyz[1] > 0.0 {
                    let [x, y] = position(uv(xyz)).map(|v| (v * SIZE as f32) as usize);
                    counts[y.min(SIZE - 1) * SIZE + x.min(SIZE - 1)][0] += 1;
                }
                counts
            },
        )
        .reduce(
            || vec![[0; 3]; SIZE * SIZE],
            |mut counts, partial| {
                for (a, b) in counts.iter_mut().zip(partial) {
                    a[0] += b[0];
                }
                counts
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
    fn device_vectorscope_preserves_neutrals_primaries_and_hdr() {
        use crate::node_ui::data::{HostImage, Placement};
        use crate::node_ui::scopes::tests::{context, vectorscope_of};
        let ctx = context();
        let mut pixels = vec![[0.18; 3]; 40000];
        pixels.extend([
            [0.0; 3],
            [-1.0; 3],
            [1.0, -0.2, 0.0],
            [4.0, 0.0, 0.0],
            [f32::MAX; 3],
            [f32::MIN_POSITIVE; 3],
        ]);
        let image = HostImage {
            width: pixels.len(),
            height: 1,
            plane: Placement::whole([pixels.len() as f32, 1.0]),
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: pixels.into(),
        };
        let counts = vectorscope_of(&image, &ctx).counts.clone();
        let expected = vector_counts(&image.pixels, &color::rgb_to_xyz(REC2020, D65));
        assert_eq!(counts, expected);
    }
    #[test]
    #[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
    fn device_vectorscope_rounding_stays_near_reference() {
        use crate::node_ui::data::{HostImage, Placement};
        use crate::node_ui::scopes::tests::{context, vectorscope_of};
        let ctx = context();
        let mut state = 42u32;
        let pixels: Vec<[f32; 3]> = (0..262144)
            .map(|_| {
                std::array::from_fn(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 0.1
                })
            })
            .collect();
        let image = HostImage {
            width: 512,
            height: 512,
            plane: Placement::whole([512.0, 512.0]),
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: pixels.into(),
        };
        let counts = vectorscope_of(&image, &ctx).counts.clone();
        let expected = vector_counts(&image.pixels, &color::rgb_to_xyz(REC2020, D65));
        assert_eq!(
            counts.iter().map(|v| u64::from(v[0])).sum::<u64>(),
            expected.iter().map(|v| u64::from(v[0])).sum::<u64>()
        );
        let moved =
            counts.iter().zip(&expected).map(|(a, b)| u64::from(a[0].abs_diff(b[0]))).sum::<u64>()
                / 2;
        eprintln!("vectorscope bins differing from f64 reference: {moved}/{}", image.pixels.len());
        assert!(moved < image.pixels.len() as u64 / 1000, "f32 bin-boundary drift: {moved}");
    }

    #[test]
    fn vectorscope_counts_neutrals_omits_black_and_clips_negative_channels() {
        let matrix = color::rgb_to_xyz(REC2020, D65);
        let mut pixels = vec![[0.18; 3]; 40000];
        pixels.extend([[0.0; 3], [-1.0; 3], [1.0, -0.2, 0.0], [4.0, 0.0, 0.0]]);
        let run = |workers| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()
                .unwrap()
                .install(|| vector_counts(&pixels, &matrix))
        };
        let counts = run(1);
        assert_eq!(counts, run(4));
        assert_eq!(counts.iter().flatten().sum::<u32>(), 40002);
        assert_eq!(counts[128 * SIZE + 128][0], 40000);
        assert_eq!(counts.iter().filter(|c| c[0] == 2).count(), 1);
        assert!(vector_counts(&[], &matrix).iter().all(|c| *c == [0; 3]));
    }

    #[test]
    fn chromaticity_matches_cie_coordinates_and_is_exposure_independent() {
        let matrix = color::rgb_to_xyz(REC2020, D65);
        let white = uv(matrix::apply(&matrix, [1.0; 3]));
        assert!((white[0] - 0.197830).abs() < 1e-6);
        assert!((white[1] - 0.468320).abs() < 1e-6);
        assert_eq!(position(white), [0.5; 2]);
        let red = uv(matrix::apply(&matrix, [1.0, 0.0, 0.0]));
        assert!((red[0] - 0.556604).abs() < 1e-6);
        assert!((red[1] - 0.516509).abs() < 1e-6);
        assert_eq!(red, uv(matrix::apply(&matrix, [4.0, 0.0, 0.0])));
    }
}
