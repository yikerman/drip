//! Interactive frontend for building and tuning node graphs: a winit window,
//! wgpu output and egui widgets over the drip library.
//!
//!     drip-gui [project.drip]
//!
//! Logging goes to stderr; `RUST_LOG=debug` shows evaluation timings.

mod app;
mod display;
mod editor;
mod inspector;
mod preview;
mod theme;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use app::App;
use display::Display;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    let event_loop = EventLoop::new().expect("an event loop");
    if let Err(e) = event_loop.run_app(&mut Shell { file, running: None }) {
        log::error!("{e}");
    }
}

struct Shell {
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

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("Drip")
            .with_inner_size(winit::dpi::LogicalSize::new(1600, 1000));
        let window = Arc::new(event_loop.create_window(attributes).expect("a window"));
        let display = match Display::new(window.clone()) {
            Ok(display) => display,
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
            Some(display.max_texture_side()),
        );
        let app = App::new(self.file.take(), display.wide_gamut);
        window.request_redraw();
        self.running = Some(Running { window, display, egui, app, repaint_at: None });
    }

    fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
        if let (StartCause::ResumeTimeReached { .. }, Some(r)) = (cause, &self.running) {
            r.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else { return };
        let response = r.egui.on_window_event(&r.window, &event);
        // egui asks to repaint after every redraw; follow-up frames are
        // scheduled from its repaint delay in `frame` instead.
        let repaint = response.repaint && !matches!(event, WindowEvent::RedrawRequested);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => r.display.resize(size.width, size.height),
            WindowEvent::RedrawRequested => r.frame(),
            _ => {}
        }
        if repaint {
            r.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let wake = self.running.as_ref().and_then(|r| r.repaint_at);
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

impl Running {
    fn frame(&mut self) {
        let input = self.egui.take_egui_input(&self.window);
        let ctx = self.egui.egui_ctx().clone();
        let output = ctx.run_ui(input, |ui| self.app.ui(ui));
        self.egui.handle_platform_output(&self.window, output.platform_output);
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        self.display.render(&jobs, output.textures_delta, output.pixels_per_point);

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
