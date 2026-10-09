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
/// Apply a row-major Camera-to-Color matrix, supplied on its own reusable port.
/// The CameraRgb interpretation must match the matrix source. Produces ColorRgb
/// in the declared target coordinates; no implicit decoding or clipping.
#[crate::node(id = "camera-to-rgb-matrix", name="Camera to RGB", category="color", contract = camera_to_rgb_contract)]
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
