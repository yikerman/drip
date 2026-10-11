//! Images prepared for GUI views. Processing payloads remain in `drip`.
use drip::{
    Error, Result,
    node::data::{DeviceBuffer, HostBuffer, Rgb, RgbDesc},
    ports::{Cpu, Device, Read},
    resource::Resources,
};

/// Host pixels prepared for presentation, with the labels views show.
#[derive(Debug, Clone, PartialEq)]
pub struct HostImage {
    pub width: usize,
    pub height: usize,
    pub requested_scale: u32,
    pub color_space: String,
    pub pixels: HostBuffer<[f32; 3]>,
}
impl HostImage {
    /// Shares the evaluated pixels; `requested_scale` is the preview request.
    pub fn new<R>(image: &Read<'_, Cpu<Rgb>, R>, requested_scale: u32) -> Self {
        Self {
            width: image.desc.extent.width as usize,
            height: image.desc.extent.height as usize,
            requested_scale,
            color_space: image.desc.coordinates.to_string(),
            pixels: image.data.clone(),
        }
    }
}

#[derive(Default)]
pub struct PrepareContext {
    pub client: Option<cubecl::prelude::Client>,
    pub resources: Resources,
    pub level: u8,
}
/// Device RGB retained for GUI-owned scope kernels.
pub struct DeviceRgb {
    pub desc: RgbDesc,
    pub pixels: DeviceBuffer<Rgb>,
}
impl DeviceRgb {
    pub fn new<R>(image: &Read<'_, Device<Rgb>, R>) -> Self {
        Self { desc: image.desc.clone(), pixels: image.data.clone() }
    }
}

impl PrepareContext {
    pub fn client(&self) -> Result<&cubecl::prelude::Client> {
        self.client.as_ref().ok_or_else(|| Error::Runtime("no compute runtime is bound".into()))
    }

    pub fn requested_scale(&self) -> u32 {
        1 << self.level
    }
}
