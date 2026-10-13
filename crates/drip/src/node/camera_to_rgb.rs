use crate::{
    Error, Result,
    body::matrix,
    node::{camera, data::*},
    ports::*,
    registry::tag,
    runtime::GlobalContext,
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
/// Convert camera RGB to linear Rec.2020 with the camera's color matrix from
/// the metadata. White balance happens in Chromatic adaptation, after this
/// node. Values are not clipped. The metadata must come from the same camera
/// as the image.
// The matrix replaces the image's balance with D65 reference gains, so camera
// neutral under D65 maps to white and scene white balance stays a separate
// adaptation.
#[crate::node(id = "camera-to-color-rgb", name="Camera RGB to color RGB", tags = [tag::INPUT_TRANSFORM, tag::COLOR], contract = camera_to_rgb_contract, map = super::shared_kernel::matrix)]
pub fn compute(
    _: &(),
    image: Point<'_, Rgb, CameraRgb>,
    metadata: Read<'_, CaptureMetadata>,
    _output: Point<'_, Rgb, Rec2020>,
) -> Result<[f32; 9]> {
    let to_rgb = camera::matrix(metadata.data.host()?, image.desc.response()?.balance)?;
    Ok(bytemuck::cast(matrix::to_f32(&to_rgb)))
}
