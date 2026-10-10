use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
type CameraMatrix = Matrix3<Camera, Color>;
fn camera_to_rgb_contract(
    _: &GlobalContext,
    _: &(),
    image: Option<&ImageDesc<Camera>>,
    matrix: Option<&MatrixDesc<Camera, Color>>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    if matrix.is_some_and(|m| m.target.encoding != Encoding::Identity) {
        return Err(Error::Contract("camera matrix output must declare identity encoding".into()));
    }
    let (Some(image), Some(matrix)) = (image, matrix) else {
        return Ok((None,));
    };
    if image.interpretation != matrix.source {
        return Err(Error::Contract(format!(
            "image domain {:?} differs from matrix source {:?}",
            image.interpretation, matrix.source
        )));
    }
    Ok((Some(ImageDesc { extent: image.extent.clone(), interpretation: matrix.target.clone() }),))
}
/// Convert CameraRgb to ColorRgb with the supplied CameraMatrix.
/// The image interpretation must match the matrix source. Output uses the matrix's
/// target coordinates, without implicit decoding, normalization or clipping.
#[crate::node(id = "color.camera-to-color-rgb", name="Camera RGB to color RGB", category="Color", contract = camera_to_rgb_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<CameraRgb>>,
    matrix: Read<'_, Device<CameraMatrix>>,
    output: Write<'_, Device<ColorRgb>>,
) -> Result<()> {
    let (count, dim) = dispatch_dims(image.desc.extent.pixels());
    super::shared_kernel::matrix::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        matrix.data.argument(),
        output.data.argument(),
    );
    Ok(())
}
