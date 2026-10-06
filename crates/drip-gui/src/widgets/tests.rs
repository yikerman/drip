use super::*;
use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, vec2};
use egui_kittest::Harness;

#[test]
fn pointer_leave_stops_dropdown_scrolling() {
    for same_frame in [false, true] {
        let mut h = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, state: &mut (usize, Rect)| {
                state.1 = dropdown(
                    ui,
                    egui::ComboBox::from_id_salt("test"),
                    &mut state.0,
                    &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
                    |v| v.to_string(),
                )
                .rect;
            },
            (4, Rect::NOTHING),
        );
        h.event(Event::PointerMoved(h.state().1.center()));
        h.step();
        h.input_mut().events.push(Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: vec2(0.0, -3.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        if !same_frame {
            h.step();
            assert_eq!(h.state().0, 5, "scroll changes selection while the pointer is present");
        }
        let before = h.state().0;
        h.input_mut().events.push(Event::PointerGone);
        h.step();
        assert_eq!(h.state().0, before);
    }
}
