//! Pop-out image navigation, independent of graph edits and processing.

use std::sync::Arc;

use egui::{PointerButton, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};

use crate::render::image::{self, Image};

#[derive(Clone, Copy)]
struct ViewState {
    /// Render-target pixels per rendered image pixel; None follows the window.
    zoom: Option<f32>,
    /// Normalized image position at the viewport center, retained across updates.
    center: Pos2,
}

impl Default for ViewState {
    fn default() -> Self {
        Self { zoom: None, center: pos2(0.5, 0.5) }
    }
}

impl ViewState {
    fn scale(self, area: Rect, pixels: Vec2, pixels_per_point: f32) -> f32 {
        self.zoom.map_or_else(
            || (area.width() / pixels.x).min(area.height() / pixels.y),
            |zoom| zoom / pixels_per_point,
        )
    }

    fn constrain(&mut self, area: Rect, size: Vec2) {
        let half = (area.size() / size * 0.5).min(Vec2::splat(0.5));
        self.center = self.center.clamp(half.to_pos2(), (Vec2::splat(1.0) - half).to_pos2());
    }

    fn rect(self, area: Rect, size: Vec2) -> Rect {
        Rect::from_min_size(area.center() - self.center.to_vec2() * size, size)
    }

    fn zoom_at(&mut self, area: Rect, pixels: Vec2, ppp: f32, pointer: Pos2, factor: f32) {
        let old_scale = self.scale(area, pixels, ppp);
        let anchor = self.center + (pointer - area.center()) / (pixels * old_scale);
        self.zoom = Some((old_scale * ppp * factor).clamp(0.01, 16.0));
        let size = pixels * self.scale(area, pixels, ppp);
        self.center = anchor - (pointer - area.center()) / size;
        self.constrain(area, size);
    }

    fn pan(&mut self, area: Rect, size: Vec2, delta: Vec2) {
        self.center -= delta / size;
        self.constrain(area, size);
    }
}

pub(super) fn show(ui: &mut Ui, image: &Arc<Image>, interpolation: bool) {
    let id = ui.id().with("image view");
    let mut state = ui.data_mut(|data| data.get_temp::<ViewState>(id).unwrap_or_default());
    ui.horizontal_wrapped(|ui| {
        let mut options = vec![None, Some(0.25), Some(0.5), Some(1.0), Some(2.0), Some(4.0)];
        // Wheel zoom can land between presets; list it in order so dropdown
        // scrolling moves to the adjacent preset in either direction.
        if !options.contains(&state.zoom) {
            options.push(state.zoom);
            options.sort_by(|a, b| a.partial_cmp(b).expect("finite zoom"));
        }
        crate::widgets::dropdown(
            ui,
            egui::ComboBox::from_id_salt(id.with("zoom")).width(75.0),
            &mut state.zoom,
            &options,
            |zoom| zoom.map_or_else(|| "Fit".into(), |z| format!("{:.0}%", z * 100.0)),
        )
        .on_hover_text("100%: one rendered image pixel per display pixel");
        if state.zoom.is_none() {
            state.center = pos2(0.5, 0.5);
        }
        let detail = if image.requested_scale == 1 {
            "Full detail requested".into()
        } else {
            format!("1/{} detail requested", image.requested_scale)
        };
        ui.weak(format!("{} × {} · {detail}", image.width, image.height));
    });

    let (area, response) = ui.allocate_exact_size(ui.available_size(), Sense::drag());
    if !area.is_positive() {
        ui.data_mut(|data| data.insert_temp(id, state));
        return;
    }
    let pixels = vec2(image.width as f32, image.height as f32);
    let ppp = ui.ctx().pixels_per_point();
    state.constrain(area, pixels * state.scale(area, pixels, ppp));
    if let Some(pointer) = response.hover_pos() {
        let scroll = ui.input_mut(|input| {
            let scroll = input.smooth_scroll_delta.y;
            input.smooth_scroll_delta.y = 0.0;
            scroll
        });
        if scroll != 0.0 {
            state.zoom_at(area, pixels, ppp, pointer, (scroll * 0.002).exp());
            ui.ctx().request_repaint();
        }
    }
    let size = pixels * state.scale(area, pixels, ppp);
    if response.dragged_by(PointerButton::Primary) {
        state.pan(area, size, response.drag_delta());
    }
    let cursor = if response.dragged_by(PointerButton::Primary) {
        egui::CursorIcon::Grabbing
    } else {
        egui::CursorIcon::Grab
    };
    response.on_hover_and_drag_cursor(cursor);
    let rect = state.rect(area, size);
    // Align the origin to the render target so integer zooms align pixel grids.
    let rect = Rect::from_min_size((rect.min.to_vec2() * ppp).round().to_pos2() / ppp, size);
    image::draw(&ui.painter().with_clip_rect(area), rect, id, image.clone(), interpolation);
    ui.data_mut(|data| data.insert_temp(id, state));
}

#[cfg(test)]
mod tests;
