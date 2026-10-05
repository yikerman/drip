//! Application state and layout: one project, evaluated for what is visible.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use drip::eval::Evaluator;
use drip::graph::NodeId;
use drip::node::Registry;
use drip::param::ParamKind;
use drip::project::Project;
use drip::{nodes, templates};
use egui::{CentralPanel, Panel, RichText, Ui};

use crate::editor::{Editor, Frame};
use crate::{inspector, theme};

/// Preview levels never go coarser than 1/256 of the sensor.
const MAX_LEVEL: u8 = 8;
const DEFAULT_LEVEL: u8 = 3;

pub struct App {
    pub(crate) project: Project,
    registry: Registry,
    pub(crate) evaluator: Evaluator,
    file: Option<PathBuf>,
    pub(crate) selected: Option<NodeId>,
    editor: Editor,
    /// One user-selected level for every preview target (DESIGN E16).
    level: u8,
    status: Option<Status>,
    action: Option<mpsc::Receiver<Result<(), String>>>,
    wide_gamut: bool,
}

struct Status {
    error: bool,
    text: String,
}

impl App {
    pub fn new(file: Option<PathBuf>, wide_gamut: bool) -> Self {
        let mut app = App {
            project: templates::raw_to_tiff(),
            registry: nodes::registry(),
            evaluator: Evaluator::default(),
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
        self.poll_action(ui.ctx());
        let evaluator = &self.evaluator;
        let visible = self.editor.visible(&self.project.graph, |id| evaluator.result(id));
        self.evaluator.evaluate(&self.project.graph, self.level, &visible);

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
            let evaluator = &self.evaluator;
            let results = |id| evaluator.result(id);
            let mut frame = Frame { results: &results, refused: None };
            self.editor.show(
                ui,
                &mut self.project.graph,
                &self.registry,
                &mut self.selected,
                &mut frame,
            );
            if let Some(refused) = frame.refused {
                self.report(Err(refused));
            }
        });
    }

    fn menu(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                self.set_project(templates::raw_to_tiff(), None).expect("valid template");
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
            if ui.button("Reload files").clicked() {
                self.reload();
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
                ui.ctx().request_repaint();
            }
        });
    }

    fn status_line(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if !self.wide_gamut {
                ui.label(RichText::new("previews clipped to sRGB").color(theme::ERROR));
            }
            if let Some(status) = &self.status {
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
        self.evaluator = self.evaluator.fork(&self.project.graph);
        Ok(())
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

    /// Re-reads every file the project names, recomputing what depends on them.
    fn reload(&mut self) {
        let paths: Vec<PathBuf> = self
            .project
            .graph
            .nodes()
            .flat_map(|(_, node)| {
                let paths = node
                    .kind
                    .params
                    .iter()
                    .filter(|spec| matches!(spec.kind, ParamKind::Path { .. }));
                paths.filter_map(|spec| node.params[spec.name].as_str().map(PathBuf::from))
            })
            .collect();
        for path in &paths {
            self.evaluator.reload(path);
        }
        self.report(Ok(format!("reloaded {} files", paths.len())));
    }

    /// Runs a node action on a snapshot of the graph in the background.
    pub(crate) fn run_action(&mut self, id: NodeId, name: &'static str) {
        let (tx, rx) = mpsc::channel();
        let graph = self.project.graph.clone();
        let evaluator = self.evaluator.fork(&graph);
        std::thread::spawn(move || {
            let result = evaluator.run_action(&graph, id, name).map_err(|e| e.to_string());
            // The receiver is gone only if the app quit; nothing is left to tell.
            let _ = tx.send(result);
        });
        self.action = Some(rx);
        self.report(Ok(format!("running {name}…")));
    }

    pub(crate) fn action_running(&self) -> bool {
        self.action.is_some()
    }

    fn poll_action(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.action else { return };
        match rx.try_recv() {
            Ok(result) => {
                self.action = None;
                self.report(result.map(|()| "done".into()));
            }
            Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.action = None;
                self.report(Err("the action crashed".into()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use drip::value::View;
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

    fn has_view(app: &App, label: &str) -> bool {
        let id = app.project.graph.find(label).unwrap();
        app.evaluator.result(id).is_some_and(|r| r.as_ref().is_ok_and(|e| e.view.is_some()))
    }

    #[test]
    fn previews_use_the_selected_global_detail() {
        let file = fixture_project("open");
        let mut h = harness(App::new(Some(file.clone()), true));
        h.run();
        let app = h.state();
        assert!(app.status.as_ref().is_some_and(|s| !s.error && s.text.starts_with("opened")));
        assert!(has_view(app, "preview") && has_view(app, "histogram"));
        let id = app.project.graph.find("preview").unwrap();
        let Some(View::Image(image)) =
            app.evaluator.result(id).unwrap().as_ref().unwrap().view.clone()
        else {
            panic!("an image")
        };
        assert_eq!(app.level, DEFAULT_LEVEL);
        assert_eq!(image.rgb().scale, 16, "1/8 preview plus 2×2 debayer");
        let raw = app.project.graph.find("raw").unwrap();
        let cached = std::sync::Arc::downgrade(
            app.evaluator.result(raw).unwrap().as_ref().unwrap().outputs[0].mosaic(),
        );

        h.get_by_label("Preview detail").click();
        h.run();
        h.get_by_label("1/4").click();
        h.run();
        assert_eq!(h.state().level, 2);
        let app = h.state();
        let Some(View::Image(image)) = &app.evaluator.result(id).unwrap().as_ref().unwrap().view
        else {
            panic!("an image")
        };
        assert_eq!(image.rgb().scale, 8);
        let histogram = app.project.graph.find("histogram").unwrap();
        let Some(View::Histogram(histogram)) =
            &app.evaluator.result(histogram).unwrap().as_ref().unwrap().view
        else {
            panic!("a histogram")
        };
        assert_eq!(
            histogram.counts.iter().map(|bin| bin[0] as usize).sum::<usize>(),
            image.rgb().pixels.len()
        );
        let image = std::sync::Arc::downgrade(image.rgb());
        h.state_mut().project.graph.set_ui(id, json!({"size": [1200.0, 900.0]})).unwrap();
        h.run();
        assert_eq!(h.state().level, 2, "view size does not select resolution");
        assert!(image.upgrade().is_some(), "resizing preserves the evaluated image");
        h.get_by_label("Save").click();
        h.run();
        let restored = App::new(Some(file.clone()), true);
        assert_eq!(restored.level, 2);
        assert!(!restored.status.unwrap().error);

        h.get_by_label("New").click();
        h.run();
        assert!(!has_view(h.state(), "preview"), "the new template has no raw file yet");
        assert_eq!(h.state().level, DEFAULT_LEVEL);
        assert!(cached.upgrade().is_some(), "raw levels survive project changes");
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn invalid_detail_does_not_replace_the_open_project() {
        let mut app = App::new(None, true);
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
        let mut h = harness(App::new(Some("/nonexistent.drip".into()), true));
        h.run();
        assert!(h.state().status.as_ref().is_some_and(|s| s.error));
    }

    #[test]
    fn warns_when_previews_are_clipped() {
        let mut h = harness(App::new(None, false));
        h.run();
        assert!(h.query_by_label("previews clipped to sRGB").is_some());
        let mut h = harness(App::new(None, true));
        h.run();
        assert!(h.query_by_label("previews clipped to sRGB").is_none());
    }

    #[test]
    fn inspector_shows_the_selected_node_and_the_inputs() {
        let mut app = App::new(None, true);
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
