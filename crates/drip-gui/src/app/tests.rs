use drip::runtime::KernelContext;
use serde::{Deserialize, Serialize};
fn empty_contract<P>(_: &drip::runtime::GlobalContext, _: &P) -> drip::Result<()> {
    Ok(())
}

use crate::node_ui::scopes::Histogram;

use crate::node_ui::preview::ImageView;
use crate::worker::view;

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use std::path::Path;

use super::*;

#[test]
fn detail_dropdown_scroll_updates_the_project_and_stops_at_ends() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 100.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(100)
        .build_ui_state(
            |ui, app: &mut App| {
                crate::theme::fonts(ui.ctx());
                app.menu(ui)
            },
            App::new(None, true, || {}),
        );
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
    let raw = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-ilce-7rm3.arw");
    let mut p = templates::default_project();
    let read = p.graph.find("Bayer RAW").unwrap();
    p.graph.set_param(read, "path", json!(raw)).unwrap();
    let file = std::env::temp_dir().join(format!("drip-gui-{name}-{}.drip", std::process::id()));
    std::fs::write(&file, p.to_json().unwrap()).unwrap();
    file
}

fn harness<'a>(app: App) -> Harness<'a, App> {
    Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut App| {
            crate::theme::fonts(ui.ctx());
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
    h.state_mut().edit(Edit::Param(id, name, value));
}

#[test]
fn auto_exposure_is_queued_and_applies_its_ev_as_an_edit() {
    let file = fixture_project("auto-exposure");
    let mut h = harness(App::new(Some(file.clone()), true, || {}));
    settle(&mut h);
    let exposure = h.state().project.graph.find("Exposure").unwrap();
    h.state_mut().selected = Some(exposure);
    let mut metered = Vec::new();
    for _ in 0..2 {
        h.run();
        h.get_by_label("Auto exposure").click();
        h.run();
        assert!(h.state().action.as_ref().unwrap().running.contains("Auto exposure"));
        assert!(h.get_by_label("Auto exposure").accesskit_node().is_disabled());
        // Replacing the project would drop the running action's result.
        for project in ["New", "Open…"] {
            assert!(h.get_by_label(project).accesskit_node().is_disabled());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        while h.state().action.is_some() {
            assert!(std::time::Instant::now() < deadline, "action did not finish");
            std::thread::sleep(std::time::Duration::from_millis(2));
            h.step();
        }
        let app = h.state();
        assert!(!app.status.as_ref().unwrap().error, "{}", app.status.as_ref().unwrap().text);
        assert!(app.modified && app.worker.busy(), "applied as an ordinary edit");
        metered.push(app.project.graph.node(exposure).unwrap().params["ev"].as_f64().unwrap());
        settle(&mut h);
    }
    // The meter reads Exposure's input, so the current EV does not matter.
    assert!(metered[0] != 0.0 && metered[0] == metered[1], "{metered:?}");
    std::fs::remove_file(file).unwrap();
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
        app.edit(change);
        assert!(app.take_redraw(), "other windows must see the shared edit");
        assert!(!app.worker.busy(), "presentation edits need no evaluation");
    }
    assert_eq!(app.windows()[0].title, "tone · Parameters · Drip");
    assert!(app.project.graph.inputs().any(|input| input == (id, "contrast")));

    app.edit(crate::editing::Edit::Name(id, ""));
    assert_eq!(app.project.graph.node(id).unwrap().name, "tone");
    assert!(app.status.as_ref().unwrap().error);
    assert!(!app.worker.busy());

    app.edit(Edit::Param(id, "contrast", json!(2.0)));
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
    let image = view::<ImageView>(&app.worker, id).image.clone();
    assert_eq!(app.level, DEFAULT_LEVEL);
    assert_eq!(image.width, 3984, "1/2 preview with reduced Bayer input to RCD");
    h.get_by_label("Preview detail").click();
    settle(&mut h);
    h.get_by_label("1/4").click();
    settle(&mut h);
    assert_eq!(h.state().level, 2);
    let app = h.state();
    let image = &view::<ImageView>(&app.worker, id).image;
    assert_eq!(image.width, 1992);
    let histogram = app.project.graph.find("Histogram").unwrap();
    let histogram = view::<Histogram>(&app.worker, histogram);
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

mod only_views_force_evaluation_fixture {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    #[derive(Clone, Serialize, Deserialize, drip::Parameters)]
    struct OffscreenKernel {
        #[param(default = false)]
        value: bool,
    }
    #[drip::node(id="test-offscreen", name="offscreen", contract=empty_contract)]
    fn offscreen(_: &KernelContext<'_>, _: &OffscreenKernel) -> drip::Result<()> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    #[test]
    fn only_views_force_evaluation() {
        let kind = &offscreen::KIND;

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
        set_param(&mut h, left, "value", json!(true));
        settle(&mut h);
        // Nodes without a view are thunks nothing forces, wherever they are.
        assert_eq!(CALLS.load(Ordering::SeqCst), 0);
        assert!(h.state().worker.result(left).is_none());
        assert!(h.state().worker.result(right).is_none());
        let before = completions.load(Ordering::SeqCst);
        h.state_mut().project.graph.set_ui(left, json!({"pos": [0, 0]})).unwrap();
        settle(&mut h);
        assert_eq!(completions.load(Ordering::SeqCst), before, "moving nodes requests no work");
    }
}

mod evaluating_message_keeps_the_ui_and_previous_preview_available_fixture {
    use super::*;
    use crate::test_nodes::{SLOW_GATE as GATE, slow};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    #[test]
    fn evaluating_message_keeps_the_ui_and_previous_preview_available() {
        let (started, entered) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        *GATE.lock().unwrap() = Some((started, resume));
        let mut project = Project::default();
        let id = project.graph.add_node(&slow::KIND).unwrap();
        let mut app = App::new(None, true, || {});
        app.set_project(project, None).unwrap();
        let mut h = harness(app);
        settle(&mut h);
        let before = &view::<ImageView>(&h.state().worker, id).image;
        let before = before.clone();
        set_param(&mut h, id, "block", json!(true));
        h.step();
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let start = Instant::now();
        h.step();
        assert!(h.query_by_label("Evaluating…").is_some());
        let shown = &view::<ImageView>(&h.state().worker, id).image;
        assert!(Arc::ptr_eq(shown, &before));
        h.get_by_label("Preview detail").click();
        h.run();
        assert!(h.query_by_label("Full").is_some(), "controls respond while the node is blocked");
        assert!(start.elapsed() < Duration::from_secs(2));
        release.send(()).unwrap();
        settle(&mut h);
        assert!(h.query_by_label("Evaluating…").is_none());
        assert!(h.query_by_label("Preview updated").is_some());
    }
}

#[test]
fn invalid_detail_does_not_replace_the_open_project() {
    let mut app = App::new(None, true, || {});
    let original = app.project.clone();
    for value in [json!(-1), json!(9), json!(256), json!(2.5), json!("2"), json!(null)] {
        let mut project = templates::default_project();
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
fn opening_templates_creates_untitled_projects_and_saving_clears_modified_title() {
    let template =
        std::env::temp_dir().join(format!("drip-title-{}.drip-template", std::process::id()));
    let destination = template.with_extension("drip");
    std::fs::write(&template, Project::default().to_json().unwrap()).unwrap();
    let mut h = harness(App::new(Some(template.clone()), true, || {}));
    settle(&mut h);
    assert!(h.state().file.is_none());
    assert_eq!(h.state().title(), "Untitled · Drip");
    h.state_mut().edit(crate::editing::Edit::Add(&drip::node::exposure::compute::KIND, Vec2::ZERO));
    assert_eq!(h.state().title(), "Untitled * · Drip");
    h.state_mut().file = Some(destination.clone());
    h.run();
    h.get_by_label("Save").click();
    h.run();
    assert!(!h.state().modified);
    assert_eq!(
        h.state().title(),
        format!("{} · Drip", destination.file_name().unwrap().to_string_lossy())
    );
    let id = h.state().project.graph.find("Exposure").unwrap();
    h.state_mut().edit(crate::editing::Edit::Name(id, "Test exposure"));
    assert!(h.state().modified, "renaming affects persistence without evaluation");
    h.state_mut().set_project(Project::default(), None).unwrap();
    assert_eq!(h.state().title(), "Untitled · Drip");
    std::fs::remove_file(template).unwrap();
    std::fs::remove_file(destination).unwrap();
}

#[test]
fn shortcuts_run_project_commands_with_exact_modifiers() {
    let file = std::env::temp_dir().join(format!("drip-shortcuts-{}.drip", std::process::id()));
    std::fs::write(&file, Project::default().to_json().unwrap()).unwrap();
    let mut h = harness(App::new(Some(file.clone()), true, || {}));
    h.run();
    h.state_mut().edit(Edit::Add(&drip::node::exposure::compute::KIND, Vec2::ZERO));
    // Extra modifiers select no command, so New keeps the edit.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    h.run();
    assert!(h.state().project.graph.find("Exposure").is_some());
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    assert!(!h.state().modified);
    let saved = Project::from_json(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(saved.graph.find("Exposure").is_some());
    h.state_mut().action = Some(ActionStatus { running: String::new(), completed: String::new() });
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    assert_eq!(h.state().file.as_ref(), Some(&file), "a running action keeps its project");
    h.state_mut().action = None;
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    assert!(h.state().file.is_none());
    std::fs::remove_file(file).unwrap();
}

#[test]
fn replacing_a_modified_project_asks_to_save_first() {
    let file = std::env::temp_dir().join(format!("drip-unsaved-{}.drip", std::process::id()));
    std::fs::write(&file, Project::default().to_json().unwrap()).unwrap();
    let mut h = harness(App::new(Some(file.clone()), true, || {}));
    h.run();
    let add = |h: &mut Harness<'_, App>| {
        let kind = &drip::node::exposure::compute::KIND;
        h.state_mut().edit(Edit::Add(kind, Vec2::ZERO));
    };
    add(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.query_by_label("Discard").is_none());
    assert_eq!(h.state().file.as_ref(), Some(&file), "canceling keeps the project");
    h.get_by_label("New").click();
    h.run();
    // The prompt's Save, above the menu's.
    h.get_all_by_label("Save").last().unwrap().click();
    h.run();
    assert!(h.state().file.is_none(), "saving replaces the project");
    let saved = Project::from_json(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(saved.graph.find("Exposure").is_some());
    add(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    h.get_by_label("Discard").click();
    h.run();
    assert!(!h.state().modified, "discarding replaces the project");
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    h.run();
    assert!(h.query_by_label("Discard").is_none(), "unmodified projects are replaced");
    std::fs::remove_file(file).unwrap();
}

#[test]
fn built_in_templates_replace_the_project_after_the_unsaved_prompt() {
    let file = std::env::temp_dir().join(format!("drip-built-in-{}.drip", std::process::id()));
    std::fs::write(&file, Project::default().to_json().unwrap()).unwrap();
    let mut h = harness(App::new(Some(file.clone()), true, || {}));
    h.run();
    h.state_mut().edit(Edit::Add(&drip::node::exposure::compute::KIND, Vec2::ZERO));
    h.get_by_label("New from template").click();
    h.run();
    h.get_by_label(templates::TEMPLATES[0].name).click();
    h.run();
    assert_eq!(h.state().file.as_ref(), Some(&file), "the prompt holds the template");
    h.get_by_label("Discard").click();
    h.run();
    assert!(h.state().file.is_none() && !h.state().modified);
    assert!(h.state().project.graph.find("Bayer RAW").is_some());
    std::fs::remove_file(file).unwrap();
}

fn undo(h: &mut Harness<'_, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

fn redo(h: &mut Harness<'_, App>) {
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    h.run();
}

#[test]
fn a_slider_drag_is_one_undo_step() {
    let mut h = harness(App::new(None, true, || {}));
    let id = h.state().project.graph.find("Exposure").unwrap();
    h.state_mut().selected = Some(id);
    h.run();
    let ev = |h: &Harness<'_, App>| h.state().project.graph.node(id).unwrap().params["ev"].clone();
    let before = ev(&h);
    let slider = h.get_by_role(egui::accesskit::Role::Slider).rect();
    let start = slider.center();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(start));
    h.event(button(start, true));
    h.step();
    let end = egui::pos2(slider.right() - 2.0, start.y);
    for step in 1..=3 {
        h.event(egui::Event::PointerMoved(start.lerp(end, step as f32 / 3.0)));
        h.step();
    }
    assert_ne!(ev(&h), before, "the drag edits while held");
    assert!(!h.state().history.can_undo(), "a held drag is not yet a step");
    h.event(button(end, false));
    h.run();
    let dragged = ev(&h);
    assert_ne!(dragged, before);
    undo(&mut h);
    assert_eq!(ev(&h), before);
    assert!(!h.state().history.can_undo(), "the drag was one step");
    redo(&mut h);
    assert_eq!(ev(&h), dragged);
}

#[test]
fn undo_restores_removed_nodes_and_shares_the_others() {
    let mut h = harness(App::new(None, true, || {}));
    h.run();
    let graph = |h: &Harness<'_, App>| h.state().project.graph.clone();
    let raw = graph(&h).find("Bayer RAW").unwrap();
    let shared = |h: &Harness<'_, App>| {
        std::ptr::from_ref(h.state().project.graph.dag.node(raw).unwrap()).cast::<()>()
    };
    let raw_node = shared(&h);
    let id = graph(&h).find("Exposure").unwrap();
    let edges: Vec<_> = graph(&h).edges().collect();
    let ui = graph(&h).ui(id).unwrap().clone();
    h.state_mut().edit(Edit::Remove(id));
    assert!(graph(&h).kind(id).is_none());
    undo(&mut h);
    assert_eq!(graph(&h).edges().collect::<Vec<_>>(), edges);
    assert_eq!(graph(&h).node(id).unwrap().name, "Exposure");
    assert_eq!(graph(&h).ui(id), Some(&ui));
    assert_eq!(shared(&h), raw_node, "unchanged nodes are shared, not copied");
    redo(&mut h);
    assert!(graph(&h).kind(id).is_none());
    assert_eq!(shared(&h), raw_node);
}

#[test]
fn focused_text_fields_keep_the_undo_shortcut() {
    let mut h = harness(App::new(None, true, || {}));
    let id = h.state().project.graph.find("Exposure").unwrap();
    h.state_mut().selected = Some(id);
    h.run();
    h.state_mut().edit(Edit::Param(id, "ev", json!(1.0)));
    node_ui::parameters::focus_name(&h.ctx, id);
    h.run();
    assert!(h.ctx.text_edit_focused());
    undo(&mut h);
    assert_eq!(h.state().project.graph.node(id).unwrap().params["ev"], json!(1.0));
}

#[test]
fn warns_when_previews_are_clipped() {
    let mut h = harness(App::new(None, false, || {}));
    h.run();
    assert!(h.query_by_label("Previews clipped to sRGB").is_some());
    let mut h = harness(App::new(None, true, || {}));
    h.run();
    assert!(h.query_by_label("Previews clipped to sRGB").is_none());
}

#[test]
fn inspector_shows_the_selected_node_and_the_inputs() {
    let mut app = App::new(None, true, || {});
    app.selected = app.project.graph.find("Export TIFF");
    let mut h = harness(app);
    h.run();
    for label in [
        "Output profile",
        "Rendering intent",
        "Depth",
        "Export",
        "Output file (template)",
        "Bayer RAW · RAW file",
        "Export TIFF · Output file",
    ] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
}

#[test]
fn node_help_follows_the_type_id_and_stays_out_of_parameter_windows() {
    let documentation = drip::node::sigmoid::apply::KIND.help.trim().replace('\n', " ");
    let mut app = App::new(None, true, || {});
    let id = app.project.graph.find("Sigmoid").unwrap();
    app.selected = Some(id);
    let mut h = harness(app);
    h.run();
    let kind = h.get_by_label("sigmoid").rect();
    let description = h.get_by_label(&documentation).rect();
    let output = h.get_by_label("Output").rect();
    let control = h.get_by_label("Contrast").rect();
    assert!(kind.bottom() <= description.top());
    assert!(description.bottom() <= output.top());
    assert!(output.bottom() <= control.top());

    let popped = Popped { node: id, part: node_ui::Part::Parameters };
    let mut popup = Harness::builder().with_size(egui::vec2(360.0, 320.0)).build_ui_state(
        move |ui, app: &mut App| {
            crate::theme::fonts(ui.ctx());
            app.window(ui, popped)
        },
        {
            let mut app = App::new(None, true, || {});
            app.project = h.state().project.clone();
            app
        },
    );
    popup.run();
    assert!(popup.query_by_label("sigmoid").is_some());
    assert!(popup.query_by_label("Contrast").is_some());
    assert!(popup.query_by_label("Rec.2020 RGB").is_none());
    assert!(popup.query_by_label("darktable: sigmoid").is_none());
    assert!(popup.query_by_label(&documentation).is_none());
}

#[test]
fn nodes_without_controls_show_documentation_and_references() {
    let mut app = App::new(None, true, || {});
    app.selected = app.project.graph.find("RCD demosaic");
    let mut h = harness(app);
    h.run();
    let kind = h.get_by_label("rcd-demosaic").rect();
    let input = h.get_by_label("Input · image").rect();
    let output = h.get_by_label("Output").rect();
    let reference = h.get_by_label("RCD source").rect();
    assert!(kind.bottom() <= input.top());
    assert!(input.bottom() <= output.top());
    assert!(output.bottom() <= reference.top());
}

mod undocumented_nodes_show_generated_port_help_fixture {
    use super::*;

    use drip::node::data::{Rgb, RgbDesc};
    use drip::ports::Read;
    use drip::ports::{Cpu, Write};
    fn contract(
        _: &drip::runtime::GlobalContext,
        _: &(),
        image: &RgbDesc,
    ) -> drip::Result<(RgbDesc,)> {
        Ok((image.clone(),))
    }
    #[drip::node(id="test-generic", name="generic", contract=contract)]
    fn generic(
        _: &KernelContext<'_>,
        _: &(),
        image: Read<'_, Cpu<Rgb>>,
        output: Write<'_, Cpu<Rgb>>,
    ) -> drip::Result<()> {
        output.data.copy_from_slice(image.data);
        Ok(())
    }

    #[test]
    fn undocumented_nodes_show_generated_port_help() {
        let mut project = Project::default();
        let id = project.graph.add_node(&generic::KIND).unwrap();
        let mut app = App::new(None, true, || {});
        app.set_project(project, None).unwrap();
        app.selected = Some(id);
        let mut h = harness(app);
        h.run();
        let kind = h.get_by_label("test-generic").rect();
        let input = h.get_by_label("Input · image").rect();
        let output = h.get_by_label("Output").rect();
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
        graph.nodes().filter(|(_, n)| !n.kind.parameters.is_empty()).map(|(id, _)| id).collect();
    let viewers: Vec<_> =
        graph.nodes().filter(|(_, n)| crate::node_ui::has_view(n.kind)).map(|(id, _)| id).collect();
    assert_eq!(h.get_all_by_label("≡").count(), with_params.len());
    assert_eq!(h.get_all_by_label("❒").count(), viewers.len());
    // AccessKit bounds ignore the canvas transform, so clicks go by action.
    let index = |ids: &[NodeId]| ids.iter().position(|&id| id == histogram).unwrap();
    h.get_all_by_label("≡").nth(index(&with_params)).unwrap().click_accesskit();
    h.run();
    h.get_all_by_label("❒").nth(index(&viewers)).unwrap().click_accesskit();
    h.run();
    let parameters = Popped { node: histogram, part: Part::Parameters };
    let view = Popped { node: histogram, part: Part::View };
    let titles: Vec<_> = h.state().windows().into_iter().map(|w| (w.popped, w.title)).collect();
    assert_eq!(
        titles,
        [
            (parameters, "Histogram · Parameters · Drip".into()),
            (view, "Histogram · View · Drip".into())
        ]
    );
    assert!(h.query_by_label("Shown in its window").is_some());
    h.state_mut().close_window(view);
    h.run();
    assert!(h.query_by_label("Shown in its window").is_none());

    // Node ids are reused, so a new project closes every window.
    h.state_mut().set_project(templates::default_project(), None).unwrap();
    assert!(h.state().windows().is_empty());
}

#[test]
fn scope_nodes_offer_popouts() {
    for kind in [
        &crate::node_ui::scopes::waveform::observe::KIND,
        &crate::node_ui::scopes::vectorscope::observe::KIND,
    ] {
        let mut project = Project::default();
        let id = project.graph.add_node(kind).unwrap();
        let mut app = App::new(None, true, || {});
        app.set_project(project, None).unwrap();
        let mut h = harness(app);
        h.run();
        h.get_by_label("❒").click_accesskit();
        h.run();
        let windows = h.state().windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].popped.node, id);
        assert_eq!(windows[0].title, format!("{} · View · Drip", kind.name));
    }
}

mod declared_gui_view_gets_a_popout_before_preparation_succeeds_fixture {
    use super::*;
    use crate::test_nodes::declared_view as view;

    #[test]
    fn declared_gui_view_gets_a_popout_before_preparation_succeeds() {
        let mut project = Project::default();
        let id = project.graph.add_node(&view::KIND).unwrap();
        let mut app = App::new(None, true, || {});
        app.set_project(project, None).unwrap();
        let mut h = harness(app);
        h.run();
        h.get_by_label("❒").click_accesskit();
        h.run();
        let windows = h.state().windows();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].popped.node, id);
        assert_eq!(windows[0].title, "Declared view · View · Drip");
    }
}

mod custom_controls_keep_the_generic_view_and_popout_fixture {
    use super::*;
    use crate::test_nodes::custom_controls_view as view;

    #[test]
    fn custom_controls_keep_the_generic_view_and_popout() {
        let mut project = Project::default();
        let id = project.graph.add_node(&view::KIND).unwrap();
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
        h.get_by_label("❒").click_accesskit();
        h.run();
        let popped = Popped { node: id, part: crate::node_ui::Part::View };
        assert_eq!(h.state().windows()[0].popped, popped);
        assert!(h.query_by_label("Shown in its window").is_some());
        assert!(h.query_by_label("Custom view controls").is_some());
        h.state_mut().close_window(popped);
        h.run();
        assert!(h.query_by_label("Shown in its window").is_none());
        assert!(h.query_by_label("Custom view controls").is_some());
    }
}

#[test]
fn parameter_windows_show_the_parameters_and_close_with_their_node() {
    for (name, labels) in [
        ("Export TIFF", &["Output profile", "Rendering intent", "Depth", "Export"][..]),
        (
            "Sigmoid",
            &["Contrast", "Skew", "Preserve hue", "−8 … +8 EV relative to middle grey"][..],
        ),
    ] {
        let mut app = App::new(None, true, || {});
        let id = app.project.graph.find(name).unwrap();
        let popped = Popped { node: id, part: crate::node_ui::Part::Parameters };
        app.popped.insert(popped);
        // The window opens at the size its content takes, smaller than a
        // guess yet with every parameter inside.
        let size = app.window_size(popped, 1.25);
        assert!(size.x < 480.0 && size.y < 320.0, "{size:?}");
        let mut h = Harness::builder().with_size(size).build_ui_state(
            move |ui, app: &mut App| {
                crate::theme::fonts(ui.ctx());
                app.window(ui, popped)
            },
            app,
        );
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
    app.selected = app.project.graph.find("Export TIFF");
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
