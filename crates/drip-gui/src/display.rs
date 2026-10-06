//! Each window composites egui and previews in an FP16 extended-sRGB canvas.
//! Presentation decodes it to extended linear BT.709, with sRGB as fallback.
//! The driver declares the surface color space and the compositor maps it to
//! the display. Rendering intent is driver-controlled.

mod output;
#[cfg(test)]
mod tests;

use output::Output;

use std::sync::Arc;

use winit::window::Window;

use crate::preview;

const CANVAS: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The GPU every window draws with.
#[derive(Clone)]
pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    /// Picks an adapter that can present to `window` and sets up the window's
    /// display; the app's other windows are on the same display.
    pub fn new(window: Arc<Window>) -> Result<(Self, Display), String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let gpu = Gpu { instance, adapter, device, queue };
        let display = Display::with_surface(&gpu, window, surface)?;
        Ok((gpu, display))
    }

    pub fn max_texture_side(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }
}

/// One window's output.
pub struct Display {
    gpu: Gpu,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Whether the output preserves extended RGB coordinates.
    pub wide_gamut: bool,
    canvas: wgpu::TextureView,
    composite: wgpu::RenderPipeline,
    composite_group: wgpu::BindGroup,
    pub egui: egui_wgpu::Renderer,
}

impl Display {
    pub fn new(gpu: &Gpu, window: Arc<Window>) -> Result<Self, String> {
        let surface = gpu.instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        Self::with_surface(gpu, window, surface)
    }

    fn with_surface(
        gpu: &Gpu,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
    ) -> Result<Self, String> {
        let Gpu { adapter, device, .. } = gpu;
        let size = window.inner_size();
        let caps = surface.get_capabilities(adapter);

        let output = Output::choose(&caps).ok_or("no usable surface color space")?;
        let Output { format, color_space } = output;
        if !output.wide_gamut() {
            log::warn!("the display offers no extended-sRGB output; using sRGB");
        }
        log::info!("presenting {format:?} in {color_space:?} on {}", adapter.get_info().name);
        let config = wgpu::SurfaceConfiguration {
            format,
            color_space,
            // The default is the driver's first mode, which may not wait for
            // vsync; frames beyond the refresh rate are never seen.
            present_mode: wgpu::PresentMode::Fifo,
            ..surface
                .get_default_config(adapter, size.width.max(1), size.height.max(1))
                .ok_or("unsupported surface")?
        };
        surface.configure(device, &config);

        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/composite.wgsl"));
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(output.fragment()),
                targets: &[Some(format.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let canvas = Self::canvas(device, &config);
        let composite_group = Self::composite_group(device, &composite, &canvas);
        let mut egui =
            egui_wgpu::Renderer::new(device, CANVAS, egui_wgpu::RendererOptions::default());
        preview::install(device, &mut egui);
        Ok(Display {
            gpu: gpu.clone(),
            window,
            surface,
            config,
            wide_gamut: output.wide_gamut(),
            canvas,
            composite,
            composite_group,
            egui,
        })
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

    fn composite_group(
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

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        (self.config.width, self.config.height) = (width, height);
        self.surface.configure(&self.gpu.device, &self.config);
        self.canvas = Self::canvas(&self.gpu.device, &self.config);
        self.composite_group =
            Self::composite_group(&self.gpu.device, &self.composite, &self.canvas);
    }

    /// Draws one frame. Texture changes are applied even if the frame is
    /// skipped, since later frames build on them.
    pub fn render(
        &mut self,
        jobs: &[egui::ClippedPrimitive],
        mut textures: egui::TexturesDelta,
        pixels_per_point: f32,
    ) {
        for (id, deltas) in textures.set.drain() {
            for delta in deltas {
                self.egui.update_texture(&self.gpu.device, &self.gpu.queue, id, &delta);
            }
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("the surface was lost; recreating it");
                match self.gpu.instance.create_surface(self.window.clone()) {
                    Ok(surface) => self.surface = surface,
                    Err(e) => log::error!("cannot recreate the surface: {e}"),
                }
                self.surface.configure(&self.gpu.device, &self.config);
                None
            }
            other => {
                log::debug!("skipping a frame: {other:?}");
                self.surface.configure(&self.gpu.device, &self.config);
                None
            }
        };
        if let Some(frame) = frame {
            self.draw(jobs, frame, pixels_per_point);
        }
        for id in textures.free.drain() {
            self.egui.free_texture(&id);
        }
    }

    fn draw(
        &mut self,
        jobs: &[egui::ClippedPrimitive],
        frame: wgpu::SurfaceTexture,
        pixels_per_point: f32,
    ) {
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point,
        };
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let mut commands = self.egui.update_buffers(
            &self.gpu.device,
            &self.gpu.queue,
            &mut encoder,
            jobs,
            &screen,
        );
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.canvas,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.egui.render(&mut pass, jobs, &screen);
        }
        preview::end_frame(&mut self.egui);
        let view = frame.texture.create_view(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
        commands.push(encoder.finish());
        self.gpu.queue.submit(commands);
        // On Wayland, winit then holds back redraws until the compositor asks
        // for a frame, so frames never pile up waiting for a free image.
        self.window.pre_present_notify();
        self.gpu.queue.present(frame);
    }
}
