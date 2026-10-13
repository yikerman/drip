//! Images prepared for GUI views. Processing payloads remain in `drip`.
use drip::{
    Result,
    node::data::{DeviceBuffer, HostBuffer, Rgb, RgbDesc},
    ports::Read,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
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
    /// Shares the evaluated host pixels, downloading them first if they are
    /// on the device; `requested_scale` is the preview request.
    pub fn new<R>(image: &Read<'_, Rgb, R>, requested_scale: u32) -> Result<Self> {
        Ok(Self {
            width: image.desc.extent.width as usize,
            height: image.desc.extent.height as usize,
            requested_scale,
            color_space: image.desc.coordinates.to_string(),
            pixels: image.data.host()?.clone(),
        })
    }
}

pub struct PrepareContext {
    /// The evaluator's runtime, which owns the inputs' device samples.
    pub runtime: RuntimeContext,
    /// The demand's context: the preview's, or full detail for actions.
    pub global: GlobalContext,
}
impl Default for PrepareContext {
    fn default() -> Self {
        Self { runtime: RuntimeContext::host(), global: Default::default() }
    }
}
/// Device RGB retained for GUI-owned scope kernels.
pub struct DeviceRgb {
    pub desc: RgbDesc,
    pub pixels: DeviceBuffer<Rgb>,
}
impl DeviceRgb {
    /// Shares the evaluated device pixels, uploading them first if they are on the host.
    pub fn new<R>(image: &Read<'_, Rgb, R>, ctx: &PrepareContext) -> Result<Self> {
        Ok(Self { desc: image.desc.clone(), pixels: image.data.device(&ctx.kernels())?.clone() })
    }
}

impl PrepareContext {
    pub fn client(&self) -> Result<&cubecl::prelude::Client> {
        self.runtime.client()
    }
    /// For running drip kernels and reading device samples on the runtime.
    pub fn kernels(&self) -> KernelContext<'_> {
        KernelContext::new(&self.global, &self.runtime)
    }
}

/// A GUI kernel's scratch buffer as an argument. Every such buffer holds
/// 4-byte scalars (f32 or u32), matching the kernels' slices.
pub(crate) fn argument(handle: &cubecl::server::Handle) -> cubecl::prelude::BufferArg {
    // SAFETY: the buffer's element size is one scalar, as documented above.
    unsafe { cubecl::prelude::BufferArg::from_raw_parts(handle.clone(), 1) }
}
