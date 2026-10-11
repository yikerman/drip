use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
fn camera_to_rgb_contract(
    _: &GlobalContext,
    _: &(),
    image: Option<&ImageDesc<Camera>>,
    metadata: Option<&MetadataDesc>,
) -> Result<(Option<ImageDesc<Color>>,)> {
    let matrix = metadata.and_then(|m| m.matrix.as_ref());
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
/// Convert CameraRgb to ColorRgb with the matrix in CaptureMetadata.
/// The image interpretation must match the matrix source. Output uses the matrix's
/// target coordinates, without implicit decoding, normalization or clipping.
#[crate::node(id = "color.camera-to-color-rgb", name="Camera RGB to color RGB", category="Color", contract = camera_to_rgb_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<CameraRgb>>,
    metadata: Read<'_, Cpu<CaptureMetadata>>,
    output: Write<'_, Device<ColorRgb>>,
) -> Result<()> {
    let matrix = metadata
        .data
        .matrix
        .as_ref()
        .ok_or_else(|| Error::Contract("metadata has no camera color matrix".into()))?;
    let coefficients = DeviceBuffer::<()>::new(
        ctx.client()?.create(cubecl::bytes::Bytes::from_elems(matrix.values.to_vec())),
    );
    let (count, dim) = dispatch_dims(image.desc.extent.pixels());
    super::shared_kernel::matrix::launch(
        ctx.client()?,
        count,
        dim,
        image.data.argument(),
        coefficients.argument(),
        output.data.argument(),
    );
    Ok(())
}
