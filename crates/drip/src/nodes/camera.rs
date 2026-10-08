//! Camera characterization to additive Rec.2020 coordinates.

use crate::color::{self, D65, REC2020};
use crate::image::{CameraRgb, ColorImage, ColorMeaning, Gpu};
use crate::node::{EvalContext, KernelError};
use std::sync::Arc;

/// Transform CameraRgb to ColorImage in additive Rec.2020/D65 coordinates.
///
/// Requires a usable camera characterization for the supplied response basis.
/// The matrix is normalized so neutral camera RGB (1, 1, 1) stays neutral. This
/// assigns represented color coordinates; it does not certify scene accuracy,
/// recover clipped measurements, or establish a sensor noise model.
#[crate::node(kind = CAMERA_TO_REC2020, id = "color.camera_to_rec2020", category = "color", name = "Camera to Rec.2020", outputs = ["image"])]
fn camera_to_rec2020(
    image: &CameraRgb<Gpu>,
    #[context] ctx: &EvalContext<'_>,
) -> Result<(Arc<ColorImage<Gpu>>,), KernelError> {
    let matrix = color::camera_to_rgb(
        &color::to_f64(&image.interpretation().xyz_to_cam),
        &color::rgb_to_xyz(REC2020, D65),
    )
    .ok_or_else(|| KernelError::Failed("camera characterization matrix is degenerate".into()))?;
    let matrix = color::to_f32(&matrix);
    if matrix.iter().flatten().any(|v| !v.is_finite()) {
        return Err(KernelError::Failed("camera conversion coefficients exceed f32 range".into()));
    }
    let output = super::gpu::pointwise(
        ctx.compute()?,
        "matrix",
        image.gpu_buffer(),
        image.gpu_buffer().len(),
        image.width() * image.height(),
        matrix.as_flattened(),
    )?;
    Ok((Arc::new(ColorImage::from_gpu(
        output,
        image.width(),
        image.height(),
        image.scale(),
        ColorMeaning::rec2020(),
    )?),))
}
