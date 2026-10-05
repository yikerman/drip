//! Node views (DESIGN U1): prepared for drawing on the worker, then drawn into
//! a rectangle of a node's body or window.

use std::sync::Arc;

use drip::value::{Histogram, Rgb, Scope, ScopeAxes, View};
use egui::{Align2, FontId, Painter, Rect, Stroke};

use crate::preview::{self, Image};
use crate::theme;

/// A view in the form the UI thread draws without further CPU work.
#[derive(Clone)]
pub enum PreparedView {
    Image(Arc<Image>),
    Histogram(Arc<Histogram>),
    Scope(Arc<Scope>, Arc<egui::Mesh>),
}

/// Prepared images by source, so an unchanged image is packed once. The owner
/// keeps every image until nothing else does, so the last drop happens there.
#[derive(Default)]
pub struct Prepared(Vec<(Arc<Rgb>, Arc<Image>)>);

impl Prepared {
    pub fn view(&mut self, view: &View) -> PreparedView {
        match view {
            View::Scope(scope) => PreparedView::Scope(scope.clone(), Arc::new(scope_mesh(scope))),
            View::Histogram(histogram) => PreparedView::Histogram(histogram.clone()),
            View::Image(value) => {
                let source = value.rgb();
                if let Some((_, image)) = self.0.iter().find(|(rgb, _)| Arc::ptr_eq(rgb, source)) {
                    return PreparedView::Image(image.clone());
                }
                let image = Arc::new(Image::new(source));
                self.0.push((source.clone(), image.clone()));
                PreparedView::Image(image)
            }
        }
    }

    /// Drops the images no one else holds.
    pub fn collect(&mut self) {
        self.0.retain(|(_, image)| Arc::strong_count(image) > 1);
    }
}

/// Draws `view` fitted into `rect`, labelling it in `font`.
pub fn draw(painter: &Painter, rect: Rect, id: egui::Id, view: &PreparedView, font: &FontId) {
    match view {
        PreparedView::Image(image) => {
            let fit = (rect.width() / image.width as f32).min(rect.height() / image.height as f32);
            let size = egui::vec2(image.width as f32, image.height as f32) * fit;
            let shown = Rect::from_center_size(rect.center(), size);
            preview::draw(painter, shown, id, image.clone());
        }
        PreparedView::Histogram(h) => histogram(painter, rect, h, font),
        PreparedView::Scope(scope, mesh) => draw_scope(painter, rect, scope, mesh, font),
    }
}

/// Each channel's counts per stop, linearly or on a log scale that keeps small
/// populations visible; the marked line is 1.0 (0 EV).
fn histogram(painter: &Painter, rect: Rect, h: &Histogram, font: &FontId) {
    let scale = |n: u32| if h.log { (n as f32).ln_1p() } else { n as f32 };
    let peak = h.counts.iter().flatten().map(|&c| scale(c)).fold(1.0, f32::max);
    let x = |i: usize| rect.left() + rect.width() * (i as f32 + 0.5) / h.counts.len() as f32;
    let colors = [
        egui::Color32::from_rgb(110, 20, 20),
        egui::Color32::from_rgb(20, 80, 20),
        egui::Color32::from_rgb(20, 30, 110),
    ];
    for (c, color) in colors.into_iter().enumerate() {
        let y = |n: &[u32; 3]| rect.bottom() - rect.height() * scale(n[c]) / peak;
        let points = h.counts.iter().enumerate().map(|(i, n)| egui::pos2(x(i), y(n))).collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
    }
    let zero = rect.left() + rect.width() * -h.min_stop / (h.max_stop - h.min_stop);
    painter.vline(zero, rect.y_range(), Stroke::new(1.0, theme::LIGHTER));
    let label = |at, align, text: String| painter.text(at, align, text, font.clone(), theme::WEAK);
    label(rect.left_bottom(), Align2::LEFT_BOTTOM, format!("{} EV", h.min_stop));
    label(egui::pos2(zero + 2.0, rect.top()), Align2::LEFT_TOP, "0 EV".into());
    label(rect.right_bottom(), Align2::RIGHT_BOTTOM, format!("+{} EV", h.max_stop));
}

