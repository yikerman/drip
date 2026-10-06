use super::*;
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, Rect, TouchPhase, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

struct State {
    kind: ParamKind,
    value: Json,
    rect: Rect,
    offset: f32,
}

fn harness(kind: ParamKind, value: Json) -> Harness<'static, State> {
    Harness::builder()
        .with_size(vec2(400.0, 200.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(100)
        .build_ui_state(
            |ui, state: &mut State| {
                let scroll = egui::ScrollArea::vertical().show(ui, |ui| {
                    state.rect = ui
                        .horizontal(|ui| {
                            if let Some(value) =
                                edit_value(ui, egui::Id::new("param"), &state.kind, &state.value)
                            {
                                state.value = value;
                            }
                        })
                        .response
                        .rect;
                    ui.allocate_space(vec2(200.0, 600.0));
                });
                state.offset = scroll.state.offset.y;
            },
            State { kind, value, rect: Rect::NOTHING, offset: 0.0 },
        )
}

fn click(h: &mut Harness<'_, State>, pos: Pos2, button: PointerButton) {
    h.event(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
        h.step();
    }
}

fn scroll(h: &mut Harness<'_, State>, pos: Pos2, unit: MouseWheelUnit, y: f32) {
    h.event(Event::PointerMoved(pos));
    h.event(Event::MouseWheel {
        unit,
        delta: vec2(0.0, y),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

#[test]
fn wheel_adjusts_sliders_and_consumes_panel_scroll() {
    for (kind, value, increment) in [
        (ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 }, json!(2.0), 0.2),
        (ParamKind::Int { min: -24, max: -1, default: -12 }, json!(-10), 1.0),
    ] {
        let mut h = harness(kind, value.clone());
        let pos = h.state().rect.left_center() + vec2(30.0, 0.0);
        scroll(&mut h, pos, MouseWheelUnit::Line, 1.0);
        assert!(
            (h.state().value.as_f64().unwrap() - value.as_f64().unwrap() - increment).abs() < 1e-6
        );
        scroll(&mut h, pos, MouseWheelUnit::Line, -2.0);
        assert!(
            (h.state().value.as_f64().unwrap() - value.as_f64().unwrap() + increment).abs() < 1e-6
        );
        assert_eq!(h.state().offset, 0.0);
        scroll(&mut h, pos, MouseWheelUnit::Line, 1000.0);
        let max = match kind {
            ParamKind::Float { max, .. } => max,
            ParamKind::Int { max, .. } => max as f64,
            _ => unreachable!(),
        };
        assert_eq!(h.state().value.as_f64().unwrap(), max);
        let value = h.state().value.clone();
        scroll(&mut h, Pos2::new(150.0, 100.0), MouseWheelUnit::Line, -3.0);
        assert_eq!(h.state().value, value);
        assert!(h.state().offset > 0.0);
    }
}

#[test]
fn right_button_preserves_the_displayed_slider_value_on_every_frame() {
    for (kind, value) in [
        (ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 }, json!(2.0)),
        (ParamKind::Int { min: -24, max: -1, default: -12 }, json!(-10)),
    ] {
        let mut h = harness(kind, value.clone());
        let pos = h.state().rect.left_center() + vec2(7.0, 0.0);
        h.event(Event::PointerMoved(pos));
        for event in [
            Event::PointerButton {
                pos,
                button: PointerButton::Secondary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerMoved(pos + vec2(40.0, 0.0)),
            Event::PointerButton {
                pos: pos + vec2(40.0, 0.0),
                button: PointerButton::Secondary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ] {
            h.event(event);
            h.step();
            assert_eq!(h.state().value, value);
            for role in [egui::accesskit::Role::Slider, egui::accesskit::Role::SpinButton] {
                assert_eq!(h.get_by_role(role).accesskit_node().numeric_value(), value.as_f64());
            }
        }
        click(&mut h, pos, PointerButton::Primary);
        h.run();
        assert_ne!(h.state().value, value);
    }
}

#[test]
fn context_menu_resets_without_editing_on_right_click() {
    for (kind, value) in [
        (ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 }, json!(2.0)),
        (ParamKind::Int { min: -24, max: -1, default: -12 }, json!(-10)),
        (ParamKind::Bool { default: true }, json!(false)),
        (ParamKind::Choice { options: &["first", "second"], default: "first" }, json!("second")),
        (ParamKind::Path { output: false }, json!("/tmp/example.raw")),
    ] {
        let mut h = harness(kind, value.clone());
        let pos = if matches!(kind, ParamKind::Path { .. }) {
            h.get_by_label("example.raw").rect().center()
        } else {
            h.state().rect.left_center() + vec2(7.0, 0.0)
        };
        click(&mut h, pos, PointerButton::Secondary);
        h.run();
        assert_eq!(h.state().value, value, "{kind:?}");
        h.get_by_label("Reset to default").click();
        h.run();
        assert_eq!(h.state().value, kind.default_value(), "{kind:?}");
        assert!(!egui::Popup::is_any_open(&h.ctx));
    }
}

#[test]
fn numeric_readout_menu_resets_without_restoring_its_edit_buffer() {
    let kind = ParamKind::Float { min: -10.0, max: 10.0, default: 0.0 };
    let mut h = harness(kind, json!(2.0));
    let pos = h.get_by_role(egui::accesskit::Role::SpinButton).rect().center();
    click(&mut h, pos, PointerButton::Primary);
    click(&mut h, pos, PointerButton::Secondary);
    h.run();
    h.get_by_label("Reset to default").click();
    h.run();
    assert_eq!(h.state().value, kind.default_value());
    click(&mut h, Pos2::new(150.0, 100.0), PointerButton::Primary);
    h.run();
    assert_eq!(h.state().value, kind.default_value());
}

#[test]
fn small_trackpad_deltas_accumulate_into_integer_steps() {
    let mut h = harness(ParamKind::Int { min: -24, max: -1, default: -12 }, json!(-10));
    let pos = h.state().rect.left_center() + vec2(30.0, 0.0);
    let line = h.ctx.options(|options| options.input_options.line_scroll_speed);
    for _ in 0..10 {
        scroll(&mut h, pos, MouseWheelUnit::Point, line / 10.0);
    }
    assert_eq!(h.state().value, json!(-9));
    assert_eq!(h.state().offset, 0.0);
}
