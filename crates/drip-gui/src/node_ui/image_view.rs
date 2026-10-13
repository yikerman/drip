//! Pop-out image navigation, independent of graph edits and processing.
//!
//! Navigation works in the image's full-detail plane, so 100 % shows one
//! full-detail pixel per display pixel whatever the preview level. A preview
//! coarser than that is magnified, and the view reports the visible region
//! for a full-detail crop, which it draws over the preview once it arrives.

use std::sync::Arc;

use egui::{PointerButton, Pos2, Rect, Sense, Ui, Vec2, pos2};

use crate::render::image::{self, Image};
use crate::theme::Tip;

#[derive(Clone, Copy)]
struct ViewState {
    /// Render-target pixels per full-detail pixel; None follows the window.
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

/// Where `show` drew the image, for overlays.
pub(super) struct Shown {
    /// The navigation area.
    pub area: Rect,
    /// The whole image on screen, possibly beyond `area`.
    pub image: Rect,
    pub response: egui::Response,
    /// The visible region of the plane in full-detail pixels, when the view
    /// magnifies a preview coarser than full detail to 100 % or more.
    pub detail: Option<drip::shape::Rect>,
}

/// Navigable image, with `detail`, a full-detail crop of it, drawn over it.
/// With `editing`, the primary button is left to the caller's handles and the
/// secondary or middle button pans.
pub(super) fn show(
    ui: &mut Ui,
    image: &Arc<Image>,
    detail: Option<&Arc<Image>>,
    interpolation: bool,
    editing: bool,
) -> Option<Shown> {
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
            |zoom| zoom_label(zoom, &options),
        )
        .tip("100%: one full-detail pixel per display pixel");
        if state.zoom.is_none() {
            state.center = pos2(0.5, 0.5);
        }
        let requested = if image.requested_scale == 1 {
            "Full detail requested".into()
        } else {
            format!("1/{} detail requested", image.requested_scale)
        };
        let shown = if detail.is_some() { " · full detail in view" } else { "" };
        ui.weak(format!("{} × {} · {requested}{shown}", image.width, image.height));
    });

    let sense = if editing { Sense::click_and_drag() } else { Sense::drag() };
    let (area, response) = ui.allocate_exact_size(ui.available_size(), sense);
    if !area.is_positive() {
        ui.data_mut(|data| data.insert_temp(id, state));
        return None;
    }
    let pixels = Vec2::from(image.plane.full);
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
    let panning = if editing {
        response.dragged_by(PointerButton::Secondary) || response.dragged_by(PointerButton::Middle)
    } else {
        response.dragged_by(PointerButton::Primary)
    };
    if panning {
        state.pan(area, size, response.drag_delta());
    }
    let response = if panning {
        response.on_hover_and_drag_cursor(egui::CursorIcon::Grabbing)
    } else if editing {
        response
    } else {
        response.on_hover_and_drag_cursor(egui::CursorIcon::Grab)
    };
    let rect = state.rect(area, size);
    // Align the origin to the render target so integer zooms align pixel grids.
    let rect = Rect::from_min_size((rect.min.to_vec2() * ppp).round().to_pos2() / ppp, size);
    let painter = ui.painter().with_clip_rect(area);
    let scale = size / pixels;
    for (image, id) in std::iter::once((image, id)).chain(detail.map(|d| (d, id.with("detail")))) {
        let [min, max] = image.plane.covers.map(|corner| rect.min + Vec2::from(corner) * scale);
        image::draw(&painter, Rect::from_min_max(min, max), id, image.clone(), interpolation);
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    // `scale · ppp` counts display pixels per full-detail pixel.
    let magnified = image.requested_scale > 1 && scale.x * ppp >= 1.0 - 1e-3;
    let detail = magnified.then(|| visible(area, rect, pixels)).flatten();
    Some(Shown { area, image: rect, response, detail })
}

/// The cells of a plane of `plane` full-detail pixels, drawn over `rect`,
/// that meet `area`.
fn visible(area: Rect, rect: Rect, plane: Vec2) -> Option<drip::shape::Rect> {
    let shown = area.intersect(rect);
    if !shown.is_positive() {
        return None;
    }
    let to_plane = |p: Pos2| (p - rect.min) / rect.size() * plane;
    let (min, max) = (to_plane(shown.min), to_plane(shown.max));
    let axis = |min: f32, max: f32, side: f32| {
        let start = min.floor().clamp(0.0, side - 1.0);
        let end = max.ceil().clamp(start + 1.0, side);
        (start as u32, (end - start) as u32)
    };
    let ((x, width), (y, height)) = (axis(min.x, max.x, plane.x), axis(min.y, max.y, plane.y));
    Some(drip::shape::Rect { origin: [x, y], extent: [width, height] })
}

/// Retain enough precision to distinguish a wheel zoom from nearby presets.
fn zoom_label(zoom: Option<f32>, options: &[Option<f32>]) -> String {
    let Some(zoom) = zoom else { return "Fit".into() };
    let percent = f64::from(zoom) * 100.0;
    for precision in 0..=9 {
        let label = format!("{percent:.precision$}%");
        if options.iter().flatten().all(|&other| {
            other == zoom || format!("{:.precision$}%", f64::from(other) * 100.0) != label
        }) {
            return label;
        }
    }
    unreachable!("nine decimals distinguish supported f32 zoom values")
}

#[cfg(test)]
mod tests;
