//! Application state and layout: one project, evaluated at a global detail
//! level, in a main window and a window per popped-out part of a node.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::model::NodeId;
use crate::model::Project;
use crate::model::Registry;
use crate::node_ui::templates;
use egui::{CentralPanel, Panel, RichText, Ui, Vec2};

use crate::editing::{Frame, NodeCx, Report};
use crate::editor::Editor;
use crate::node_ui::{self, Popped};
use crate::ui_state::{self, DEFAULT_LEVEL, MAX_LEVEL};
use crate::worker::{Notice, Worker};
use crate::{inspector, theme};

pub struct App {
    project: Project,
    registry: Registry,
    worker: Worker,
    dirty: bool,
    file: Option<PathBuf>,
    selected: Option<NodeId>,
    editor: Editor,
    /// One user-selected level for every preview target.
    level: u8,
    status: Option<Status>,
    action: Option<&'static str>,
    wide_gamut: bool,
    /// Parts of nodes shown in their own windows; not saved.
    popped: BTreeSet<Popped>,
    /// Whether the windows need redrawing because what they show changed.
    redraw: bool,
}

/// A popped-out window as the app wants it.
pub struct Window {
    pub popped: Popped,
    pub title: String,
}

struct Status {
    error: bool,
    text: String,
}

impl App {
    pub fn new(
        file: Option<PathBuf>,
        wide_gamut: bool,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let mut app = App {
            project: templates::raw_to_tiff(),
            registry: Registry,
            worker: Worker::new(wake),
            dirty: true,
            file: None,
            selected: None,
            editor: Editor::default(),
            level: DEFAULT_LEVEL,
            status: None,
            action: None,
            wide_gamut,
            popped: BTreeSet::new(),
            redraw: false,
        };
        if let Some(file) = file {
            app.open(file);
        }
        app
    }

    /// Shows `result` in the status line; event owners choose what to log.
    fn report(&mut self, result: Result<String, String>) {
        let (error, text) = match result {
            Ok(text) => (false, text),
            Err(text) => (true, text),
        };
        self.status = Some(Status { error, text });
        self.redraw = true;
    }

