//! Application state and layout: one project, evaluated at a global detail
//! level, in a main window and a window per popped-out part of a node.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use drip::graph::NodeId;
use drip::node::Registry;
use drip::project::Project;
use drip::{nodes, templates};
use egui::{CentralPanel, Panel, RichText, Ui, Vec2};

use crate::editing::{Frame, NodeCx, Report};
use crate::editor::Editor;
use crate::node_ui::{self, Popped};
use crate::worker::{Notice, Worker};
use crate::{inspector, theme};

/// Preview levels never go coarser than 1/256 of the sensor.
const MAX_LEVEL: u8 = 8;
const DEFAULT_LEVEL: u8 = 1;

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
            registry: nodes::registry(),
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

    /// Shows `result` in the status line and the log.
    fn report(&mut self, result: Result<String, String>) {
        let (error, text) = match result {
            Ok(text) => (false, text),
            Err(text) => (true, text),
        };
        if error {
            log::error!("{text}");
        } else {
            log::info!("{text}");
        }
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
            Window { popped, title: popped.title(node) }
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
        node_ui::fitted(&ctx).unwrap_or_else(|| popped.size(node))
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
        self.redraw |= report.redraw;
        self.dirty |= report.edited;
        if let Some(refused) = report.refused {
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
            egui::ComboBox::from_label("Preview detail")
                .width(64.0)
                .selected_text(label(self.level))
                .show_ui(ui, |ui| {
                    for level in 0..=MAX_LEVEL {
                        ui.selectable_value(&mut self.level, level, label(level));
                    }
                })
                .response
                .on_hover_text("Global preview scale. Export always uses full detail.");
            if self.level != previous {
                self.project.ui["preview_level"] = serde_json::json!(self.level);
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
                ui.label("evaluating…");
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
        if !project.ui.is_null() && !project.ui.is_object() {
            return Err("project UI state must be an object".into());
        }
        let level = match project.ui.get("preview_level") {
            None => DEFAULT_LEVEL,
            Some(value) => {
                value.as_u64().filter(|&level| level <= u64::from(MAX_LEVEL)).ok_or_else(|| {
                    format!("preview level must be an integer from 0 to {MAX_LEVEL}")
                })? as u8
            }
        };
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
        let loaded = std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|text| Project::from_json(&text, &self.registry).map_err(|e| e.to_string()))
            .and_then(|project| self.set_project(project, Some(file.clone())));
        match loaded {
            Ok(()) => self.report(Ok(format!("opened {}", file.display()))),
            Err(e) => self.report(Err(format!("{}: {e}", file.display()))),
        }
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
            self.save(&file, self.project.template());
        } else if self.save(&file, self.project.clone()) {
            self.file = Some(file);
        }
    }

    /// Writes `project` to `file`; returns whether that succeeded.
    fn save(&mut self, file: &Path, project: Project) -> bool {
        let result = std::fs::write(file, project.to_json());
        let saved = result.is_ok();
        let shown = file.display();
        self.report(result.map(|()| format!("saved {shown}")).map_err(|e| format!("{shown}: {e}")));
        saved
    }

    fn invalidate_cache(&mut self) {
        self.dirty = true;
        let result = self.worker.invalidate();
        self.report(result.map(|()| "invalidating cache…".into()));
    }

    /// The evaluator-owning worker snapshots resources in command order.
    fn run_action(&mut self, id: NodeId, name: &'static str) {
        match self.worker.action(&self.project.graph, id, name) {
            Ok(()) => {
                self.action = Some(name);
                self.report(Ok(format!("running {name}…")));
            }
            Err(error) => self.report(Err(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use drip::image::DisplayRec2020;
    use drip::node::{EvalContext, NodeKernel};
    use drip::param::Params;
    use drip::view::PreviewImage;

    use crate::views::PreparedView as View;
    use drip::param::ParamKind;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::*;

    /// The built-in template applied to the fixture raw, saved to a temp file.
    fn fixture_project(name: &str) -> PathBuf {
        let raw =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-ilce-7rm3.arw");
        let mut p = templates::raw_to_tiff();
        let read = p.graph.find("raw").unwrap();
        p.graph.set_param(read, "path", json!(raw)).unwrap();
        let file =
            std::env::temp_dir().join(format!("drip-gui-{name}-{}.drip", std::process::id()));
        std::fs::write(&file, p.to_json()).unwrap();
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
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
        let id = app.project.graph.find("sigmoid").unwrap();
        let popped = Popped { node: id, part: node_ui::Part::Parameters };
        app.popped.insert(popped);
        app.take_redraw();

        for change in [
            crate::editing::Edit::Label(id, "tone"),
            crate::editing::Edit::External(id, "contrast", true),
            crate::editing::Edit::Ui(id, "pos", egui::vec2(40.0, 50.0)),
        ] {
            edit(app, change);
            assert!(app.take_redraw(), "other windows must see the shared edit");
            assert!(!app.worker.busy(), "presentation edits need no evaluation");
        }
        assert_eq!(app.windows()[0].title, "tone parameters · Drip");
        assert!(app.project.graph.inputs().any(|input| input == (id, "contrast")));

        edit(app, crate::editing::Edit::Label(id, ""));
        assert_eq!(app.project.graph.node(id).unwrap().label, "tone");
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
        for name in ["preview", "histogram", "waveform", "vectorscope"] {
            assert!(has_view(app, name), "{name}");
        }
        let id = app.project.graph.find("preview").unwrap();
        let Some(View::Image(image)) = app.worker.result(id).unwrap().as_ref().unwrap().clone()
        else {
            panic!("an image")
        };
        assert_eq!(app.level, DEFAULT_LEVEL);
        assert_eq!(image.width, 3984, "1/2 preview with full-size RCD");
        h.get_by_label("Invalidate cache").click();
        settle(&mut h);
        let Some(View::Image(refreshed)) = h.state().worker.result(id).unwrap().as_ref().unwrap()
        else {
            panic!("an image")
        };
        assert!(!std::sync::Arc::ptr_eq(&image, refreshed));
        assert_eq!(image.texels, refreshed.texels);
        h.get_by_label("Preview detail").click();
        settle(&mut h);
        h.get_by_label("1/4").click();
        settle(&mut h);
        assert_eq!(h.state().level, 2);
        let app = h.state();
        let Some(View::Image(image)) = app.worker.result(id).unwrap().as_ref().unwrap() else {
            panic!("an image")
        };
        assert_eq!(image.width, 1992);
        let histogram = app.project.graph.find("histogram").unwrap();
        let Some(View::Histogram(histogram)) =
            app.worker.result(histogram).unwrap().as_ref().unwrap()
        else {
            panic!("a histogram")
        };
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
        let restored = App::new(Some(file.clone()), true, || {});
        assert_eq!(restored.level, 2);
        assert!(!restored.status.unwrap().error);

        h.get_by_label("New").click();
        settle(&mut h);
        assert!(!has_view(h.state(), "preview"), "the new template has no raw file yet");
        assert_eq!(h.state().level, DEFAULT_LEVEL);
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn evaluation_is_independent_of_node_positions() {
        use drip::node::{Evaluated, NodeKind};
        use drip::param::ParamSpec;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static CALLS: AtomicUsize = AtomicUsize::new(0);
        static NODE: NodeKind = NodeKind::new::<OffscreenKernel>(
            "test.offscreen",
            "offscreen",
            &[ParamSpec::new("value", ParamKind::Bool { default: false })],
            &[],
            &[],
        );
        struct OffscreenKernel;
        impl NodeKernel for OffscreenKernel {
            type Inputs = ();
            type Outputs = ();

            fn eval(
                _: Params<'_>,
                (): (),
                _: &EvalContext<'_>,
            ) -> Result<Evaluated<Self::Outputs>, String> {
                CALLS.fetch_add(1, Ordering::SeqCst);
                Ok(Evaluated::default())
            }
        }

        let mut project = Project::default();
        let left = project.graph.add_node(&NODE);
        let right = project.graph.add_node(&NODE);
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
        assert_eq!(CALLS.load(Ordering::SeqCst), 3, "off-screen edits are evaluated");
        let before = completions.load(Ordering::SeqCst);
        h.state_mut().project.graph.set_ui(left, json!({"pos": [0, 0]})).unwrap();
        settle(&mut h);
        assert_eq!(completions.load(Ordering::SeqCst), before, "moving nodes requests no work");
    }

    #[test]
    fn evaluating_message_keeps_the_ui_and_previous_preview_available() {
        use drip::image::Rgb;
        use drip::node::{Evaluated, NodeKind};
        use drip::param::ParamSpec;
        use std::sync::{Arc, Mutex, mpsc};
        use std::time::{Duration, Instant};
        static GATE: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
        static SLOW: NodeKind = NodeKind::new::<SlowKernel>(
            "test.slow",
            "slow",
            &[ParamSpec::new("block", ParamKind::Bool { default: false })],
            &[],
            &[],
        );
        struct SlowKernel;
        impl NodeKernel for SlowKernel {
            type Inputs = ();
            type Outputs = ();

            fn eval(
                p: Params<'_>,
                (): (),
                _: &EvalContext<'_>,
            ) -> Result<Evaluated<Self::Outputs>, String> {
                if p.bool("block") {
                    let gate = GATE.lock().unwrap();
                    let (started, release) = gate.as_ref().unwrap();
                    started.send(()).unwrap();
                    release.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                Ok(Evaluated {
                    outputs: (),
                    view: Some(drip::view::View::Image(PreviewImage::new(&*Arc::new(
                        DisplayRec2020::from(Arc::new(Rgb {
                            width: 1,
                            height: 1,
                            scale: 1,
                            pixels: vec![[0.5; 3]],
                        })),
                    )))),
                })
            }
        }

        let (started, entered) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        *GATE.lock().unwrap() = Some((started, resume));
        let mut project = Project::default();
        let id = project.graph.add_node(&SLOW);
        let mut app = App::new(None, true, || {});
        app.set_project(project, None).unwrap();
        let mut h = harness(app);
        settle(&mut h);
        let Some(View::Image(before)) = h.state().worker.result(id).unwrap().as_ref().unwrap()
        else {
            panic!("image")
        };
        let before = before.clone();
        set_param(&mut h, id, "block", json!(true));
        h.step();
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let start = Instant::now();
        h.step();
        assert!(h.query_by_label("evaluating…").is_some());
        let Some(View::Image(shown)) = h.state().worker.result(id).unwrap().as_ref().unwrap()
        else {
            panic!("previous image")
        };
        assert!(Arc::ptr_eq(shown, &before));
        h.get_by_label("Preview detail").click();
        h.run();
        assert!(h.query_by_label("Full").is_some(), "controls respond while the node is blocked");
        assert!(start.elapsed() < Duration::from_secs(2));
        release.send(()).unwrap();
        settle(&mut h);
        assert!(h.query_by_label("evaluating…").is_none());
        assert!(h.query_by_label("done").is_some());
    }

    #[test]
    fn invalid_detail_does_not_replace_the_open_project() {
        let mut app = App::new(None, true, || {});
        let original = app.project.clone();
        for value in [json!(-1), json!(9), json!(256), json!(2.5), json!("2"), json!(null)] {
            let mut project = templates::raw_to_tiff();
            project.ui = json!({"preview_level": value});
            assert!(app.set_project(project, None).is_err());
            assert_eq!(app.project, original);
            assert_eq!(app.level, DEFAULT_LEVEL);
        }
    }

    #[test]
    fn reports_unreadable_projects() {
        let mut h = harness(App::new(Some("/nonexistent.drip".into()), true, || {}));
        h.run();
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
        app.selected = app.project.graph.find("export");
        let mut h = harness(app);
        h.run();
        for label in
            ["profile", "intent", "depth", "export", "path (input)", "raw · path", "export · path"]
        {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
    }

    #[test]
    fn node_help_follows_the_type_id_and_stays_out_of_parameter_windows() {
        let mut app = App::new(None, true, || {});
        let id = app.project.graph.find("sigmoid").unwrap();
        app.selected = Some(id);
        let mut h = harness(app);
        h.run();
        let kind = h.get_by_label("tone.sigmoid").rect();
        let input = h.get_by_label("scn rec2020 img").rect();
        let output = h.get_by_label("disp rec2020 img").rect();
        let control = h.get_by_label("contrast").rect();
        assert!(h.query_by_label("darktable: sigmoid").is_some());
        assert!(kind.bottom() <= input.top());
        assert!(input.bottom() <= output.top());
        assert!(output.bottom() <= control.top());

        let popped = Popped { node: id, part: node_ui::Part::Parameters };
        let mut popup = Harness::builder().with_size(egui::vec2(360.0, 320.0)).build_ui_state(
            move |ui, app: &mut App| app.window(ui, popped),
            App::new(None, true, || {}),
        );
        popup.run();
        assert!(popup.query_by_label("tone.sigmoid").is_some());
        assert!(popup.query_by_label("contrast").is_some());
        assert!(popup.query_by_label("scn rec2020 img").is_none());
        assert!(popup.query_by_label("disp rec2020 img").is_none());
        assert!(popup.query_by_label("darktable: sigmoid").is_none());
    }

    #[test]
    fn nodes_without_controls_show_documentation_and_references() {
        let mut app = App::new(None, true, || {});
        app.selected = app.project.graph.find("demosaic");
        let mut h = harness(app);
        h.run();
        let kind = h.get_by_label("demosaic.rcd").rect();
        let input = h.get_by_label("sensor mosaic").rect();
        let output = h.get_by_label("camera RGB img").rect();
        let reference = h.get_by_label("RCD algorithm").rect();
        assert!(kind.bottom() <= input.top());
        assert!(input.bottom() <= output.top());
        assert!(output.bottom() <= reference.top());
    }

    #[test]
    fn nodes_with_parameters_and_views_pop_them_out() {
        use crate::node_ui::Part;
        let mut h = harness(App::new(None, true, || {}));
        h.run();
        let graph = &h.state().project.graph;
        let histogram = graph.find("histogram").unwrap();
        let with_params: Vec<_> =
            graph.nodes().filter(|(_, n)| !n.kind.params.is_empty()).map(|(id, _)| id).collect();
        let viewers: Vec<_> = graph
            .nodes()
            .filter(|(_, n)| {
                [&nodes::PREVIEW, &nodes::HISTOGRAM, &nodes::WAVEFORM, &nodes::VECTORSCOPE]
                    .contains(&n.kind)
            })
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
        let view = Popped { node: histogram, part: Part::Gui("view") };
        let titles: Vec<_> = h.state().windows().into_iter().map(|w| (w.popped, w.title)).collect();
        assert_eq!(
            titles,
            [
                (parameters, "histogram parameters · Drip".into()),
                (view, "histogram view · Drip".into())
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
        for kind in [&nodes::WAVEFORM, &nodes::VECTORSCOPE] {
            let mut project = Project::default();
            let id = project.graph.add_node(kind);
            let mut app = App::new(None, true, || {});
            app.set_project(project, None).unwrap();
            let mut h = harness(app);
            h.run();
            h.get_by_label("🗗").click_accesskit();
            h.run();
            let windows = h.state().windows();
            assert_eq!(windows.len(), 1);
            assert_eq!(windows[0].popped.node, id);
            assert_eq!(windows[0].title, format!("{} view · Drip", kind.label));
        }
    }

    #[test]
    fn parameter_windows_show_the_parameters_and_close_with_their_node() {
        for (name, labels) in [
            ("export", &["profile", "intent", "depth", "export"][..]),
            (
                "sigmoid",
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
            h.state_mut().project.graph.remove_node(id);
            h.run();
            assert!(h.state().windows().is_empty());
        }
    }

    #[test]
    fn empty_canvas_above_the_nodes_takes_clicks() {
        let mut app = App::new(None, true, || {});
        app.selected = app.project.graph.find("export");
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
