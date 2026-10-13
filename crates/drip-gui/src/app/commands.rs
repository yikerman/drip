//! Project commands: the table the menu and shortcuts share, the unsaved-edits
//! prompt, opening and saving files, and undo/redo.
use std::path::{Path, PathBuf};

use super::App;
use crate::editing::Report;
use crate::editor::Editor;
use crate::history::History;
use crate::model::{Graph, Project};
use crate::node_ui::Popped;
use crate::templates::{self, Template};
use crate::ui_state;
use crate::worker::Worker;
use egui::{Key, KeyboardShortcut, Modifiers};

/// Work that replaces the project, deferred while unsaved edits are resolved.
pub(super) type Replace = Box<dyn FnOnce(&mut App)>;

/// A project command, offered in the menu and by its shortcut.
pub(super) struct Command {
    pub(super) label: &'static str,
    pub(super) shortcut: KeyboardShortcut,
    /// Whether it replaces the open project.
    replaces: bool,
    available: fn(&App) -> bool,
    run: fn(&mut App),
}

pub(super) const COMMANDS: [Command; 7] = {
    const SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
    const ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);
    [
        Command {
            label: "New",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::N),
            replaces: true,
            available: App::idle,
            run: App::new_project,
        },
        Command {
            label: "Open…",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
            replaces: true,
            available: App::idle,
            run: App::open_dialog,
        },
        Command {
            label: "Save",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::S),
            replaces: false,
            available: |_| true,
            run: App::save_project,
        },
        Command {
            label: "Save as…",
            shortcut: KeyboardShortcut::new(SHIFT, Key::S),
            replaces: false,
            available: |_| true,
            run: |app| app.save_as(false),
        },
        Command {
            label: "Save template…",
            shortcut: KeyboardShortcut::new(ALT, Key::S),
            replaces: false,
            available: |_| true,
            run: |app| app.save_as(true),
        },
        Command {
            label: "Undo",
            shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
            replaces: false,
            available: |app| app.editing.is_some() || app.history.can_undo(),
            run: App::undo,
        },
        Command {
            label: "Redo",
            shortcut: KeyboardShortcut::new(SHIFT, Key::Z),
            replaces: false,
            available: |app| app.history.can_redo(),
            run: App::redo,
        },
    ]
};

/// Takes a press of exactly `shortcut`. egui's own matching ignores extra
/// Shift and Alt, so Save as would also be taken as Save.
fn pressed(ctx: &egui::Context, shortcut: KeyboardShortcut) -> bool {
    ctx.input_mut(|input| {
        let before = input.events.len();
        input.events.retain(|event| {
            !matches!(event, egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. }
                if *key == shortcut.logical_key && modifiers.matches_exact(shortcut.modifiers))
        });
        input.events.len() < before
    })
}