// Build density geometry on the worker. Drawing only transforms the shared
// result, so image size never determines UI-thread counting work.
fn scope_mesh(scope: &Scope) -> egui::Mesh {
    let scale = |n: u32| if scope.log { (n as f32).ln_1p() } else { n as f32 };
    let peak = scope.counts.iter().flatten().map(|&n| scale(n)).fold(1.0, f32::max);
    let mut mesh = egui::Mesh::default();
    let size = scope.size as f32;
    for (i, count) in scope.counts.iter().enumerate() {
        if *count == [0; 3] {
            continue;
        }
        let rgb = count.map(|n| (255.0 * scale(n) / peak).round() as u8);
        let color = match scope.axes {
            ScopeAxes::Waveform { .. } => egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
            ScopeAxes::Vectorscope { .. } => egui::Color32::from_gray(rgb[0]),
        };
        let min = egui::pos2((i % scope.size) as f32 / size, (i / scope.size) as f32 / size);
        mesh.add_colored_rect(Rect::from_min_size(min, egui::vec2(1.0 / size, 1.0 / size)), color);
    }
    mesh
}

fn draw_scope(painter: &Painter, rect: Rect, scope: &Scope, mesh: &egui::Mesh, font: &FontId) {
    let rect = match scope.axes {
        ScopeAxes::Waveform { .. } => rect,
        ScopeAxes::Vectorscope { .. } => Rect::from_center_size(
            rect.center(),
            egui::Vec2::splat(rect.width().min(rect.height())),
        ),
    };
    painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
    let at = |[x, y]: [f32; 2]| rect.min + egui::vec2(x * rect.width(), y * rect.height());
    let mut mesh = mesh.clone();
    for vertex in &mut mesh.vertices {
        vertex.pos = at([vertex.pos.x, vertex.pos.y]);
    }
    painter.add(egui::Shape::mesh(mesh));
    let label = |pos, align, text: &str| painter.text(pos, align, text, font.clone(), theme::WEAK);
    match scope.axes {
        ScopeAxes::Waveform { min_stop, max_stop } => {
            let zero = at([0.0, max_stop / (max_stop - min_stop)]).y;
            painter.hline(rect.x_range(), zero, Stroke::new(1.0, theme::LIGHTER));
            label(rect.left_top(), Align2::LEFT_TOP, &format!("+{max_stop} EV"));
            label(egui::pos2(rect.left(), zero), Align2::LEFT_BOTTOM, "0 EV");
            label(rect.left_bottom(), Align2::LEFT_BOTTOM, &format!("{min_stop} EV"));
            label(rect.right_bottom(), Align2::RIGHT_BOTTOM, "image x →");
        }
        ScopeAxes::Vectorscope { primaries } => {
            painter.hline(rect.x_range(), rect.center().y, Stroke::new(0.5, theme::LIGHTER));
            painter.vline(rect.center().x, rect.y_range(), Stroke::new(0.5, theme::LIGHTER));
            let points = primaries.into_iter().chain([primaries[0]]).map(at).collect();
            painter.add(egui::Shape::line(points, Stroke::new(0.5, theme::LIGHTER)));
            for (point, name) in primaries.into_iter().zip(["R", "G", "B"]) {
                painter.circle_stroke(at(point), 3.0, Stroke::new(1.0, theme::WEAK));
                label(at(point) + egui::vec2(4.0, 0.0), Align2::LEFT_CENTER, name);
            }
            label(rect.left_top(), Align2::LEFT_TOP, "u′v′ · D65");
            label(rect.right_bottom(), Align2::RIGHT_BOTTOM, "Rec.2020");
        }
    }
}

#[cfg(test)]
mod tests {
    use drip::value::Value;

    use super::*;

    #[test]
    fn preparation_reuses_images_and_keeps_destruction_on_its_owner() {
        let source =
            Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[-1.0, 0.5, 2.0]] });
        let raw = Arc::downgrade(&source);
        let view = View::Image(Value::DisplayRec2020(source));
        let mut prepared = Prepared::default();
        let PreparedView::Image(first) = prepared.view(&view) else { panic!("image") };
        let PreparedView::Image(second) = prepared.view(&view) else { panic!("image") };
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            first.texels,
            [0xbc00u16, 0x3800, 0x4000, 0x3c00]
                .into_iter()
                .flat_map(u16::to_ne_bytes)
                .collect::<Vec<_>>()
        );
        let pixels = Arc::downgrade(&first);
        drop(view);
        drop(first);
        prepared.collect();
        assert!(raw.upgrade().is_some(), "renderer still owns the prepared image");
        drop(second);
        assert!(pixels.upgrade().is_some(), "the owner does the final destruction");
        prepared.collect();
        assert!(pixels.upgrade().is_none() && raw.upgrade().is_none());
    }
}
