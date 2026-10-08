//! Interactive frontend for building and tuning node graphs: winit windows,
//! wgpu output and egui widgets over the drip library. The main window holds
//! the editor; popped-out nodes get windows of their own.
//!
//! `app.rs` owns the project, `editing.rs` applies edits and their effects, and
//! `worker.rs` schedules background evaluation. `editor.rs`, `inspector.rs` and
//! `node_ui/` handle interaction; `render/` prepares node views, draws images and
//! presents each window. This module owns OS windows and the event loop.
//!
//!     drip-gui [project.drip]
//!
//! Logging goes to stderr. `RUST_LOG=warn,drip=debug,drip_gui=debug` shows
//! processing diagnostics; `RUST_LOG=warn,drip_gui::frame=trace` shows frame timings.

mod app;
mod editing;
mod editor;
mod inspector;
mod node_ui;
mod parent;
mod render;
mod theme;
mod ui_state;
mod widgets;
mod worker;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

use app::App;
use node_ui::Popped;
use render::display::{Display, Gpu};

fn main() -> ExitCode {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,drip=info,drip_gui=info"),
    )
    .init();
    log::info!("Drip {} starting", env!("CARGO_PKG_VERSION"));
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    let event_loop = match EventLoop::<WorkerReady>::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            log::error!("cannot create event loop: {error}");
            return ExitCode::FAILURE;
        }
    };
    let wake = event_loop.create_proxy();
    let mut shell = Shell { file, running: None, wake, failed: false };
    if let Err(e) = event_loop.run_app(&mut shell) {
        log::error!("event loop failed: {e}");
        return ExitCode::FAILURE;
    }
    if shell.failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

struct WorkerReady;

struct Shell {
    wake: EventLoopProxy<WorkerReady>,
    file: Option<PathBuf>,
    running: Option<Running>,
    failed: bool,
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
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                log::error!("cannot open main window: {error}");
                self.failed = true;
                event_loop.exit();
                return;
            }
        };
        let (gpu, display) = match Gpu::new(window.clone()) {
            Ok(output) => output,
            Err(e) => {
                log::error!("cannot set up display window={:?}: {e}", window.id());
                self.failed = true;
                event_loop.exit();
                return;
            }
        };
        let main = Pane::new(&gpu, window, display);
        let wake = self.wake.clone();
        let app =
            App::new(gpu.compute.clone(), self.file.take(), main.display.wide_gamut, move || {
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
                            log::error!("cannot open window node={:?}: {e}", w.popped.node);
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
        log::debug!("opened window={:?}", window.id());
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
        log::trace!(
            target: "drip_gui::frame",
            "window={:?} {total:.1?}: ui {ui:.1?}, tessellate {tessellate:.1?}, render {:.1?}; font atlas {:.1}% full",
            self.window.id(),
            total - ui - tessellate,
            ctx.fonts(|f| f.font_atlas_fill_ratio()) * 100.0,
        );

        let delay = output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
        log::trace!(target: "drip_gui::frame", "window={:?} repaint_delay={delay:?}", self.window.id());
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

impl Drop for Pane {
    fn drop(&mut self) {
        log::debug!("closed window={:?}", self.window.id());
    }
}