impl App {
    /// Runs the first project command whose shortcut was pressed in `ctx`.
    /// Text fields keep their keys, including their own undo.
    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() {
            return;
        }
        if let Some(command) = COMMANDS.iter().find(|c| self.allows(c) && pressed(ctx, c.shortcut))
        {
            self.invoke(command);
        }
    }

    /// Runs `command`, after asking about unsaved edits if it replaces the project.
    pub(super) fn invoke(&mut self, command: &Command) {
        if command.replaces {
            self.replace(Box::new(command.run));
        } else {
            (command.run)(self);
        }
    }

    /// Runs `replace`, which replaces the project, after asking about unsaved edits.
    pub(super) fn replace(&mut self, replace: Replace) {
        if self.modified {
            self.replacing = Some(replace);
            self.redraw = true;
        } else {
            replace(self);
        }
    }

    /// Asks whether to save unsaved edits before a waiting command replaces
    /// the project. Every window shows it; the first answer applies.
    pub(super) fn unsaved_prompt(&mut self, ctx: &egui::Context) {
        if self.replacing.is_none() {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("unsaved edits")).show(ctx, |ui| {
            ui.label(format!("{} has unsaved changes.", self.name()));
            ui.horizontal(|ui| {
                let save = ui.button("Save").clicked();
                let discard = ui.button("Discard").clicked();
                (save, discard, ui.button("Cancel").clicked())
            })
            .inner
        });
        let (save, discard, cancel) = response.inner;
        if save {
            self.save_project();
        }
        if save || discard || cancel || response.should_close() {
            let replace = self.replacing.take().expect("checked above");
            self.redraw = true;
            // A failed or canceled save keeps the project.
            if discard || (save && !self.modified) {
                replace(self);
            }
        }
    }

    pub(super) fn allows(&self, command: &Command) -> bool {
        (command.available)(self)
    }

    /// A running action cannot be interrupted, and its result would be lost.
    pub(super) fn idle(&self) -> bool {
        self.action.is_none()
    }

    /// Notes edits a frame of `window` made, and makes them an undo step once
    /// no pointer is held there, so that a drag is one step.
    pub(super) fn record(&mut self, ctx: &egui::Context, window: Option<Popped>, changed: bool) {
        if changed {
            self.editing.get_or_insert(window);
        }
        if self.editing == Some(window) && !ctx.input(|input| input.pointer.any_down()) {
            self.commit();
        }
    }

    pub(super) fn commit(&mut self) {
        self.editing = None;
        self.history.commit(&self.project.graph);
    }

    /// Undoes the latest step, including edits of a gesture still in progress.
    pub(super) fn undo(&mut self) {
        if self.editing.is_some() {
            self.commit();
        }
        if let Some(graph) = self.history.undo() {
            self.restore(graph);
        }
    }

    pub(super) fn redo(&mut self) {
        if let Some(graph) = self.history.redo() {
            self.restore(graph);
        }
    }

    /// Shows a graph from the history as an edit: it is evaluated and redrawn,
    /// and windows of nodes it lacks close.
    pub(super) fn restore(&mut self, graph: Graph) {
        self.selected = self.selected.filter(|id| graph.kind(*id).is_some());
        self.project.graph = graph;
        self.apply(Report { changed: true, edited: true, ..Report::default() });
    }

    pub(super) fn new_project(&mut self) {
        self.new_from(&templates::TEMPLATES[0]);
    }

    /// Replaces the project with an untitled one from a built-in template.
    pub(super) fn new_from(&mut self, template: &Template) {
        let result =
            template.project().map_err(|e| e.to_string()).and_then(|p| self.set_project(p, None));
        if let Err(error) = result {
            self.report(Err(error));
        }
    }

    pub(super) fn open_dialog(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Open Drip project or template")
            .add_filter("Drip project or template", &["drip", "drip-template"])
            .pick_file()
        {
            self.open(file);
        }
    }

    /// Saves to the project's file, or asks for one.
    pub(super) fn save_project(&mut self) {
        match self.file.clone() {
            Some(file) => {
                if self.save(&file, self.project.clone()) {
                    self.modified = false;
                }
            }
            None => self.save_as(false),
        }
    }

    pub(super) fn set_project(
        &mut self,
        project: Project,
        file: Option<PathBuf>,
    ) -> Result<(), String> {
        let level = ui_state::preview_level(&project.ui)?;
        self.level = level;
        self.history = History::new(project.graph.clone());
        self.editing = None;
        self.project = project;
        self.ui_generation += 1;
        self.file = file;
        self.modified = false;
        self.selected = None;
        self.editor = Editor::default();
        // Node ids are reused across projects.
        self.popped.clear();
        self.dirty = true;
        // Queued work and late replies belong to the old project.
        self.worker = Worker::new(self.wake.clone(), self.disk.clone());
        Ok(())
    }

    /// Opens a project, or a template as an untitled project.
    pub(super) fn open(&mut self, file: PathBuf) {
        let template =
            file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("drip-template"));
        let result = std::fs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|text| Project::from_json(&text).map_err(|e| e.to_string()))
            .and_then(|project| self.set_project(project, (!template).then(|| file.clone())));
        self.report(result.map(|()| format!("Opened {}", file.display())));
    }

    pub(super) fn save_as(&mut self, template: bool) {
        let name = if template { "Drip template" } else { "Drip project" };
        let extension = if template { "drip-template" } else { "drip" };
        let Some(file) = rfd::FileDialog::new()
            .set_title(format!("Save {name}"))
            .add_filter(name, &[extension])
            .save_file()
        else {
            return;
        };
        let mut file = file.into_os_string();
        if !Path::new(&file).extension().is_some_and(|ext| ext.eq_ignore_ascii_case(extension)) {
            file.push(format!(".{extension}"));
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
            self.modified = false;
        }
    }

    /// Writes `project` to `file`; returns whether that succeeded.
    pub(super) fn save(&mut self, file: &Path, project: Project) -> bool {
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
        self.report(result.map(|()| format!("Saved {shown}")).map_err(|e| format!("{shown}: {e}")));
        saved
    }
}
