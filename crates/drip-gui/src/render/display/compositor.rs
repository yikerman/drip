//! The encoded egui canvas and its conversion to the presentation encoding.

use super::{CANVAS, presentation::Output};

pub(super) const SHADER: &str =
    concat!(include_str!("../shaders/color.wgsl"), include_str!("../shaders/presentation.wgsl"),);

pub(super) struct Compositor {
    pub canvas: wgpu::TextureView,
    composite: wgpu::RenderPipeline,
    composite_group: wgpu::BindGroup,
}

impl Compositor {
    pub fn new(device: &wgpu::Device, output: Output, config: &wgpu::SurfaceConfiguration) -> Self {
        let composite =
            crate::render::pipeline(device, "composite", SHADER, output.fragment(), output.format);
        let canvas = Self::canvas(device, config);
        let composite_group = Self::composite_group(device, &composite, &canvas);
        Self { canvas, composite, composite_group }
    }

    fn canvas(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("canvas"),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: CANVAS,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    pub(super) fn composite_group(
        device: &wgpu::Device,
        pipeline: &wgpu::RenderPipeline,
        canvas: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(canvas),
            }],
        })
    }

    pub fn resize(&mut self, device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) {
        self.canvas = Self::canvas(device, config);
        self.composite_group = Self::composite_group(device, &self.composite, &self.canvas);
    }

    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &self.composite_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