    /// The main window: menu, status line, inspector and editor.
    pub fn ui(&mut self, ui: &mut Ui) {
        Panel::top("menu").show_separator_line(false).show(ui, |ui| self.menu(ui));
        Panel::bottom("status").show_separator_line(false).show(ui, |ui| self.status_line(ui));
        let results = |id| self.worker.result(id);
        let mut frame = Frame::new(&results, &self.popped, self.action.is_some());
        let graph = &mut self.project.graph;
        Panel::right("inspector").resizable(true).default_size(320.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(id) = self.selected.filter(|id| graph.node(*id).is_some()) {
                    inspector::selected_node(ui, &mut NodeCx::new(graph, id, &mut frame));
                    ui.separator();
                }
                inspector::inputs(ui, graph, &mut frame);
            });
        });
        CentralPanel::default().show(ui, |ui| {
            self.editor.show(ui, graph, &self.registry, &mut self.selected, &mut frame);
        });
        let report = frame.report;
        self.apply(report);
    }

    /// A popped-out window.
    pub fn window(&mut self, ui: &mut Ui, popped: Popped) {
        let results = |id| self.worker.result(id);
        let mut frame = Frame::new(&results, &self.popped, self.action.is_some());
        let graph = &mut self.project.graph;
        CentralPanel::default().show(ui, |ui| {
            if graph.node(popped.node).is_some() {
                popped.show(ui, &mut NodeCx::new(graph, popped.node, &mut frame));
            }
        });
        let report = frame.report;
        self.apply(report);
    }

    pub fn windows(&self) -> Vec<Window> {
        let window = |popped: Popped| {
            let node = self.project.graph.node(popped.node).expect("popped nodes exist");
            Window { popped, title: popped.title(&node) }
        };
        self.popped.iter().map(|&popped| window(popped)).collect()
    }

    /// The size, in points, to open window `popped` at. egui has no pass
    /// that only measures, so the content is laid out once, unseen, in a
    /// scratch context at the window's scale; a size it records there wins
    /// over its kind's. Resizing once the window is open would race the
    /// compositor's own configures.
    pub fn window_size(&mut self, popped: Popped, pixels_per_point: f32) -> Vec2 {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(2000.0));
        let mut input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point =
            Some(pixels_per_point);
        ctx.run_ui(input, |ui| self.window(ui, popped)).textures_delta.clear();
        let node = self.project.graph.node(popped.node).expect("popped nodes exist");
        node_ui::fitted(&ctx).unwrap_or_else(|| popped.size(&node))
    }

    pub fn close_window(&mut self, popped: Popped) {
        self.popped.remove(&popped);
        self.redraw = true;
    }

    /// Whether the windows need redrawing since the last call.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// Applies what a frame's GUIs did and requests evaluation after edits.
    fn apply(&mut self, report: Report) {
        for (id, params) in report.bindings {
            if let Err(error) = self.worker.bind(&self.project.graph, id, params) {
                self.report(Err(error));
            }
        }
        self.redraw |= report.redraw;
        self.dirty |= report.edited;
        if let Some(refused) = report.refused {
            log::debug!("graph edit refused: {refused}");
            self.report(Err(refused));
        }
        if let Some((id, name)) = report.action {
            self.run_action(id, name);
        }
        if let Some(popped) = report.toggled {
            if !self.popped.remove(&popped) {
                self.popped.insert(popped);
            }
            self.redraw = true;
        }
        let graph = &self.project.graph;
        self.popped.retain(|p| graph.node(p.node).is_some());
        if std::mem::take(&mut self.dirty) {
            let targets = self.project.graph.nodes().map(|(id, _)| id).collect();
            if let Err(error) = self.worker.request(&self.project.graph, self.level, targets) {
                self.report(Err(error));
            }
            self.redraw = true;
        }
    }

    /// Takes the worker's notices; called whenever the worker wakes the app.
    pub fn poll(&mut self) {
        let mut evaluated = None;
        let mut action_reported = false;
        let notices = self.worker.poll();
        self.redraw |= !notices.is_empty();
        for notice in notices {
            match notice {
                Notice::Bound { id, result } => {
                    if self.project.graph.dag.node(id).is_ok() {
                        let result =
                            result.and_then(|node| self.project.graph.dag.replace(id, node));
                        self.dirty |= result.is_ok();
                        self.report(result.map(|()| "loaded".into()).map_err(|e| e.to_string()));
                        action_reported = true;
                    }
                }
                Notice::Opened { file, result } => {
                    let result =
                        result.and_then(|project| self.set_project(project, Some(file.clone())));
                    self.report(result.map(|()| format!("opened {}", file.display())));
                    action_reported = true;
                }
                Notice::Evaluated(result) => evaluated = Some(result),
                Notice::Action(result) => {
                    self.action = None;
                    self.report(result.map(|()| "done".into()));
                    action_reported = true;
                }
                Notice::Failed => {
                    self.action = None;
                    self.report(Err("the preview worker stopped".into()));
                    action_reported = true;
                }
            }
        }
        // A preview finishing in the same frame must not hide an export error.
        if !action_reported
            && self.action.is_none()
            && let Some(result) = evaluated
        {
            self.report(result.map(|()| "done".into()));
        }
    }

    pub fn after_frame(&mut self) {
        self.worker.after_frame();
    }

    fn menu(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("New").clicked()
                && let Err(error) = self.set_project(templates::raw_to_tiff(), None)
            {
                self.report(Err(error));
            }
            if ui.button("Open…").clicked()
                && let Some(file) =
                    rfd::FileDialog::new().add_filter("Drip project", &["drip"]).pick_file()
            {
                self.open(file);
            }
            if ui.button("Save").clicked() {
                match self.file.clone() {
                    Some(file) => {
                        self.save(&file, self.project.clone());
                    }
                    None => self.save_as(false),
                }
            }
            if ui.button("Save as…").clicked() {
                self.save_as(false);
            }
            if ui.button("Save template…").clicked() {
                self.save_as(true);
            }
            if ui.button("Invalidate cache").clicked() {
                self.invalidate_cache();
            }
            ui.separator();
            let label = |level| {
                if level == 0 { "Full".to_owned() } else { format!("1/{}", 1u32 << level) }
            };
            let previous = self.level;
            crate::widgets::dropdown(
                ui,
                egui::ComboBox::from_label("Preview detail").width(64.0),
                &mut self.level,
                &(0..=MAX_LEVEL).collect::<Vec<_>>(),
                label,
            )
            .on_hover_text("Global preview scale. Export always uses full detail.");
            if self.level != previous {
                log::debug!("preview level changed from {previous} to {}", self.level);
                ui_state::set_preview_level(&mut self.project.ui, self.level);
                self.dirty = true;
            }
        });
    }

    fn status_line(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if !self.wide_gamut {
                ui.label(RichText::new("previews clipped to sRGB").color(theme::ERROR));
            }
            if self.worker.busy() {
                let label = ui.label("evaluating…");
                if self.worker.showing_previous() {
                    label.on_hover_text("Views retain the previous completed evaluation while the current request runs.");
                }
            }
            if let Some(action) = self.action {
                ui.label(format!("running {action}…"));
            }
            if let Some(status) = &self.status
                && (status.error || (!self.worker.busy() && self.action.is_none()))
            {
                let text = RichText::new(&status.text);
                ui.label(if status.error { text.color(theme::ERROR) } else { text });
            }
        });
    }

    fn set_project(&mut self, project: Project, file: Option<PathBuf>) -> Result<(), String> {
        let level = ui_state::preview_level(&project.ui)?;
        self.level = level;
        self.project = project;
        self.file = file;
        self.selected = None;
        self.editor = Editor::default();
        // Node ids are reused across projects.
        self.popped.clear();
        self.dirty = true;
        self.worker.reset()
    }

    fn open(&mut self, file: PathBuf) {
        let result = self.worker.open(file.clone());
        self.report(result.map(|()| format!("opening {}", file.display())));
    }

    fn save_as(&mut self, template: bool) {
        let name = if template { "Drip template" } else { "Drip project" };
        let Some(file) = rfd::FileDialog::new().add_filter(name, &["drip"]).save_file() else {
            return;
        };
        let mut file = file.into_os_string();
        if !file.as_encoded_bytes().ends_with(b".drip") {
            file.push(".drip");
        }
        let file = PathBuf::from(file);
        if template {
            match self.project.template() {
                Ok(project) => {
                    self.save(&file, project);
                }
                Err(error) => self.report(Err(error.to_string())),
            }
        } else if self.save(&file, self.project.clone()) {
            self.file = Some(file);
        }
    }

    /// Writes `project` to `file`; returns whether that succeeded.
    fn save(&mut self, file: &Path, project: Project) -> bool {
        let result = project
            .to_json()
            .map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(file, text).map_err(|e| e.to_string()));
        let saved = result.is_ok();
        let shown = file.display();
        match &result {
            Ok(()) => log::info!("saved project {shown}"),
            Err(error) => log::error!("cannot save project {shown}: {error}"),
        }
        self.report(result.map(|()| format!("saved {shown}")).map_err(|e| format!("{shown}: {e}")));
        saved
    }

    fn invalidate_cache(&mut self) {
        self.dirty = true;
        let result = (|| {
            self.worker.invalidate()?;
            for (id, node) in self.project.graph.dag.nodes().filter(|(_, n)| n.loads_assets()) {
                self.worker.bind(
                    &self.project.graph,
                    id,
                    node.parameters().map_err(|e| e.to_string())?,
                )?;
            }
            Ok(())
        })();
        self.report(result.map(|()| "invalidating cache…".into()));
    }

    fn run_action(&mut self, id: NodeId, name: &'static str) {
        match self.worker.action(&self.project.graph, id, name) {
            Ok(()) => {
                self.action = Some(name);
                self.report(Ok(format!("running {name}…")));
            }
            Err(error) => {
                log::error!("cannot start action node={id:?} action={name}: {error}");
                self.report(Err(error));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use drip::{Error as KernelError, runtime::KernelContext};
    use serde::{Deserialize, Serialize};
    fn empty_contract<P>(_: &drip::runtime::GlobalContext, _: &P) -> drip::Result<()> {
        Ok(())
    }

    use crate::node_ui::{preview::PreviewImage, scopes::Histogram};

    use crate::node_ui::preview::ImageView;
    use drip::param::ParamKind;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::*;

    #[test]
    fn detail_dropdown_scroll_updates_the_project_and_stops_at_ends() {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1600.0, 100.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(100)
            .build_ui_state(|ui, app: &mut App| app.menu(ui), App::new(None, true, || {}));
        let pos = h.get_by_label("Preview detail").rect().center();
        for (delta, level) in [(-1.0, 2), (1.0, 1), (10.0, 0), (-20.0, MAX_LEVEL)] {
            h.state_mut().dirty = false;
            h.event(egui::Event::PointerMoved(pos));
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, delta),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
            h.run();
            assert_eq!(h.state().level, level);
            assert_eq!(h.state().project.ui["preview_level"], json!(level));
            assert!(h.state().dirty);
        }
    }

    /// The built-in template applied to the fixture raw, saved to a temp file.
    fn fixture_project(name: &str) -> PathBuf {
        let raw =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-ilce-7rm3.arw");
        let mut p = templates::raw_to_tiff();
        let read = p.graph.find("RAW").unwrap();
        p.graph.set_param(read, "path", json!(raw)).unwrap();
        let file =
            std::env::temp_dir().join(format!("drip-gui-{name}-{}.drip", std::process::id()));
        std::fs::write(&file, p.to_json().unwrap()).unwrap();
        file
    }

    fn harness<'a>(app: App) -> Harness<'a, App> {
        Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_ui_state(
            |ui, app: &mut App| {
                app.poll();
                app.ui(ui)
            },
            app,
        )
    }

    fn settle(h: &mut Harness<'_, App>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            h.step();
            h.state_mut().after_frame();
            if !h.state().worker.busy() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "worker did not finish");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        h.run();
    }

    /// A parameter edit as the inspector reports it.
    fn set_param(h: &mut Harness<'_, App>, id: NodeId, name: &str, value: serde_json::Value) {
        edit(h.state_mut(), crate::editing::Edit::Param(id, name, value));
    }

    fn edit(app: &mut App, edit: crate::editing::Edit<'_>) {
        let results = |id| app.worker.result(id);
        let mut frame = Frame::new(&results, &app.popped, app.action.is_some());
        frame.edit(&mut app.project.graph, edit);
        app.apply(frame.report);
    }

    #[test]
    fn shared_edits_redraw_windows_without_evaluating_presentation_changes() {
        let mut h = harness(App::new(None, true, || {}));
        settle(&mut h);
        let app = h.state_mut();
        let id = app.project.graph.find("Sigmoid").unwrap();
        let popped = Popped { node: id, part: node_ui::Part::Parameters };
        app.popped.insert(popped);
        app.take_redraw();

        for change in [
            crate::editing::Edit::Name(id, "tone"),
            crate::editing::Edit::External(id, "contrast", true),
            crate::editing::Edit::Ui(
                id,
                crate::ui_state::LayoutField::Position,
                egui::vec2(40.0, 50.0),
            ),
        ] {
            edit(app, change);
            assert!(app.take_redraw(), "other windows must see the shared edit");
            assert!(!app.worker.busy(), "presentation edits need no evaluation");
        }
        assert_eq!(app.windows()[0].title, "tone parameters · Drip");
        assert!(app.project.graph.inputs().any(|input| input == (id, "contrast")));

        edit(app, crate::editing::Edit::Name(id, ""));
        assert_eq!(app.project.graph.node(id).unwrap().name, "tone");
        assert!(app.status.as_ref().unwrap().error);
        assert!(!app.worker.busy());

        edit(app, crate::editing::Edit::Param(id, "contrast", json!(2.0)));
        assert!(app.take_redraw());
        assert!(app.worker.busy(), "processing edits must request evaluation");
    }

    fn has_view(app: &App, label: &str) -> bool {
        let id = app.project.graph.find(label).unwrap();
        app.worker.result(id).is_some_and(|r| r.as_ref().is_ok_and(Option::is_some))
    }

    #[test]
    fn previews_use_the_selected_global_detail() {
        let file = fixture_project("open");
        let mut h = harness(App::new(Some(file.clone()), true, || {}));
        settle(&mut h);
        let app = h.state();
        assert!(app.file.as_ref() == Some(&file));
        for name in ["Preview", "Histogram", "Waveform", "Vectorscope"] {
            assert!(
                has_view(app, name),
                "{name}: {}",
                app.status.as_ref().map(|s| s.text.as_str()).unwrap_or("no status")
            );
        }
        let id = app.project.graph.find("Preview").unwrap();
        let image = app
            .worker
            .result(id)
            .unwrap()
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .downcast_ref::<ImageView>()
            .unwrap()
            .image
            .clone();
        assert_eq!(app.level, DEFAULT_LEVEL);
        assert_eq!(image.width, 3984, "1/2 preview with reduced Bayer input to RCD");
        h.get_by_label("Invalidate cache").click();
        settle(&mut h);
        let refreshed = &h
            .state()
            .worker
            .result(id)
            .unwrap()
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .downcast_ref::<ImageView>()
            .unwrap()
            .image;
        assert!(!std::sync::Arc::ptr_eq(&image, refreshed));
        assert_eq!(image.texels, refreshed.texels);
        h.get_by_label("Preview detail").click();
        settle(&mut h);
        h.get_by_label("1/4").click();
        settle(&mut h);
        assert_eq!(h.state().level, 2);
        let app = h.state();
        let image = &app
            .worker
            .result(id)
            .unwrap()
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .downcast_ref::<ImageView>()
            .unwrap()
            .image;
        assert_eq!(image.width, 1992);
        let histogram = app.project.graph.find("Histogram").unwrap();
        let histogram = app
            .worker
            .result(histogram)
            .unwrap()
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .downcast_ref::<Histogram>()
            .unwrap();
        assert_eq!(
            histogram.counts.iter().map(|bin| bin[0] as usize).sum::<usize>(),
            image.width * image.height
        );
        let image = std::sync::Arc::downgrade(image);
        h.state_mut().project.graph.set_ui(id, json!({"size": [1200.0, 900.0]})).unwrap();
        settle(&mut h);
        assert_eq!(h.state().level, 2, "view size does not select resolution");
        assert!(image.upgrade().is_some(), "resizing preserves the evaluated image");
        h.get_by_label("Save").click();
        settle(&mut h);
        let mut restored = App::new(Some(file.clone()), true, || {});
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while restored.worker.busy() {
            restored.poll();
            assert!(std::time::Instant::now() < deadline, "project did not load");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(restored.level, 2);
        assert!(!restored.status.as_ref().unwrap().error);

        h.get_by_label("New").click();
        settle(&mut h);
        assert!(!has_view(h.state(), "Preview"), "the new template has no raw file yet");
        assert_eq!(h.state().level, DEFAULT_LEVEL);
        std::fs::remove_file(file).unwrap();
    }

    mod evaluation_is_independent_of_node_positions_fixture {
        use super::*;

        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALLS: AtomicUsize = AtomicUsize::new(0);
        #[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
        struct OffscreenKernel {
            #[param(ParamKind::Bool { default: false })]
            value: bool,
        }
        #[drip::node(id="test.offscreen", name="offscreen", category="test", contract=empty_contract)]
        fn offscreen(_: &KernelContext<'_>, _: &OffscreenKernel) -> drip::Result<()> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        #[test]
        fn evaluation_is_independent_of_node_positions() {
            let kind = Registry.get("test.offscreen").unwrap();

            let mut project = Project::default();
            let left = project.graph.add_node(kind).unwrap();
            let right = project.graph.add_node(kind).unwrap();
            for (id, x) in [(left, -100_000), (right, 100_000)] {
                project.graph.set_ui(id, json!({"pos": [x, 0]})).unwrap();
            }
            let completions = Arc::new(AtomicUsize::new(0));
            let wake = completions.clone();
            let mut app = App::new(None, true, move || {
                wake.fetch_add(1, Ordering::SeqCst);
            });
            app.set_project(project, None).unwrap();
            let mut h = harness(app);
            settle(&mut h);
            assert_eq!(CALLS.load(Ordering::SeqCst), 2);
            assert!(h.state().worker.result(left).unwrap().is_ok());
            assert!(h.state().worker.result(right).unwrap().is_ok());

            set_param(&mut h, left, "value", json!(true));
            settle(&mut h);
            assert_eq!(
                CALLS.load(Ordering::SeqCst),
                4,
                "each request evaluates both demanded nodes"
            );
            let before = completions.load(Ordering::SeqCst);
            h.state_mut().project.graph.set_ui(left, json!({"pos": [0, 0]})).unwrap();
            settle(&mut h);
            assert_eq!(completions.load(Ordering::SeqCst), before, "moving nodes requests no work");
        }
    }

    mod evaluating_message_keeps_the_ui_and_previous_preview_available_fixture {
        use super::*;

        use crate::node_ui::data::Rgb;

        use std::sync::{Arc, Mutex, mpsc};
        use std::time::{Duration, Instant};
        static GATE: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
        #[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
        struct SlowKernel {
            #[param(ParamKind::Bool { default: false })]
            block: bool,
        }
        #[drip::node(id="test.slow", name="slow", category="test", contract=empty_contract)]
        fn slow(_: &KernelContext<'_>, _: &SlowKernel) -> drip::Result<()> {
            Ok(())
        }
        struct SlowGui;
        impl crate::node_ui::GuiNode for SlowGui {
            type Parameters = SlowKernel;
            type Presentation = PreviewImage;
            const ID: &'static str = "test.slow";
            const PREPARE: Option<crate::node_ui::Prepare<Self>> = Some(|p, _, _| {
                if p.block {
                    let gate = GATE.lock().unwrap();
                    let (started, release) = gate.as_ref().unwrap();
                    started.send(()).unwrap();
                    release.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                Ok(PreviewImage::new(&Arc::new(Rgb {
                    width: 1,
                    height: 1,
                    requested_scale: 1,
                    pixels: vec![[0.5; 3]].into(),
                })))
            });
        }
        #[linkme::distributed_slice(crate::node_ui::BINDINGS)]
        static SLOW_UI: crate::node_ui::Binding = crate::node_ui::Binding::new::<SlowGui>();

        #[test]
        fn evaluating_message_keeps_the_ui_and_previous_preview_available() {
            let (started, entered) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            *GATE.lock().unwrap() = Some((started, resume));
            let mut project = Project::default();
            let id = project.graph.add_node(Registry.get("test.slow").unwrap()).unwrap();
            let mut app = App::new(None, true, || {});
            app.set_project(project, None).unwrap();
            let mut h = harness(app);
            settle(&mut h);
            let before = &h
                .state()
                .worker
                .result(id)
                .unwrap()
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .downcast_ref::<ImageView>()
                .unwrap()
                .image;
            let before = before.clone();
            set_param(&mut h, id, "block", json!(true));
            h.step();
            entered.recv_timeout(Duration::from_secs(5)).unwrap();
            let start = Instant::now();
            h.step();
            assert!(h.query_by_label("evaluating…").is_some());
            let shown = &h
                .state()
                .worker
                .result(id)
                .unwrap()
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .downcast_ref::<ImageView>()
                .unwrap()
                .image;
            assert!(Arc::ptr_eq(shown, &before));
            h.get_by_label("Preview detail").click();
            h.run();
            assert!(
                h.query_by_label("Full").is_some(),
                "controls respond while the node is blocked"
            );
            assert!(start.elapsed() < Duration::from_secs(2));
            release.send(()).unwrap();
            settle(&mut h);
            assert!(h.query_by_label("evaluating…").is_none());
            assert!(h.query_by_label("done").is_some());
        }
    }

    #[test]
    fn invalid_detail_does_not_replace_the_open_project() {
        let mut app = App::new(None, true, || {});
        let original = app.project.clone();
        for value in [json!(-1), json!(9), json!(256), json!(2.5), json!("2"), json!(null)] {
            let mut project = templates::raw_to_tiff();
            project.ui = json!({"preview_level": value});
            assert!(app.set_project(project, None).is_err());
            assert_eq!(app.project.to_json().unwrap(), original.to_json().unwrap());
            assert_eq!(app.level, DEFAULT_LEVEL);
        }
    }

    #[test]
    fn reports_unreadable_projects() {
        let mut h = harness(App::new(Some("/nonexistent.drip".into()), true, || {}));
        settle(&mut h);
        assert!(h.state().status.as_ref().is_some_and(|s| s.error));
    }

    #[test]
    fn warns_when_previews_are_clipped() {
        let mut h = harness(App::new(None, false, || {}));
        h.run();
        assert!(h.query_by_label("previews clipped to sRGB").is_some());
        let mut h = harness(App::new(None, true, || {}));
        h.run();
        assert!(h.query_by_label("previews clipped to sRGB").is_none());
    }

    #[test]
    fn inspector_shows_the_selected_node_and_the_inputs() {
        let mut app = App::new(None, true, || {});
        app.selected = app.project.graph.find("Export");
        let mut h = harness(app);
        h.run();
        for label in
            ["profile", "intent", "depth", "export", "path (input)", "RAW · path", "Export · path"]
        {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
    }

    #[test]
    fn node_help_follows_the_type_id_and_stays_out_of_parameter_windows() {
        let documentation =
            Registry.get("apply-rec2020-sigmoid").unwrap().documentation.trim().replace('\n', " ");
        let mut app = App::new(None, true, || {});
        let id = app.project.graph.find("Sigmoid").unwrap();
        app.selected = Some(id);
        let mut h = harness(app);
        h.run();
        let kind = h.get_by_label("apply-rec2020-sigmoid").rect();
        let description = h.get_by_label(&documentation).rect();
        let output = h.get_by_label("output output").rect();
        let control = h.get_by_label("contrast").rect();
        assert!(kind.bottom() <= description.top());
        assert!(description.bottom() <= output.top());
        assert!(output.bottom() <= control.top());

        let popped = Popped { node: id, part: node_ui::Part::Parameters };
        let mut popup = Harness::builder().with_size(egui::vec2(360.0, 320.0)).build_ui_state(
            move |ui, app: &mut App| app.window(ui, popped),
            {
                let mut app = App::new(None, true, || {});
                app.project = h.state().project.clone();
                app
            },
        );
        popup.run();
        assert!(popup.query_by_label("apply-rec2020-sigmoid").is_some());
        assert!(popup.query_by_label("contrast").is_some());
        assert!(popup.query_by_label("Rec.2020 RGB").is_none());
        assert!(popup.query_by_label("darktable: sigmoid").is_none());
        assert!(popup.query_by_label(&documentation).is_none());
    }

    #[test]
    fn nodes_without_controls_show_documentation_and_references() {
        let mut app = App::new(None, true, || {});
        app.selected = app.project.graph.find("Demosaic");
        let mut h = harness(app);
        h.run();
        let kind = h.get_by_label("bayer-rcd").rect();
        let input = h.get_by_label("input image").rect();
        let output = h.get_by_label("output output").rect();
        let reference = h.get_by_label("RCD source").rect();
        assert!(kind.bottom() <= input.top());
        assert!(input.bottom() <= output.top());
        assert!(output.bottom() <= reference.top());
    }

    mod undocumented_nodes_show_generated_port_help_fixture {
        use super::*;

        use drip::node::data::{Color, ColorRgb, ImageDesc};
        use drip::ports::Read;
        use drip::ports::{Cpu, Write};
        fn contract(
            _: &drip::runtime::GlobalContext,
            _: &(),
            image: Option<&ImageDesc<Color>>,
        ) -> drip::Result<(Option<ImageDesc<Color>>,)> {
            Ok((image.cloned(),))
        }
        #[drip::node(id="test.generic", name="generic", category="test", contract=contract)]
        fn generic(
            _: &KernelContext<'_>,
            _: &(),
            image: Read<'_, Cpu<ColorRgb>>,
            output: Write<'_, Cpu<ColorRgb>>,
        ) -> drip::Result<()> {
            output.data.copy_from_slice(image.data);
            Ok(())
        }

        #[test]
        fn undocumented_nodes_show_generated_port_help() {
            let mut project = Project::default();
            let id = project.graph.add_node(Registry.get("test.generic").unwrap()).unwrap();
            let mut app = App::new(None, true, || {});
            app.set_project(project, None).unwrap();
            app.selected = Some(id);
            let mut h = harness(app);
            h.run();
            let kind = h.get_by_label("test.generic").rect();
            let input = h.get_by_label("input image").rect();
            let output = h.get_by_label("output output").rect();
            assert!(kind.bottom() <= input.top() && input.bottom() <= output.top());
            assert!(h.query_by_label("pending").is_none());
        }
    }

    #[test]
    fn nodes_with_parameters_and_views_pop_them_out() {
        use crate::node_ui::Part;
        let mut h = harness(App::new(None, true, || {}));
        h.run();
        let graph = &h.state().project.graph;
        let histogram = graph.find("Histogram").unwrap();
        let with_params: Vec<_> =
            graph.nodes().filter(|(_, n)| !n.kind.params.is_empty()).map(|(id, _)| id).collect();
        let viewers: Vec<_> = graph
            .nodes()
            .filter(|(_, n)| crate::node_ui::binding(n.kind).is_some_and(|b| b.has_preparation()))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(h.get_all_by_label("⚙").count(), with_params.len());
        assert_eq!(h.get_all_by_label("🗗").count(), viewers.len());
        // AccessKit bounds ignore the canvas transform, so clicks go by action.
        let index = |ids: &[NodeId]| ids.iter().position(|&id| id == histogram).unwrap();
        h.get_all_by_label("⚙").nth(index(&with_params)).unwrap().click_accesskit();
        h.run();
        h.get_all_by_label("🗗").nth(index(&viewers)).unwrap().click_accesskit();
        h.run();
        let parameters = Popped { node: histogram, part: Part::Parameters };
        let view = Popped { node: histogram, part: Part::View };
        let titles: Vec<_> = h.state().windows().into_iter().map(|w| (w.popped, w.title)).collect();
        assert_eq!(
            titles,
            [
                (parameters, "Histogram parameters · Drip".into()),
                (view, "Histogram view · Drip".into())
            ]
        );
        assert!(h.query_by_label("shown in its window").is_some());
        h.state_mut().close_window(view);
        h.run();
        assert!(h.query_by_label("shown in its window").is_none());

        // Node ids are reused, so a new project closes every window.
        h.state_mut().set_project(templates::raw_to_tiff(), None).unwrap();
        assert!(h.state().windows().is_empty());
    }

    #[test]
    fn scope_nodes_offer_popouts() {
        for kind in
            [Registry.get("view.waveform").unwrap(), Registry.get("view.vectorscope").unwrap()]
        {
            let mut project = Project::default();
            let id = project.graph.add_node(kind).unwrap();
            let mut app = App::new(None, true, || {});
            app.set_project(project, None).unwrap();
            let mut h = harness(app);
            h.run();
            h.get_by_label("🗗").click_accesskit();
            h.run();
            let windows = h.state().windows();
            assert_eq!(windows.len(), 1);
            assert_eq!(windows[0].popped.node, id);
            assert_eq!(windows[0].title, format!("{} view · Drip", kind.name));
        }
    }

    mod declared_gui_view_gets_a_popout_before_preparation_succeeds_fixture {
        use super::*;

        #[drip::node(id="test.declared_view", category="test", name="Declared view", contract=empty_contract)]
        fn view(_: &KernelContext<'_>, _: &()) -> drip::Result<()> {
            Ok(())
        }
        struct TestGui;
        impl crate::node_ui::GuiNode for TestGui {
            type Parameters = ();
            type Presentation = PreviewImage;
            const ID: &'static str = "test.declared_view";
            const PREPARE: Option<crate::node_ui::Prepare<Self>> =
                Some(|_, _, _| Err(KernelError::Contract("no view yet".into())));
        }
        #[linkme::distributed_slice(crate::node_ui::BINDINGS)]
        static UI: crate::node_ui::Binding = crate::node_ui::Binding::new::<TestGui>();

        #[test]
        fn declared_gui_view_gets_a_popout_before_preparation_succeeds() {
            let mut project = Project::default();
            let id = project.graph.add_node(Registry.get("test.declared_view").unwrap()).unwrap();
            let mut app = App::new(None, true, || {});
            app.set_project(project, None).unwrap();
            let mut h = harness(app);
            h.run();
            h.get_by_label("🗗").click_accesskit();
            h.run();
            let windows = h.state().windows();
            assert_eq!(windows.len(), 1);
            assert_eq!(windows[0].popped.node, id);
            assert_eq!(windows[0].title, "Declared view view · Drip");
        }
    }

    mod custom_controls_keep_the_generic_view_and_popout_fixture {
        use super::*;

        #[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
        pub struct Settings {
            #[param(drip::param::ParamKind::Bool { default: false })]
            enabled: bool,
        }
        #[drip::node(id="test.custom_controls_view", category="test", name="Custom controls view", contract=empty_contract)]
        fn view(_: &KernelContext<'_>, _: &Settings) -> drip::Result<()> {
            Ok(())
        }
        fn controls(
            ui: &mut egui::Ui,
            cx: &mut crate::node_ui::ControlCx<'_, '_, '_, '_, Settings>,
        ) {
            cx.schema(ui);
            ui.label("Custom view controls");
            ui.label(format!("Typed enabled: {}", cx.parameters().enabled));
        }
        struct TestGui;
        impl crate::node_ui::GuiNode for TestGui {
            type Parameters = Settings;
            type Presentation = PreviewImage;
            const ID: &'static str = "test.custom_controls_view";
            const PREPARE: Option<crate::node_ui::Prepare<Self>> =
                Some(|_, _, _| Err(KernelError::Contract("no view yet".into())));
            fn controls(
                ui: &mut egui::Ui,
                cx: &mut crate::node_ui::ControlCx<'_, '_, '_, '_, Self::Parameters>,
            ) {
                controls(ui, cx);
            }
        }
        #[linkme::distributed_slice(crate::node_ui::BINDINGS)]
        static UI: crate::node_ui::Binding = crate::node_ui::Binding::new::<TestGui>();

        #[test]
        fn custom_controls_keep_the_generic_view_and_popout() {
            let mut project = Project::default();
            let id =
                project.graph.add_node(Registry.get("test.custom_controls_view").unwrap()).unwrap();
            let mut app = App::new(None, true, || {});
            app.set_project(project, None).unwrap();
            app.selected = Some(id);
            let mut h = harness(app);
            h.run();
            assert!(h.query_by_label("Custom view controls").is_some());
            assert!(h.query_by_label("Typed enabled: false").is_some());
            h.get_by_role(egui::accesskit::Role::CheckBox).click();
            h.run();
            assert!(h.query_by_label("Typed enabled: true").is_some());
            h.get_by_label("🗗").click_accesskit();
            h.run();
            let popped = Popped { node: id, part: crate::node_ui::Part::View };
            assert_eq!(h.state().windows()[0].popped, popped);
            assert!(h.query_by_label("shown in its window").is_some());
            assert!(h.query_by_label("Custom view controls").is_some());
            h.state_mut().close_window(popped);
            h.run();
            assert!(h.query_by_label("shown in its window").is_none());
            assert!(h.query_by_label("Custom view controls").is_some());
        }
    }

    #[test]
    fn parameter_windows_show_the_parameters_and_close_with_their_node() {
        for (name, labels) in [
            ("Export", &["profile", "intent", "depth", "export"][..]),
            (
                "Sigmoid",
                &["contrast", "skew", "preserve_hue", "−8 … +8 EV relative to middle grey"][..],
            ),
        ] {
            let mut app = App::new(None, true, || {});
            let id = app.project.graph.find(name).unwrap();
            let popped = Popped { node: id, part: crate::node_ui::Part::Parameters };
            app.popped.insert(popped);
            // The window opens at the size its content takes, smaller than a
            // guess yet with every parameter inside.
            let size = app.window_size(popped, 1.25);
            assert!(size.x < 360.0 && size.y < 320.0, "{size:?}");
            let mut h = Harness::builder()
                .with_size(size)
                .build_ui_state(move |ui, app: &mut App| app.window(ui, popped), app);
            h.run();
            let window = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for &label in labels {
                let rect = h.query_by_label(label).unwrap_or_else(|| panic!("{label}")).rect();
                assert!(window.contains_rect(rect), "{label} at {rect:?} outside {size:?}");
            }
            h.state_mut().project.graph.remove_node(id).unwrap();
            h.run();
            assert!(h.state().windows().is_empty());
        }
    }

    #[test]
    fn empty_canvas_above_the_nodes_takes_clicks() {
        let mut app = App::new(None, true, || {});
        app.selected = app.project.graph.find("Export");
        let mut h = harness(app);
        h.run();
        // Above the fitted graph, inside the area the nodes' layer spans.
        let at = egui::pos2(400.0, 150.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        h.event(egui::Event::PointerMoved(at));
        h.event(button(true));
        h.step();
        h.event(button(false));
        h.run();
        assert!(h.state().selected.is_none());
    }
}
