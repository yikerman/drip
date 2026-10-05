//! Interactive frontend for building and tuning node graphs: winit windows,
//! wgpu output and egui widgets over the drip library. The main window holds
//! the editor; popped-out nodes get windows of their own (DESIGN G13).
//!
//!     drip-gui [project.drip]
//!
//! Logging goes to stderr; `RUST_LOG=debug` shows evaluation and frame
//! timings, `RUST_LOG=frame=debug` frame timings alone.

mod app;
mod display;
mod editing;
mod editor;
mod inspector;
mod node_ui;
mod parent;
mod preview;
mod theme;
mod views;
mod widgets;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

use app::App;
use display::{Display, Gpu};
use node_ui::Popped;

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
    gpu: Gpu,
    main: Pane,
    /// Popped-out windows.
    windows: Vec<(Popped, Pane)>,
    app: App,
}

/// A window and what draws into it.
struct Pane {
    window: Arc<Window>,
    display: Display,
    egui: egui_winit::State,
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
            .with_inner_size(LogicalSize::new(1600, 1000));
        let window = Arc::new(event_loop.create_window(attributes).expect("a window"));
        let (gpu, display) = match Gpu::new(window.clone()) {
            Ok(output) => output,
            Err(e) => {
                log::error!("cannot set up the display: {e}");
                event_loop.exit();
                return;
            }
        };
        let main = Pane::new(&gpu, window, display);
        let wake = self.wake.clone();
        let app = App::new(self.file.take(), main.display.wide_gamut, move || {
            let _ = wake.send_event(WorkerReady);
        });
        self.running = Some(Running { gpu, main, windows: Vec::new(), app });
    }

    /// Takes the worker's results whether or not any window can draw;
    /// `about_to_wait` then redraws the windows.
    fn user_event(&mut self, _: &ActiveEventLoop, _: WorkerReady) {
        if let Some(running) = &mut self.running {
            running.app.poll();
        }
    }

    fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
        if let (StartCause::ResumeTimeReached { .. }, Some(r)) = (cause, &mut self.running) {
            let now = Instant::now();
            for pane in std::iter::once(&mut r.main).chain(r.windows.iter_mut().map(|(_, p)| p)) {
                // A hidden Wayland window may not draw until it becomes visible.
                // Consume its deadline now so waiting for that frame cannot spin.
                if pane.repaint_at.take_if(|at| *at <= now).is_some() {
                    pane.window.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else { return };
        let Some((pane, popped, app)) = r.pane(id) else { return };
        match (&event, popped) {
            (WindowEvent::CloseRequested, Some(popped)) => return app.close_window(popped),
            (WindowEvent::CloseRequested, None) => {
                self.running = None;
                return event_loop.exit();
            }
            _ => {}
        }
        let response = pane.egui.on_window_event(&pane.window, &event);
        // egui asks to repaint after every redraw; follow-up frames are
        // scheduled from its repaint delay in `frame` instead.
        let repaint = response.repaint && !matches!(event, WindowEvent::RedrawRequested);
        match event {
            WindowEvent::Resized(size) => pane.display.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                pane.frame(|ui| match popped {
                    Some(popped) => app.window(ui, popped),
                    None => app.ui(ui),
                });
                app.after_frame();
            }
            _ => {}
        }
        if repaint {
            pane.window.request_redraw();
        }
    }

    /// The GPU surface must go before the event loop closes the Wayland
    /// connection it was created on; destroying it afterwards crashes the
    /// driver. `run_app` returns only after dropping the event loop, so the
    /// window state is dropped here rather than with `Shell`.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.running = None;
    }

    /// Opens and closes windows to match the popped-out nodes, and redraws
    /// every window when what they show changed.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(r) = &mut self.running else { return };
        let wanted = r.app.windows();
        r.windows.retain(|(popped, _)| wanted.iter().any(|w| w.popped == *popped));
        for w in wanted {
            match r.windows.iter().find(|(popped, _)| *popped == w.popped) {
                Some((_, pane)) => {
                    if pane.window.title() != w.title {
                        pane.window.set_title(&w.title);
                    }
                }
                None => {
                    let scale = r.main.window.scale_factor() as f32;
                    let size = r.app.window_size(w.popped, scale);
                    let attributes = Window::default_attributes()
                        .with_title(&w.title)
                        .with_inner_size(LogicalSize::new(size.x, size.y));
                    match Pane::open(event_loop, &r.gpu, attributes) {
                        Ok(pane) => {
                            parent::set_parent(&pane.window, &r.main.window);
                            r.windows.push((w.popped, pane));
                        }
                        Err(e) => {
                            log::error!("cannot open a window: {e}");
                            r.app.close_window(w.popped);
                        }
                    }
                }
            }
        }
        if r.app.take_redraw() {
            r.panes().for_each(|p| p.window.request_redraw());
        }
        let wake = r.panes().filter_map(|p| p.repaint_at).min();
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

impl Running {
    fn panes(&self) -> impl Iterator<Item = &Pane> {
        std::iter::once(&self.main).chain(self.windows.iter().map(|(_, pane)| pane))
    }

    /// Window `id`'s pane, what it shows if popped out, and the app.
    fn pane(&mut self, id: WindowId) -> Option<(&mut Pane, Option<Popped>, &mut App)> {
        if self.main.window.id() == id {
            return Some((&mut self.main, None, &mut self.app));
        }
        let (popped, pane) = self.windows.iter_mut().find(|(_, pane)| pane.window.id() == id)?;
        Some((pane, Some(*popped), &mut self.app))
    }
}

impl Pane {
    fn open(
        event_loop: &ActiveEventLoop,
        gpu: &Gpu,
        attributes: WindowAttributes,
    ) -> Result<Self, String> {
        let window = Arc::new(event_loop.create_window(attributes).map_err(|e| e.to_string())?);
        let display = Display::new(gpu, window.clone())?;
        Ok(Pane::new(gpu, window, display))
    }

    fn new(gpu: &Gpu, window: Arc<Window>, display: Display) -> Self {
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
        window.request_redraw();
        Pane { window, display, egui, repaint_at: None }
    }

    fn frame(&mut self, content: impl FnMut(&mut egui::Ui)) {
        let start = Instant::now();
        let input = self.egui.take_egui_input(&self.window);
        let ctx = self.egui.egui_ctx().clone();
        let output = ctx.run_ui(input, content);
        self.egui.handle_platform_output(&self.window, output.platform_output);
        let ui = start.elapsed();
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        let tessellate = start.elapsed() - ui;
        self.display.render(&jobs, output.textures_delta, output.pixels_per_point);
        drop(jobs);
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
