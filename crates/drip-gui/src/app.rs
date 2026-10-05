//! Application state and layout: one project, evaluated for what is visible.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use drip::eval::{Evaluator, run_action};
use drip::graph::NodeId;
use drip::node::Registry;
use drip::param::ParamKind;
use drip::project::Project;
use drip::value::View;
use drip::{nodes, templates};
use egui::{CentralPanel, Panel, RichText, Ui};

use crate::editor::Editor;
use crate::{inspector, preview, theme};

/// Preview levels never go coarser than 1/256 of the sensor.
const MAX_LEVEL: u8 = 8;

pub struct App {
    pub(crate) project: Project,
    pub(crate) registry: Registry,
    evaluator: Evaluator,
    file: Option<PathBuf>,
    pub(crate) selected: Option<NodeId>,
    editor: Editor,
    /// The downscale level previews are evaluated at, adapted to the preview
    /// panel so the image is at least as wide as the panel (DESIGN G2).
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
            level: 3,
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
        // Sinks are what the user looks at (views) or acts on (exports).
        let sinks: Vec<_> = self
            .project
            .graph
            .nodes()
            .filter(|(_, node)| node.kind.outputs.is_empty())
            .map(|(id, _)| id)
            .collect();
        self.evaluator.evaluate(&self.project.graph, self.level, &sinks);

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
                if let Some(View::Histogram(h)) = self.view(|v| matches!(v, View::Histogram(_))) {
                    ui.separator();
                    inspector::histogram(ui, &h);
                }
            });
        });
        Panel::bottom("editor").resizable(true).default_size(320.0).show(ui, |ui| {
            let evaluator = &self.evaluator;
            let error = self.editor.show(
                ui,
                &mut self.project.graph,
                &self.registry,
                &mut self.selected,
                |id| evaluator.result(id),
            );
            if let Some(error) = error {
                self.report(Err(error));
            }
        });
        CentralPanel::default().show(ui, |ui| self.preview(ui));
    }

    /// The first view `wanted` accepts, from the selected node if it has one,
    /// otherwise from the node with the smallest id.
    fn view(&self, wanted: impl Fn(&View) -> bool) -> Option<View> {
        let ids = self.selected.into_iter().chain(self.project.graph.nodes().map(|(id, _)| id));
        ids.filter_map(|id| self.evaluator.result(id).and_then(|r| r.as_ref().ok()?.view.clone()))
            .find(wanted)
    }

    fn preview(&mut self, ui: &mut Ui) {
        let Some(View::Image(value)) = self.view(|v| matches!(v, View::Image(_))) else {
            ui.centered_and_justified(|ui| ui.weak("no preview"));
            return;
        };
        let image = value.rgb().clone();
        let space = ui.available_rect_before_wrap();
        let fit = (space.width() / image.width as f32).min(space.height() / image.height as f32);
        let rect = egui::Rect::from_center_size(
            space.center(),
            egui::vec2(image.width as f32, image.height as f32) * fit,
        );
        ui.painter().add(preview::shape(rect, ui.id().with("preview"), image.clone()));

        let panel_px = space.width() * ui.ctx().pixels_per_point();
        // Finer as soon as the image is narrower than the panel; coarser only
        // with a margin, since cropping to whole Bayer cells makes each level
        // slightly less than half the previous and would otherwise oscillate.
        let ratio = image.width as f32 / panel_px;
        let shift = if ratio < 1.0 { ratio.log2() } else { (ratio / 1.05).log2().max(0.0) };
        let shift = shift.floor() as i32;
        let level = (i32::from(self.level) + shift).clamp(0, i32::from(MAX_LEVEL)) as u8;
        if level != self.level {
            log::debug!("preview level {} -> {level}", self.level);
            self.level = level;
            ui.ctx().request_repaint();
        }
    }

    fn menu(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                self.set_project(templates::raw_to_tiff(), None);
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

    fn set_project(&mut self, project: Project, file: Option<PathBuf>) {
        self.project = project;
        self.file = file;
        self.selected = None;
        self.editor = Editor::default();
        self.evaluator = Evaluator::default();
    }

    fn open(&mut self, file: PathBuf) {
        let loaded = std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|text| Project::from_json(&text, &self.registry).map_err(|e| e.to_string()));
        match loaded {
            Ok(project) => {
                let text = format!("opened {}", file.display());
                self.set_project(project, Some(file));
                self.report(Ok(text));
            }
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
        std::thread::spawn(move || {
            let result = run_action(&graph, id, name).map_err(|e| e.to_string());
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
        Harness::new_ui_state(|ui, app: &mut App| app.ui(ui), app)
    }

    #[test]
    fn opens_a_project_previews_it_and_starts_anew() {
        let file = fixture_project("open");
        let mut h = harness(App::new(Some(file.clone()), true));
        h.run();
        assert!(
            h.state().status.as_ref().is_some_and(|s| !s.error && s.text.starts_with("opened"))
        );
        assert!(h.query_by_label("no preview").is_none());
        assert!(h.state().level < 6, "the preview level adapts to the panel");

        h.get_by_label("New").click();
        h.run();
        assert!(h.query_by_label("no preview").is_some());
        std::fs::remove_file(file).unwrap();
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
    fn inspector_shows_the_selected_node_and_its_actions() {
        let mut app = App::new(None, true);
        app.selected = app.project.graph.find("export");
        let mut h = harness(app);
        h.run();
        for label in ["profile", "intent", "depth", "export", "path (input)", "raw · path"] {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
    }
}
