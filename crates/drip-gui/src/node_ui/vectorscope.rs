//! Photographic scopes: spatial RGB exposure and exposure-independent chromaticity.
//!
//! \[1\] CIE, "CIE 1976 uniform-chromaticity-scale diagram," e-ILV,
//!     term 17-23-073. <https://cie.co.at/eilvterm/17-23-073>
//! The vectorscope uses u′v′ directly, not darktable's lightness-scaled u*v*.
//! Negative RGB components are clipped only for chromaticity visualization.

use super::data::{DeviceRgb, PrepareContext};
#[cfg(test)]
use super::scopes::SIZE;
use super::scopes::{Scope, ScopeAxes, scope};
use drip::Error as KernelError;
use drip::node::color::{self, D65, REC2020};
use drip::{node::data::*, ports::*, runtime::KernelContext};
#[cfg(test)]
use rayon::prelude::*;
use std::sync::Arc;
const RADIUS: f32 = 0.5;
fn vector_contract(
    _: &drip::runtime::GlobalContext,
    _: &(),
    image: Option<&ImageDesc<Color>>,
) -> drip::Result<()> {
    super::contracts::working(image)
}

/// Plot identity-encoded Rec.2020/D65 ColorRgb chromaticity in CIE 1976 u′v′.
/// Requires relative white 1. Negative channels are clipped for visualization;
/// black has no chromaticity and is omitted. Density uses logarithmic counts.
#[drip::node(id="view.vectorscope", name="Vectorscope", category="View", contract=vector_contract)]
fn observe(_: &KernelContext<'_>, _: &(), image: Read<'_, Device<ColorRgb>>) -> drip::Result<()> {
    let _ = image;
    Ok(())
}
pub fn vectorscope(
    _: (),
    (image,): (&DeviceRgb<Color>,),
    ctx: &PrepareContext,
) -> Result<Arc<Scope>, KernelError> {
    let matrix = &color::rgb_to_xyz(REC2020, D65);
    let counts = super::scopes::device::vectorscope(image, ctx, matrix, white_uv())?;
    let primaries = color::transpose(*matrix).map(|primary| position(uv(primary)));
    Ok(scope(
        counts,
        ScopeAxes::Vectorscope { primaries, color_space: "Rec.2020" },
        true,
        super::data::DisplayInterpretation::label(&image.desc.interpretation),
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
fn vector_counts(pixels: &[[f32; 3]], matrix: &color::Mat3) -> Vec<[u32; 3]> {
    pixels
        .par_iter()
        .with_min_len(16384)
        .fold(
            || vec![[0u32; 3]; SIZE * SIZE],
            |mut counts, pixel| {
                let xyz = color::apply(matrix, pixel.map(|v| f64::from(v.max(0.0))));
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

struct VectorscopeGui;
#[drip_macros::gui_node]
impl crate::node_ui::GuiNode for VectorscopeGui {
    type Parameters = ();
    type Presentation = Arc<Scope>;
    const ID: &'static str = "view.vectorscope";
    const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|p, inputs, ctx| {
        vectorscope(p, (&ctx.device_image::<drip::node::data::Color>(inputs)?,), ctx)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
    fn device_vectorscope_preserves_neutrals_primaries_and_hdr() {
        use crate::node_ui::data::Rgb;
        use crate::node_ui::scopes::tests::{context, device};
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
        let image = Rgb {
            width: pixels.len(),
            height: 1,
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: Arc::new(pixels.into()),
        };
        let counts = vectorscope((), (&device(&image, &ctx),), &ctx).unwrap().counts.clone();
        let expected = vector_counts(&image.pixels, &color::rgb_to_xyz(REC2020, D65));
        assert_eq!(counts, expected);
    }
    #[test]
    #[ignore = "requires the DRIP_BACKEND compute device; run explicitly"]
    fn device_vectorscope_rounding_stays_near_reference() {
        use crate::node_ui::data::Rgb;
        use crate::node_ui::scopes::tests::{context, device};
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
        let image = Rgb {
            width: 512,
            height: 512,
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: Arc::new(pixels.into()),
        };
        let counts = vectorscope((), (&device(&image, &ctx),), &ctx).unwrap().counts.clone();
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
        let white = uv(color::apply(&matrix, [1.0; 3]));
        assert!((white[0] - 0.197830).abs() < 1e-6);
        assert!((white[1] - 0.468320).abs() < 1e-6);
        assert_eq!(position(white), [0.5; 2]);
        let red = uv(color::apply(&matrix, [1.0, 0.0, 0.0]));
        assert!((red[0] - 0.556604).abs() < 1e-6);
        assert!((red[1] - 0.516509).abs() < 1e-6);
        assert_eq!(red, uv(color::apply(&matrix, [4.0, 0.0, 0.0])));
    }
}
