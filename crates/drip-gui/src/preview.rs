//! Linear Rec.2020 images drawn through egui paint callbacks, so they sit in
//! egui's draw order (popups cover them) while keeping their full gamut.

use std::collections::HashMap;
use std::sync::Arc;

use drip::value::Rgb;
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};

/// GPU state shared by all previews, kept in egui's callback resources.
struct Previews {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// The image each preview last showed and its texture; holding the image
    /// keeps identity comparisons by pointer sound.
    uploaded: HashMap<egui::Id, (Arc<Rgb>, wgpu::BindGroup)>,
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
    renderer.callback_resources.insert(Previews { pipeline, sampler, uploaded: HashMap::new() });
}

/// A shape drawing `image` stretched over `rect`; `id` names the preview so its
/// texture is reused while the image is unchanged.
pub fn shape(rect: egui::Rect, id: egui::Id, image: Arc<Rgb>) -> egui::Shape {
    egui_wgpu::Callback::new_paint_callback(rect, Paint { id, image }).into()
}

struct Paint {
    id: egui::Id,
    image: Arc<Rgb>,
}

impl CallbackTrait for Paint {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _: &ScreenDescriptor,
        _: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let previews: &mut Previews = resources.get_mut().expect("installed");
        if previews.uploaded.get(&self.id).is_some_and(|(image, _)| Arc::ptr_eq(image, &self.image))
        {
            return vec![];
        }
        let image = &self.image;
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
        let texels: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 1.0])
            .flat_map(|v| half::f16::from_f32(v).to_ne_bytes())
            .collect();
        queue.write_texture(
            texture.as_image_copy(),
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 8),
                rows_per_image: None,
            },
            size,
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview"),
            layout: &previews.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&previews.sampler),
                },
            ],
        });
        previews.uploaded.insert(self.id, (self.image.clone(), group));
        vec![]
    }

    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let previews: &Previews = resources.get().expect("installed");
        pass.set_pipeline(&previews.pipeline);
        pass.set_bind_group(0, &previews.uploaded[&self.id].1, &[]);
        pass.draw(0..3, 0..1);
    }
}
