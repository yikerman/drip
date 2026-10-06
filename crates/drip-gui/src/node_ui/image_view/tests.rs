use super::*;
use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase};
use egui_kittest::{Harness, kittest::Queryable};

#[test]
fn pixel_zoom_tracks_desktop_scale_and_fit_tracks_window_size() {
    let pixels = vec2(1000.0, 800.0);
    for ppp in [1.0, 1.25, 2.0] {
        for zoom in [1.0, 2.0, 4.0] {
            let state = ViewState { zoom: Some(zoom), ..Default::default() };
            for viewport in [vec2(500.0, 300.0), vec2(900.0, 700.0)] {
                let area = Rect::from_min_size(pos2(8.0, 40.0), viewport);
                let size = pixels * state.scale(area, pixels, ppp);
                assert!((size * ppp - pixels * zoom).length() < 0.001);
                let fit = ViewState::default();
                let fitted = fit.rect(area, pixels * fit.scale(area, pixels, ppp));
                assert_eq!(fitted.center(), area.center());
                assert!(area.contains_rect(fitted));
                assert!(
                    (fitted.width() - area.width()).abs() < 0.001
                        || (fitted.height() - area.height()).abs() < 0.001
                );
            }
        }
    }
}

#[test]
fn zoom_keeps_pointer_anchor_and_pan_stops_at_image_edges() {
    let area = Rect::from_min_size(pos2(10.0, 40.0), vec2(500.0, 300.0));
    let pixels = vec2(2000.0, 1600.0);
    let pointer = pos2(170.0, 210.0);
    let mut state = ViewState { zoom: Some(1.0), ..Default::default() };
    let before = state.rect(area, pixels / 2.0);
    let anchor = (pointer - before.min) / before.size();
    state.zoom_at(area, pixels, 2.0, pointer, 2.0);
    let size = pixels * state.scale(area, pixels, 2.0);
    let after = state.rect(area, size);
    assert!(((pointer - after.min) / after.size() - anchor).length() < 1e-6);
    state.pan(area, size, vec2(30.0, -20.0));
    assert!((state.rect(area, size).min - after.min - vec2(30.0, -20.0)).length() < 0.001);
    for delta in [Vec2::splat(10000.0), Vec2::splat(-10000.0)] {
        state.pan(area, size, delta);
        assert!(state.rect(area, size).contains_rect(area));
    }
    state.pan(area, vec2(100.0, 100.0), Vec2::splat(10000.0));
    assert_eq!(state.center, pos2(0.5, 0.5));
}

#[test]
fn popup_controls_wheel_and_primary_drag_share_navigation_state() {
    let image = Arc::new(Image { width: 1000, height: 800, scale: 2, texels: vec![] });
    let mut h = Harness::builder()
        .with_size(vec2(500.0, 400.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(100)
        .build_ui_state(
            move |ui, state: &mut ViewState| {
                show(ui, &image);
                *state = ui.data(|data| data.get_temp(ui.id().with("image view")).unwrap());
            },
            ViewState::default(),
        );
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run();
    h.get_by_label("100%").click();
    h.run();
    assert_eq!(h.state().zoom, Some(1.0));
    h.get_by_label("1000 × 800 · 1/2 detail");
    let pointer = pos2(220.0, 180.0);
    h.event(Event::PointerMoved(pointer));
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 100.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert!(h.state().zoom.unwrap() > 1.0);
    for button in [PointerButton::Secondary, PointerButton::Primary] {
        let before = h.state().center;
        h.event(Event::PointerMoved(pointer));
        h.step();
        for event in [
            Event::PointerButton {
                pos: pointer,
                button,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerMoved(pointer + vec2(40.0, 20.0)),
            Event::PointerButton {
                pos: pointer + vec2(40.0, 20.0),
                button,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ] {
            h.event(event);
            h.step();
        }
        assert_eq!(h.state().center == before, button == PointerButton::Secondary);
    }
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run();
    h.get_by_label("Fit").click();
    h.run();
    assert_eq!(h.state().zoom, None);
    assert_eq!(h.state().center, pos2(0.5, 0.5));
}
