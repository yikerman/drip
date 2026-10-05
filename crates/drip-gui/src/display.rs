//! The window's GPU output (DESIGN D2, D6). egui and the previews draw into an
//! offscreen canvas in egui's gamma encoding, extended beyond [0, 1] for
//! wide-gamut previews; a final pass converts the canvas for the swapchain. On
//! an scRGB swapchain the compositor maps the result to the display; otherwise
//! output is clipped to sRGB.

use std::sync::Arc;

use winit::window::Window;

use crate::preview;

const CANVAS: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The compositor's reference white for scRGB, 203 cd/m², over scRGB's 1.0 at
/// 80 cd/m², as the Vulkan WSI declares it to Wayland.
const SCRGB_WHITE: f32 = 203.0 / 80.0;

pub struct Display {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Whether the swapchain is scRGB, i.e. previews keep their full gamut.
    pub wide_gamut: bool,
    canvas: wgpu::TextureView,
    composite: wgpu::RenderPipeline,
    composite_group: wgpu::BindGroup,
    params: wgpu::Buffer,
    pub egui: egui_wgpu::Renderer,
}

impl Display {
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let size = window.inner_size();
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&adapter);

        let scrgb =
            caps.color_spaces(CANVAS).contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR);
        let (format, color_space) = if scrgb {
            (CANVAS, wgpu::SurfaceColorSpace::ExtendedSrgbLinear)
        } else {
            log::warn!("the display offers no scRGB output; previews are clipped to sRGB");
            // A non-sRGB format, so the gamma-encoded canvas is copied as is.
            let format = caps
                .formats
                .iter()
                .copied()
                .find(|f| !f.is_srgb())
                .ok_or("no usable surface format")?;
            (format, wgpu::SurfaceColorSpace::Srgb)
        };
        log::info!("presenting {format:?} in {color_space:?} on {}", adapter.get_info().name);
        let config = wgpu::SurfaceConfiguration {
            format,
            color_space,
            // The default is the driver's first mode, which may not wait for
            // vsync; frames beyond the refresh rate are never seen.
            present_mode: wgpu::PresentMode::Fifo,
            ..surface
                .get_default_config(&adapter, size.width.max(1), size.height.max(1))
                .ok_or("unsupported surface")?
        };
        surface.configure(&device, &config);

        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite params"),
            size: 8,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mode: u32 = if scrgb { 0 } else { 1 };
        queue.write_buffer(&params, 0, &[mode.to_ne_bytes(), SCRGB_WHITE.to_ne_bytes()].concat());

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
                entry_point: Some("fs"),
                targets: &[Some(format.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let canvas = Self::canvas(&device, &config);
        let composite_group = Self::composite_group(&device, &composite, &canvas, &params);
        let mut egui =
            egui_wgpu::Renderer::new(&device, CANVAS, egui_wgpu::RendererOptions::default());
        preview::install(&device, &mut egui);
        Ok(Display {
            instance,
            window,
            surface,
            device,
            queue,
            config,
            wide_gamut: scrgb,
            canvas,
            composite,
            composite_group,
            params,
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
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(canvas),
                },
                wgpu::BindGroupEntry { binding: 1, resource: params.as_entire_binding() },
            ],
        })
    }

    pub fn max_texture_side(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        (self.config.width, self.config.height) = (width, height);
        self.surface.configure(&self.device, &self.config);
        self.canvas = Self::canvas(&self.device, &self.config);
        self.composite_group =
            Self::composite_group(&self.device, &self.composite, &self.canvas, &self.params);
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
                self.egui.update_texture(&self.device, &self.queue, id, &delta);
            }
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Lost => {
                log::warn!("the surface was lost; recreating it");
                match self.instance.create_surface(self.window.clone()) {
                    Ok(surface) => self.surface = surface,
                    Err(e) => log::error!("cannot recreate the surface: {e}"),
                }
                self.surface.configure(&self.device, &self.config);
                None
            }
            other => {
                log::debug!("skipping a frame: {other:?}");
                self.surface.configure(&self.device, &self.config);
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
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut commands =
            self.egui.update_buffers(&self.device, &self.queue, &mut encoder, jobs, &screen);
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
        self.queue.submit(commands);
        self.queue.present(frame);
    }
}
