//! Color-managed presentation, with one pixel pipeline across window systems.
//!
//! ```text
//! linear Rec.2020 preview                  egui + scopes (sRGB)
//!   | clip to SDR [0,1]                         |
//!   | Rec.2020 -> BT.709, extended sRGB encode  |
//!   +--------------------+---------------------+
//!                        v
//!             FP16 extended-sRGB canvas
//!             (egui's encoded-space blending)
//!                        |
//!                  sRGB decode
//!                        |
//!       BT.709 -> Rec.2020 -> clip SDR -> BT.709
//!                        |
//!          shared FP16 extended-linear sRGB
//!              /                       \
//! Wayland passthrough               native scRGB
//! BT.709 + Rec.2020 target          macOS / Windows
//!              \                       /
//!                OS display conversion
//!
//! Unsupported output -> bounded sRGB -> platform presentation
//! ```
//!
//! `compositor` owns the canvas and final conversion, `presentation` selects
//! the surface encoding, and `wayland` owns its explicit color description.
//! The shared transfer functions and matrices live in `shaders/color.wgsl`.
//!
//! egui treats float targets as gamma-encoded, so the canvas follows its
//! blending convention. Extended BT.709 coordinates preserve wide gamut.
//! SDR limits apply in Rec.2020, including after blending. Windows HDR desktop
//! white scaling remains unimplemented; macOS EDR is enabled by wgpu.

mod compositor;
mod presentation;
#[cfg(test)]
mod tests;
#[cfg(target_os = "linux")]
mod wayland;
#[cfg(not(target_os = "linux"))]
mod wayland {
    pub struct Description;

    impl Description {
        pub fn new(_: &winit::window::Window) -> Result<Self, String> {
            Err("Wayland color management unavailable on this platform".into())
        }
    }
}

use compositor::Compositor;
use presentation::Presentation;

use std::sync::Arc;

use winit::window::Window;

use crate::render::image;

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
        let info = adapter.get_info();
        log::info!(
            "GPU adapter={} backend={:?} driver={} {}",
            info.name,
            info.backend,
            info.driver,
            info.driver_info
        );
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
    compositor: Compositor,
    pub egui: egui_wgpu::Renderer,
    _presentation: Presentation,
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

        let presentation = Presentation::new(&window, &caps)?;
        let output = presentation.output;
        let (format, color_space) = (output.format, output.color_space);
        log::info!(
            "window={:?} presenting format={format:?} color_space={color_space:?} adapter={}",
            window.id(),
            adapter.get_info().name
        );
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

        let compositor = Compositor::new(device, output, &config);
        let mut egui =
            egui_wgpu::Renderer::new(device, CANVAS, egui_wgpu::RendererOptions::default());
        image::install(device, &mut egui);
        Ok(Display {
            gpu: gpu.clone(),
            window,
            surface,
            config,
            wide_gamut: output.wide_gamut(),
            compositor,
            egui,
            _presentation: presentation,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        (self.config.width, self.config.height) = (width, height);
        self.surface.configure(&self.gpu.device, &self.config);
        self.compositor.resize(&self.gpu.device, &self.config);
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
                log::warn!("window={:?} surface lost; recreating it", self.window.id());
                match self.gpu.instance.create_surface(self.window.clone()) {
                    Ok(surface) => self.surface = surface,
                    Err(e) => {
                        log::error!("window={:?} cannot recreate surface: {e}", self.window.id())
                    }
                }
                self.surface.configure(&self.gpu.device, &self.config);
                None
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("window={:?} surface texture validation failed", self.window.id());
                self.surface.configure(&self.gpu.device, &self.config);
                None
            }
            other => {
                log::trace!("window={:?} skipping frame: {other:?}", self.window.id());
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
                        view: &self.compositor.canvas,
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
        image::end_frame(&mut self.egui);
        let view = frame.texture.create_view(&Default::default());
        self.compositor.draw(&mut encoder, &view);
        commands.push(encoder.finish());
        self.gpu.queue.submit(commands);
        // On Wayland, winit then holds back redraws until the compositor asks
        // for a frame, so frames never pile up waiting for a free image.
        self.window.pre_present_notify();
        self.gpu.queue.present(frame);
    }
}
