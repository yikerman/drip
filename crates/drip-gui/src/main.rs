//! Interactive frontend for building and tuning node graphs: a winit window,
//! wgpu output and egui widgets over the drip library.
//!
//!     drip-gui [project.drip]
//!
//! Logging goes to stderr; `RUST_LOG=debug` shows evaluation and frame
//! timings, `RUST_LOG=frame=debug` frame timings alone.

mod app;
mod display;
mod editor;
mod inspector;
mod preview;
mod theme;
mod views;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use app::App;
use display::{Display, Gpu};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    let event_loop = EventLoop::<WorkerReady>::with_user_event().build().expect("an event loop");
    let wake = event_loop.create_proxy();
    if let Err(e) = event_loop.run_app(&mut Shell { file, running: None, wake }) {
        log::error!("{e}");
    }
}

struct WorkerReady;

struct Shell {
    wake: EventLoopProxy<WorkerReady>,
    file: Option<PathBuf>,
    running: Option<Running>,
}

struct Running {
    window: Arc<Window>,
    display: Display,
    egui: egui_winit::State,
    app: App,
    /// When egui next wants a frame without new input, if ever.
    repaint_at: Option<Instant>,
}

impl ApplicationHandler<WorkerReady> for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Drip")
            .with_inner_size(winit::dpi::LogicalSize::new(1600, 1000));
        let window = Arc::new(event_loop.create_window(attributes).expect("a window"));
        let output =
            Gpu::new(window.clone()).and_then(|gpu| Ok((Display::new(&gpu, window.clone())?, gpu)));
        let (display, gpu) = match output {
            Ok(output) => output,
            Err(e) => {
                log::error!("cannot set up the display: {e}");
                event_loop.exit();
                return;
            }
        };
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let egui = egui_winit::State::new(
            ctx,
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(gpu.max_texture_side()),
        );
        let wake = self.wake.clone();
        let app = App::new(self.file.take(), display.wide_gamut, move || {
            let _ = wake.send_event(WorkerReady);
        });
        window.request_redraw();
        self.running = Some(Running { window, display, egui, app, repaint_at: None });
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: WorkerReady) {
        if let Some(running) = &self.running {
            running.window.request_redraw();
        }
    }

    fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
        if let (StartCause::ResumeTimeReached { .. }, Some(r)) = (cause, &self.running) {
            r.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            self.running = None;
            event_loop.exit();
            return;
        }
        let Some(r) = &mut self.running else { return };
        let response = r.egui.on_window_event(&r.window, &event);
        // egui asks to repaint after every redraw; follow-up frames are
        // scheduled from its repaint delay in `frame` instead.
        let repaint = response.repaint && !matches!(event, WindowEvent::RedrawRequested);
        match event {
            WindowEvent::Resized(size) => r.display.resize(size.width, size.height),
            WindowEvent::RedrawRequested => r.frame(),
            _ => {}
        }
        if repaint {
            r.window.request_redraw();
        }
    }

    /// The GPU surface must go before the event loop closes the Wayland
    /// connection it was created on; destroying it afterwards crashes the
    /// driver. `run_app` returns only after dropping the event loop, so the
    /// window state is dropped here rather than with `Shell`.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.running = None;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let wake = self.running.as_ref().and_then(|r| r.repaint_at);
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

impl Running {
    fn frame(&mut self) {
        let start = Instant::now();
        let input = self.egui.take_egui_input(&self.window);
        let ctx = self.egui.egui_ctx().clone();
        let output = ctx.run_ui(input, |ui| self.app.ui(ui));
        self.egui.handle_platform_output(&self.window, output.platform_output);
        let ui = start.elapsed();
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        let tessellate = start.elapsed() - ui;
        self.display.render(&jobs, output.textures_delta, output.pixels_per_point);
        drop(jobs);
        self.app.after_frame();
        let total = start.elapsed();
        // Render includes waiting for a surface texture, which can block on
        // vsync. egui rebuilds its font atlas once it passes 80% full.
        log::debug!(
            target: "frame",
            "{total:.1?}: ui {ui:.1?}, tessellate {tessellate:.1?}, render {:.1?}; font atlas {:.1}% full",
            total - ui - tessellate,
            ctx.fonts(|f| f.font_atlas_fill_ratio()) * 100.0,
        );

        let delay = output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
        self.repaint_at = match delay {
            Some(delay) if delay.is_zero() => {
                self.window.request_redraw();
                None
            }
            Some(delay) => Instant::now().checked_add(delay),
            None => None,
        };
    }
}
