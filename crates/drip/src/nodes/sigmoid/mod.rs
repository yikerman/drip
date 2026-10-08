//! Rendering in specified additive Rec.2020 coordinates.

use crate::image::{ColorCoordinates, ColorImage, Gpu};
use crate::node::{EvalContext, KernelError};
use crate::param::ParamKind;
use std::sync::Arc;

mod algorithm;
pub use algorithm::{GREY, Sigmoid};

#[derive(crate::Parameters)]
pub struct Settings {
    #[param(ParamKind::Float { min: 0.5, max: 4.0, default: 1.5 })]
    contrast: f32,
    #[param(ParamKind::Float { min: -1.0, max: 1.0, default: -0.2 })]
    skew: f32,
    #[param(ParamKind::Float { min: 0.0, max: 1.0, default: 0.0 })]
    preserve_hue: f32,
}
impl Settings {
    pub fn curve(&self) -> Sigmoid {
        Sigmoid::new(self.contrast, self.skew, self.preserve_hue)
    }
}

/// Render ColorImage with a per-channel sigmoid and optional hue preservation.
///
/// Requires additive Rec.2020/D65 coordinates with reference white Y=1. The curve
/// fixes neutral 0.18, maps black to zero and approaches unit white. Output still
/// denotes additive Rec.2020 colors but is marked rendered: its numerical values
/// no longer claim proportionality to the original scene. Already rendered
/// input is permitted for creative intent; rendering is not required for export.
#[crate::node(kind = SIGMOID, id = "tone.sigmoid", category = "tone", name = "Sigmoid", outputs = ["image"], references = [("darktable: sigmoid", "https://docs.darktable.org/usermanual/5.6/en/module-reference/processing-modules/sigmoid/")])]
fn sigmoid(
    #[params] p: Settings,
    image: &ColorImage<Gpu>,
    #[context] ctx: &EvalContext<'_>,
) -> Result<(Arc<ColorImage<Gpu>>,), KernelError> {
    if image.interpretation().coordinates != ColorCoordinates::LinearRec2020 {
        return Err(KernelError::Failed(
            "sigmoid requires additive Rec.2020/D65 coordinates".into(),
        ));
    }
    let compute = ctx.compute()?;
    let output = compute.allocate_f32(image.gpu_buffer().len())?;
    let parameters = compute.upload_f32(&p.curve().parameters())?;
    compute.dispatch(
        "sigmoid",
        include_str!("../shaders/sigmoid.wgsl"),
        "sigmoid",
        &[image.gpu_buffer(), &output, &parameters],
        super::gpu::groups(compute, image.width() * image.height()),
    )?;
    Ok((Arc::new(ColorImage::from_gpu(
        output,
        image.width(),
        image.height(),
        image.scale(),
        image.interpretation().after_rendering(),
    )?),))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_matches_scalar_reference_for_hdr_negatives_and_hue_correction() {
        let compute = crate::nodes::gpu::test_compute();
        let pixels: Vec<_> = (0..4099)
            .map(|i| [i as f32 / 1024.0 - 0.1, 0.18, 0.5])
            .chain([
                [0.0; 3],
                [-1.0; 3],
                [0.18; 3],
                [10000.0, 0.01, 0.0],
                [0.1, -0.2, 1.0],
                [f32::MAX; 3],
                [f32::MAX, 0.0, 1.0],
                [-f32::MAX, f32::MAX, f32::MAX],
            ])
            .collect();
        let input = compute.upload_f32(pixels.as_flattened()).unwrap();
        for contrast in [0.5, 1.5, 4.0] {
            for skew in [-1.0, 0.0, 1.0] {
                for hue in [0.0, 0.5, 1.0] {
                    let curve = Sigmoid::new(contrast, skew, hue);
                    let parameters = compute.upload_f32(&curve.parameters()).unwrap();
                    let output = compute.allocate_f32(pixels.len() * 3).unwrap();
                    compute
                        .dispatch(
                            "sigmoid",
                            include_str!("../shaders/sigmoid.wgsl"),
                            "sigmoid",
                            &[&input, &output, &parameters],
                            crate::nodes::gpu::groups(compute, pixels.len()),
                        )
                        .unwrap();
                    let actual = compute.read_f32(&output).unwrap();
                    let expected = curve.process(&pixels);
                    for (i, (&a, &e)) in actual.iter().zip(expected.as_flattened()).enumerate() {
                        assert!(
                            (a - e).abs() < 2e-5,
                            "contrast={contrast} skew={skew} hue={hue} sample={i}: {a} != {e}"
                        );
                    }
                }
            }
        }
    }
}
