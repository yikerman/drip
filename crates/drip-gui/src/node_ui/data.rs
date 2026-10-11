//! Images prepared for GUI views. Processing payloads remain in `drip`.
use drip::{
    Error, Result,
    node::data::{DeviceBuffer, HostBuffer, Rgb, RgbDesc},
    ports::{Cpu, Device, Read},
    runtime::GlobalContext,
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
    /// The demand's context: the preview's, or full detail for actions.
    pub global: GlobalContext,
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
}

/// A GUI kernel's scratch buffer as an argument. Every such buffer holds
/// 4-byte scalars (f32 or u32), matching the kernels' slices.
pub(crate) fn argument(handle: &cubecl::server::Handle) -> cubecl::prelude::BufferArg {
    // SAFETY: the buffer's element size is one scalar, as documented above.
    unsafe { cubecl::prelude::BufferArg::from_raw_parts(handle.clone(), 1) }
}
