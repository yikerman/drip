//! Application state and layout: one project, evaluated at a global detail
//! level, in a main window and a window per popped-out part of a node.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::history::History;
use crate::model::NodeId;
use crate::model::Project;
use crate::templates;
use drip::store::Disk;
use egui::{CentralPanel, Panel, RichText, Ui, Vec2};

use crate::editing::{Edit, Frame, NodeCx, Report};
use crate::editor::Editor;
use crate::node_ui::{self, Part, Popped};
use crate::theme::Tip;
use crate::ui_state::{self, DEFAULT_LEVEL, MAX_LEVEL};
use crate::worker::{Notice, Worker};
use crate::{inspector, theme};

mod commands;
use commands::COMMANDS;

pub struct App {
    project: Project,
    /// Serves the current project only; replaced with it.
    worker: Worker,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The disk tier each project's worker keeps foreign outputs in.
    disk: Option<Disk>,
    dirty: bool,
    modified: bool,
    /// Namespaces temporary control drafts when node IDs are reused by a new project.
    ui_generation: u64,
    file: Option<PathBuf>,
    selected: Option<NodeId>,
    editor: Editor,
    /// One user-selected level for every preview target.
    level: u8,
    status: Option<Status>,
    action: Option<ActionStatus>,
    wide_gamut: bool,
    /// Parts of nodes shown in their own windows; not saved.
    popped: BTreeSet<Popped>,
    /// Whether the windows need redrawing because what they show changed.
    redraw: bool,
    /// A command that replaces the project, waiting until unsaved edits are
    /// saved or discarded.
    replacing: Option<commands::Replace>,
    history: History,
    /// The window whose pointer gesture made edits not yet in the history;
    /// `Some(None)` is the main window.
    editing: Option<Option<Popped>>,
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

struct ActionStatus {
    running: String,
    completed: String,
}

impl App {
    pub fn new(
        file: Option<PathBuf>,
        wide_gamut: bool,
        disk: Option<Disk>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let project = templates::default_project();
        let mut app = App {
            history: History::new(project.graph.clone()),
            project,
            worker: Worker::new(wake.clone(), disk.clone()),
            wake,
            disk,
            dirty: true,
            modified: false,
            ui_generation: 0,
            file: None,
            selected: None,
            editor: Editor::default(),
            level: DEFAULT_LEVEL,
            status: None,
            action: None,
            wide_gamut,
            popped: BTreeSet::new(),
            redraw: false,
            replacing: None,
            editing: None,
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
        self.shortcuts(ui.ctx());
        ui.push_id(self.ui_generation, |ui| self.main_ui(ui));
        self.unsaved_prompt(ui.ctx());
    }

    fn main_ui(&mut self, ui: &mut Ui) {
        Panel::top("menu").show_separator_line(false).show(ui, |ui| self.menu(ui));
        Panel::bottom("status").show_separator_line(false).show(ui, |ui| self.status_line(ui));
        let results = |id| self.worker.result(id);
        let details = |id| self.worker.detail_view(id);
        let mut frame = Frame::new(&results, &details, &self.popped, self.actions_disabled());
        let graph = &mut self.project.graph;
        Panel::right("inspector").resizable(true).default_size(320.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(id) = self.selected.filter(|id| graph.kind(*id).is_some()) {
                    inspector::selected_node(ui, &mut NodeCx::new(graph, id, &mut frame));
                    ui.separator();
                }
                inspector::inputs(ui, graph, &mut frame);
            });
        });
        CentralPanel::default().show(ui, |ui| {
            self.editor.show(ui, graph, &mut self.selected, &mut frame);
        });
        let report = frame.report;
        let changed = report.changed;
        self.apply(report);
        self.record(ui.ctx(), None, changed);
    }

    pub fn window(&mut self, ui: &mut Ui, popped: Popped) {
        self.shortcuts(ui.ctx());
        let results = |id| self.worker.result(id);
        let details = |id| self.worker.detail_view(id);
        let mut frame = Frame::new(&results, &details, &self.popped, self.actions_disabled());
        let graph = &mut self.project.graph;
        CentralPanel::default().show(ui, |ui| {
            if graph.kind(popped.node).is_some() {
                popped.show(ui, &mut NodeCx::new(graph, popped.node, &mut frame));
            }
        });
        let report = frame.report;
        let (changed, detail) = (report.changed, report.detail);
        self.apply(report);
        if popped.part == Part::View {
            self.detail(popped.node, detail);
        }
        self.record(ui.ctx(), Some(popped), changed);
        self.unsaved_prompt(ui.ctx());
    }

    /// Asks the worker for the full-detail crop a view's window wants.
    fn detail(&mut self, node: NodeId, region: Option<drip::shape::Rect>) {
        if let Err(error) = self.worker.detail(&self.project.graph, node, region) {
            self.report(Err(error));
        }
    }

    pub fn windows(&self) -> Vec<Window> {
        let window = |popped: Popped| {
            let name = self.project.graph.name(popped.node).expect("popped nodes exist");
            Window { popped, title: popped.title(name) }
        };
        self.popped.iter().map(|&popped| window(popped)).collect()
    }

    pub fn title(&self) -> String {
        format!("{}{} · Drip", self.name(), if self.modified { " *" } else { "" })
    }

    fn name(&self) -> std::borrow::Cow<'_, str> {
        self.file
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(|| "Untitled".into(), |name| name.to_string_lossy())
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
        if popped.part == Part::View {
            self.detail(popped.node, None);
        }
        self.redraw = true;
    }

    /// Whether the windows need redrawing since the last call.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// Applies what a frame's GUIs did and requests evaluation after edits.
    fn apply(&mut self, report: Report) {
        self.redraw |= report.changed;
        self.modified |= report.changed;
        self.dirty |= report.edited;
        if let Some(refused) = report.refused {
            log::debug!("graph edit refused: {refused}");
            self.report(Err(refused));
        }
        if let Some(id) = report.action {
            self.run_action(id);
        }
        if let Some(popped) = report.toggled {
            if !self.popped.remove(&popped) {
                self.popped.insert(popped);
            }
            self.redraw = true;
        }
        let graph = &self.project.graph;
        self.popped.retain(|p| graph.kind(p.node).is_some());
        if std::mem::take(&mut self.dirty) {
            // Only views force evaluation, like `main` forcing thunks.
            let targets = graph
                .dag
                .nodes()
                .filter(|(_, node)| node_ui::has_view(node.kind()))
                .map(|(id, _)| id)
                .collect();
            if let Err(error) = self.worker.request(&self.project.graph, self.level, targets) {
                self.report(Err(error));
            }
            self.redraw = true;
        }
    }

    /// Applies one edit outside a GUI frame.
    fn edit(&mut self, edit: Edit<'_>) {
        let results = |id| self.worker.result(id);
        let details = |id| self.worker.detail_view(id);
        let mut frame = Frame::new(&results, &details, &self.popped, self.actions_disabled());
        frame.edit(&mut self.project.graph, edit);
        let report = frame.report;
        let changed = report.changed;
        self.apply(report);
        if changed {
            self.commit();
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
                Notice::Performed { id, result } => {
                    let action = self.action.take();
                    let parameters = result.as_ref().ok().cloned().flatten();
                    self.report(result.map(|_| {
                        action.map_or_else(|| "Action completed".into(), |a| a.completed)
                    }));
                    // Applied however the node changed meanwhile; a refusal is reported.
                    if let Some(parameters) = parameters {
                        self.edit(Edit::Parameters(id, parameters));
                    }
                    action_reported = true;
                }
                // Redrawn like every notice.
                Notice::Detailed => {}
                Notice::Failed => {
                    self.action = None;
                    self.report(Err("the worker stopped".into()));
                    action_reported = true;
                }
            }
        }
        // A preview finishing in the same frame must not hide an export error.
        if !action_reported
            && self.action.is_none()
            && let Some(result) = evaluated
        {
            self.report(result.map(|()| "Preview updated".into()));
        }
    }

    pub fn after_frame(&mut self) {
        self.worker.after_frame();
    }

    fn menu(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for command in &COMMANDS {
                let button = egui::Button::new(command.label);
                let shortcut = ui.ctx().format_shortcut(&command.shortcut);
                if ui.add_enabled(self.allows(command), button).tip(shortcut).clicked() {
                    self.invoke(command);
                }
            }
            ui.add_enabled_ui(self.idle(), |ui| {
                ui.menu_button("New from template", |ui| {
                    for template in templates::TEMPLATES {
                        if ui.button(template.name).clicked() {
                            self.replace(Box::new(|app: &mut App| app.new_from(template)));
                            ui.close();
                        }
                    }
                })
                .response
                .tip("Built-in templates. Open… also opens .drip-template files.");
            });
            ui.separator();
            if ui.button("Add node…").tip("Space over the canvas").clicked() {
                self.editor.add_node();
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
            .tip("Global preview scale. Export always uses full detail.");
            if self.level != previous {
                log::debug!("preview level changed from {previous} to {}", self.level);
                ui_state::set_preview_level(&mut self.project.ui, self.level);
                self.modified = true;
                self.dirty = true;
            }
        });
    }

    fn status_line(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if !self.wide_gamut {
                ui.label(RichText::new("Previews clipped to sRGB").color(theme::ERROR));
            }
            if self.worker.busy() {
                let label = match self.worker.waiting() {
                    Some(name) => ui.label(format!("Computing {name}…")),
                    None => ui.label("Evaluating…"),
                };
                if self.worker.showing_previous() {
                    label.tip("Views retain the previous completed evaluation while the current request runs.");
                }
            }
            if let Some(action) = &self.action {
                ui.label(&action.running);
            }
            if let Some(status) = &self.status
                && (status.error || (!self.worker.busy() && self.action.is_none()))
            {
                let text = RichText::new(&status.text);
                ui.label(if status.error { text.color(theme::ERROR) } else { text });
            }
        });
    }

    fn actions_disabled(&self) -> bool {
        self.action.is_some()
    }

    fn run_action(&mut self, id: NodeId) {
        let node = self.project.graph.node(id).expect("action node exists");
        let action = node_ui::action(node.kind).expect("only nodes with an action offer it");
        let name = action.name;
        match self.worker.action(&self.project.graph, id, action) {
            Ok(()) => {
                let subject = match node.file(true) {
                    Some(path) => format!("{} → {path}", node.name),
                    None => node.name.clone(),
                };
                let running = format!("Running {name}: {subject}…");
                self.action = Some(ActionStatus {
                    running: running.clone(),
                    completed: format!("Completed {name}: {subject}"),
                });
                self.report(Ok(running));
            }
            Err(error) => {
                log::error!("cannot start action node={id:?} action={name}: {error}");
                self.report(Err(error));
            }
        }
    }
}

#[cfg(test)]
mod tests;
