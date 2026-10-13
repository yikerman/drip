use crate::{
    Error, Result,
    node::{camera, color, data::*},
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use compute::{Definition, Ports, add, definition};
use std::sync::Arc;
fn camera_to_rgb_contract(
    _: &GlobalContext,
    _: &(),
    image: &RgbDesc,
    metadata: &Arc<drip_raw::Metadata>,
) -> Result<(RgbDesc,)> {
    let response = image.response()?;
    if response.camera != Camera::of(metadata) {
        return Err(Error::Contract("image camera differs from the capture metadata".into()));
    }
    camera::matrix(metadata, response.balance)?;
    Ok((RgbDesc { coordinates: Coordinates::Space(REC2020_D65), ..image.clone() },))
}
/// Convert camera RGB to linear Rec.2020/D65 with the capture's camera matrix.
/// The image's balance is replaced by D65 reference gains inside the matrix, so
/// camera neutral under D65 maps to white; scene white balance is a separate
/// adaptation. No clipping or normalization is applied. The image must come
/// from the same camera make and model as the metadata.
#[crate::node(id = "camera-to-color-rgb", name="Camera RGB to color RGB", tags = [tag::INPUT_TRANSFORM, tag::COLOR], contract = camera_to_rgb_contract)]
pub fn compute(
    ctx: &KernelContext<'_>,
    _: &(),
    image: Read<'_, Device<Rgb>, CameraRgb>,
    metadata: Read<'_, Cpu<CaptureMetadata>>,
    output: Write<'_, Device<Rgb>, Rec2020>,
) -> Result<()> {
    let matrix = camera::matrix(metadata.data, image.desc.response()?.balance)?;
    let coefficients = ctx.constants(color::to_f32(&matrix).as_flattened())?;
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
