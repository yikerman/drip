//! Application state and layout: one project, evaluated at a global detail level.

use std::path::{Path, PathBuf};

use drip::graph::NodeId;
use drip::node::Registry;
use drip::project::Project;
use drip::{nodes, templates};
use egui::{CentralPanel, Panel, RichText, Ui};

use crate::editor::{Editor, Frame};
use crate::worker::{Notice, Worker};
use crate::{inspector, theme};

/// Preview levels never go coarser than 1/256 of the sensor.
const MAX_LEVEL: u8 = 8;
const DEFAULT_LEVEL: u8 = 1;

pub struct App {
    pub(crate) project: Project,
    registry: Registry,
    worker: Worker,
    dirty: bool,
    file: Option<PathBuf>,
    pub(crate) selected: Option<NodeId>,
    editor: Editor,
    /// One user-selected level for every preview target (DESIGN E16).
    level: u8,
    status: Option<Status>,
    action: Option<&'static str>,
    wide_gamut: bool,
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
        };
        if let Some(file) = file {
            app.open(file);
        }
        app
    }

    /// Shows `result` in the status line and the log.
    pub(crate) fn report(&mut self, result: Result<String, String>) {
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
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let mut evaluated = None;
        let mut action_reported = false;
        for notice in self.worker.poll() {
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

        Panel::top("menu").show_separator_line(false).show(ui, |ui| self.menu(ui));
        Panel::bottom("status").show_separator_line(false).show(ui, |ui| self.status_line(ui));
        Panel::right("inspector").resizable(true).default_size(320.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(id) = self.selected.filter(|id| self.project.graph.node(*id).is_some())
                {
                    inspector::node(self, ui, id);
                    ui.separator();
                }
                inspector::inputs(self, ui);
            });
        });
        CentralPanel::default().show(ui, |ui| {
            let results = |id| self.worker.result(id);
            let mut frame = Frame { results: &results, refused: None, changed: false };
            self.editor.show(
                ui,
                &mut self.project.graph,
                &self.registry,
                &mut self.selected,
                &mut frame,
            );
            self.dirty |= frame.changed;
            if let Some(refused) = frame.refused {
                self.report(Err(refused));
            }
        });
        if std::mem::take(&mut self.dirty) {
            let targets = self.project.graph.nodes().map(|(id, _)| id).collect();
            if let Err(error) = self.worker.request(&self.project.graph, self.level, targets) {
                self.report(Err(error));
            }
            ui.ctx().request_repaint();
        }
    }

    pub fn after_frame(&mut self) {
        self.worker.after_frame();
    }

    pub(crate) fn set_param(&mut self, id: NodeId, name: &str, value: serde_json::Value) {
        match self.project.graph.set_param(id, name, value) {
            Ok(()) => self.dirty = true,
            Err(error) => self.report(Err(error.to_string())),
        }
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
        self.dirty = true;
        self.worker.reset(&self.project.graph)
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
        let file = file.with_extension("drip");
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
    pub(crate) fn run_action(&mut self, id: NodeId, name: &'static str) {
        match self.worker.action(&self.project.graph, id, name) {
            Ok(()) => {
                self.action = Some(name);
                self.report(Ok(format!("running {name}…")));
            }
            Err(error) => self.report(Err(error)),
        }
    }

    pub(crate) fn action_running(&self) -> bool {
        self.action.is_some()
    }
}

#[cfg(test)]
mod tests {
    use crate::worker::PreparedView as View;
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
        Harness::builder()
            .with_size(egui::vec2(1600.0, 1000.0))
            .build_ui_state(|ui, app: &mut App| app.ui(ui), app)
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
        assert!(has_view(app, "preview") && has_view(app, "histogram"));
        let id = app.project.graph.find("preview").unwrap();
        let Some(View::Image(image)) = app.worker.result(id).unwrap().as_ref().unwrap().clone()
        else {
            panic!("an image")
        };
        assert_eq!(app.level, DEFAULT_LEVEL);
        assert_eq!(image.width, 1992, "1/2 preview plus 2×2 debayer");
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
        assert_eq!(image.width, 996);
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
        static NODE: NodeKind = NodeKind {
            name: "test.offscreen",
            label: "offscreen",
            params: &[ParamSpec::new("value", ParamKind::Bool { default: false })],
            inputs: &[],
            outputs: &[],
            actions: &[],
            eval: |_, _, _| {
                CALLS.fetch_add(1, Ordering::SeqCst);
                Ok(Evaluated::default())
            },
        };
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

        h.state_mut().set_param(left, "value", json!(true));
        settle(&mut h);
        assert_eq!(CALLS.load(Ordering::SeqCst), 3, "off-screen edits are evaluated");
        let before = completions.load(Ordering::SeqCst);
        h.state_mut().project.graph.set_ui(left, json!({"pos": [0, 0]})).unwrap();
        settle(&mut h);
        assert_eq!(completions.load(Ordering::SeqCst), before, "moving nodes requests no work");
    }

    #[test]
    fn evaluating_message_keeps_the_ui_and_previous_preview_available() {
        use drip::node::{Evaluated, NodeKind};
        use drip::param::ParamSpec;
        use drip::value::{Rgb, Value};
        use std::sync::{Arc, Mutex, mpsc};
        use std::time::{Duration, Instant};
        static GATE: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
        static SLOW: NodeKind =
            NodeKind {
                name: "test.slow",
                label: "slow",
                params: &[ParamSpec::new("block", ParamKind::Bool { default: false })],
                inputs: &[],
                outputs: &[],
                actions: &[],
                eval: |p, _, _| {
                    if p.bool("block") {
                        let gate = GATE.lock().unwrap();
                        let (started, release) = gate.as_ref().unwrap();
                        started.send(()).unwrap();
                        release.recv_timeout(Duration::from_secs(5)).unwrap();
                    }
                    Ok(Evaluated {
                        outputs: vec![],
                        view: Some(drip::value::View::Image(Value::DisplayRec2020(Arc::new(
                            Rgb { width: 1, height: 1, scale: 1, pixels: vec![[0.5; 3]] },
                        )))),
                    })
                },
            };
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
        h.state_mut().set_param(id, "block", json!(true));
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
}
