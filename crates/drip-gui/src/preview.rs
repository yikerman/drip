//! Linear Rec.2020 images drawn through egui paint callbacks, so they sit in
//! egui's draw order (nodes and popups cover them) while keeping their full
//! gamut.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::worker::Image;
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};

/// GPU state shared by all previews, kept in egui's callback resources.
struct Previews {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    shown: HashMap<egui::Id, Shown>,
    /// Previews drawn this frame; the others are freed at its end.
    used: HashSet<egui::Id>,
    /// The render target's size in pixels, for the viewport.
    screen: [u32; 2],
}

/// One preview's texture and placement. Holding the image keeps identity
/// comparisons by pointer sound.
struct Shown {
    image: Arc<Image>,
    group: wgpu::BindGroup,
    rect: wgpu::Buffer,
}

pub fn install(device: &wgpu::Device, renderer: &mut egui_wgpu::Renderer) {
    let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/preview.wgsl"));
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
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let previews =
        Previews { pipeline, sampler, shown: HashMap::new(), used: HashSet::new(), screen: [1, 1] };
    renderer.callback_resources.insert(previews);
}

/// Frees the previews not drawn since the last call; called once per frame.
pub fn end_frame(renderer: &mut egui_wgpu::Renderer) {
    let previews: &mut Previews = renderer.callback_resources.get_mut().expect("installed");
    let used = std::mem::take(&mut previews.used);
    previews.shown.retain(|id, _| used.contains(id));
}

/// A shape drawing `image` stretched over `rect`, clipped like any other
/// shape; `id` names the preview so its texture is reused while the image is
/// unchanged.
pub fn shape(rect: egui::Rect, id: egui::Id, image: Arc<Image>) -> egui::Shape {
    egui_wgpu::Callback::new_paint_callback(rect, Paint { id, rect, image }).into()
}

struct Paint {
    id: egui::Id,
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
        pass.set_bind_group(0, &previews.shown[&self.id].group, &[]);
        pass.draw(0..6, 0..1);
    }
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    previews: &Previews,
    image: &Arc<Image>,
) -> Shown {
    let size = wgpu::Extent3d {
        width: image.width as u32,
        height: image.height as u32,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("preview"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &image.texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.width * 8),
            rows_per_image: None,
        },
        size,
    );
    let rect = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("preview rect"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let view = texture.create_view(&Default::default());
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("preview"),
        layout: &previews.pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&previews.sampler),
            },
            wgpu::BindGroupEntry { binding: 2, resource: rect.as_entire_binding() },
        ],
    });
    Shown { image: image.clone(), group, rect }
}
