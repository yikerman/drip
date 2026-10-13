//! Images prepared for GUI views. Processing payloads remain in `drip`.
use drip::{
    Result,
    node::data::{HostBuffer, Rgb},
    ports::Read,
    runtime::{GlobalContext, KernelContext, RuntimeContext},
    shape::MapPayload,
};

/// Host pixels prepared for presentation, with the labels views show.
#[derive(Debug, Clone, PartialEq)]
pub struct HostImage {
    pub width: usize,
    pub height: usize,
    pub requested_scale: u32,
    pub color_space: String,
    pub pixels: HostBuffer<[f32; 3]>,
    pub plane: Placement,
}
impl HostImage {
    /// Shares the evaluated host pixels, downloading them first if they are
    /// on the device; `requested_scale` is the preview request.
    pub fn new<R>(image: &Read<'_, Rgb, R>, requested_scale: u32) -> Result<Self> {
        let lattice = Rgb::lattice(image.desc);
        let step: [f32; 2] =
            std::array::from_fn(|a| lattice.full[a] as f32 / lattice.extent[a] as f32);
        let corner = |samples: [u32; 2]| std::array::from_fn(|a| samples[a] as f32 * step[a]);
        Ok(Self {
            width: image.desc.extent.width as usize,
            height: image.desc.extent.height as usize,
            requested_scale,
            color_space: image.desc.coordinates.to_string(),
            pixels: image.data.host()?.clone(),
            plane: Placement {
                full: lattice.full.map(|v| v as f32),
                covers: [corner(lattice.window.origin), corner(lattice.window.end())],
            },
        })
    }
}

/// Where an image's pixels sit on its full-detail plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The whole image's size in full-detail pixels.
    pub full: [f32; 2],
    /// The rectangle the pixels cover, from its minimum to its maximum
    /// corner, in full-detail pixels. The whole plane unless the image is a
    /// crop.
    pub covers: [[f32; 2]; 2],
}
impl Placement {
    /// The whole plane of an image `full` pixels in size.
    pub fn whole(full: [f32; 2]) -> Self {
        Self { full, covers: [[0.0; 2], full] }
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
impl PrepareContext {
    /// For running drip kernels and reading device samples on the runtime.
    pub fn kernels(&self) -> KernelContext<'_> {
        KernelContext::new(&self.global, &self.runtime)
    }
}
