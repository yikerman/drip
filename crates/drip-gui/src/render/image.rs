//! Linear Rec.2020 images drawn through egui paint callbacks, so they sit in
//! egui's draw order (nodes and popups cover them) while keeping their full
//! gamut.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use drip::image::{ColorImage, Gpu, Rgb};
#[cfg(test)]
use drip::{compute::Compute, image::ImageError};
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};
use wgpu::util::DeviceExt;

/// Presentation storage stays in the producer's placement. Normal previews hold
/// the resident FP32 RGB buffer; CPU proofing results upload only when displayed.
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// Sensor pixels per image pixel along each axis.
    pub scale: u32,
    source: Source,
}

enum Source {
    Host(Arc<Rgb>),
    Resident(Arc<ColorImage<Gpu>>),
}

impl Image {
    pub fn new(rgb: &Arc<Rgb>) -> Self {
        Self {
            width: rgb.width,
            height: rgb.height,
            scale: rgb.scale,
            source: Source::Host(rgb.clone()),
        }
    }
    pub fn resident(image: Arc<ColorImage<Gpu>>) -> Self {
        Self {
            width: image.width(),
            height: image.height(),
            scale: image.scale(),
            source: Source::Resident(image),
        }
    }
    /// Explicit inspection boundary for tests and CPU consumers. Drawing itself
    /// never calls this method and never reads resident samples back to the host.
    #[cfg(test)]
    pub fn readback(&self, compute: &Compute) -> Result<Arc<Rgb>, ImageError> {
        match &self.source {
            Source::Host(image) => Ok(image.clone()),
            Source::Resident(image) => Ok(image.download(compute)?.rgb().clone()),
        }
    }
    #[cfg(test)]
    pub fn host_pixels(&self) -> Option<&Arc<Rgb>> {
        match &self.source {
            Source::Host(image) => Some(image),
            Source::Resident(_) => None,
        }
    }
}

/// GPU state shared by all previews, kept in egui's callback resources.
struct Previews {
    pipeline: wgpu::RenderPipeline,
    shown: HashMap<egui::Id, Shown>,
    /// Previews drawn this frame; the others are freed at its end.
    used: HashSet<egui::Id>,
    /// The render target's size in pixels, for the viewport.
    screen: [u32; 2],
}

/// One preview's storage bindings and placement. Holding the image keeps identity
/// comparisons by pointer sound.
struct Shown {
    image: Arc<Image>,
    groups: [wgpu::BindGroup; 2],
    rect: wgpu::Buffer,
}

pub(crate) const SHADER: &str =
    concat!(include_str!("shaders/color.wgsl"), include_str!("shaders/preview.wgsl"),);

pub fn install(device: &wgpu::Device, renderer: &mut egui_wgpu::Renderer) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("preview"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("preview"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let previews =
        Previews { pipeline, shown: HashMap::new(), used: HashSet::new(), screen: [1, 1] };
    renderer.callback_resources.insert(previews);
}

/// Frees previews not drawn this frame and returns the submitted images.
/// The caller retains these through queue completion so their compute-memory
/// reservations cover presentation work as well as numerical kernel work.
pub fn end_frame(renderer: &mut egui_wgpu::Renderer) -> Vec<Arc<Image>> {
    let previews: &mut Previews = renderer.callback_resources.get_mut().expect("installed");
    let used = std::mem::take(&mut previews.used);
    previews.shown.retain(|id, _| used.contains(id));
    previews.shown.values().map(|shown| shown.image.clone()).collect()
}

/// Draws `image` stretched over `rect`, clipped like any other shape; `id`
/// names the preview so its bindings are reused while the image is unchanged.
pub fn draw(
    painter: &egui::Painter,
    rect: egui::Rect,
    id: egui::Id,
    image: Arc<Image>,
    interpolation: bool,
) {
    // egui transforms the callback's rect with the painter's layer, but the
    // shader places the image from its own copy.
    let to_global = painter.ctx().layer_transform_to_global(painter.layer_id());
    let paint = Paint { id, rect: to_global.unwrap_or_default() * rect, image, interpolation };
    painter.add(egui_wgpu::Callback::new_paint_callback(rect, paint));
}

struct Paint {
    interpolation: bool,
    id: egui::Id,
    /// Where the image goes, in window points.
    rect: egui::Rect,
    image: Arc<Image>,
}

impl CallbackTrait for Paint {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let previews: &mut Previews = resources.get_mut().expect("installed");
        previews.screen = screen.size_in_pixels;
        previews.used.insert(self.id);
        if !previews.shown.get(&self.id).is_some_and(|s| Arc::ptr_eq(&s.image, &self.image)) {
            let shown = upload(device, queue, previews, &self.image);
            previews.shown.insert(self.id, shown);
        }
        // The rect in normalized device coordinates, y up.
        let [w, h] = screen.size_in_pixels.map(|v| v as f32);
        let (min, max) =
            (self.rect.min * screen.pixels_per_point, self.rect.max * screen.pixels_per_point);
        let ndc = [
            min.x / w * 2.0 - 1.0,
            1.0 - min.y / h * 2.0,
            max.x / w * 2.0 - 1.0,
            1.0 - max.y / h * 2.0,
        ];
        let bytes: Vec<u8> = ndc.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&previews.shown[&self.id].rect, 0, &bytes);
        vec![]
    }

    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let previews: &Previews = resources.get().expect("installed");
        // egui narrows the viewport to the visible part of the rect; the
        // shader places the image itself and the scissor does the clipping.
        let [w, h] = previews.screen.map(|v| v as f32);
        pass.set_viewport(0.0, 0.0, w, h, 0.0, 1.0);
        pass.set_pipeline(&previews.pipeline);
        pass.set_bind_group(
            0,
            &previews.shown[&self.id].groups[usize::from(self.interpolation)],
            &[],
        );
        pass.draw(0..6, 0..1);
    }
}

fn upload(
    device: &wgpu::Device,
    _queue: &wgpu::Queue,
    previews: &Previews,
    image: &Arc<Image>,
) -> Shown {
    let samples = match &image.source {
        Source::Resident(source) => source.gpu_buffer().raw().clone(),
        Source::Host(source) => {
            let bytes: Vec<u8> =
                source.pixels.as_flattened().iter().flat_map(|v| v.to_ne_bytes()).collect();
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("proof preview samples"),
                contents: &bytes,
                usage: wgpu::BufferUsages::STORAGE,
            })
        }
    };
    let rect = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("preview rect"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let groups = [0u32, 1].map(|interpolate| {
        let info: Vec<u8> = [image.width as u32, image.height as u32, interpolate, 0]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        let info = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("preview dimensions and sampling"),
            contents: &info,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview"),
            layout: &previews.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: samples.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: info.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: rect.as_entire_binding() },
            ],
        })
    });
    Shown { image: image.clone(), groups, rect }
}
