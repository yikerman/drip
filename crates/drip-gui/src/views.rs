//! Drawing node views (DESIGN U1) into a rectangle of the editor.

use drip::value::{Histogram, View};
use egui::{Align2, FontId, Painter, Rect, Stroke};

use crate::{preview, theme};

/// Draws `view` fitted into `rect`.
pub fn draw(painter: &Painter, rect: Rect, id: egui::Id, view: &View) {
    match view {
        View::Image(value) => {
            let image = value.rgb();
            let fit = (rect.width() / image.width as f32).min(rect.height() / image.height as f32);
            let size = egui::vec2(image.width as f32, image.height as f32) * fit;
            let shown = Rect::from_center_size(rect.center(), size);
            painter.add(preview::shape(shown, id, image.clone()));
        }
        View::Histogram(h) => histogram(painter, rect, h),
    }
}

/// Each channel's counts per stop, scaled by the square root so that small
/// populations stay visible; the line marks 1.0 (0 EV).
fn histogram(painter: &Painter, rect: Rect, h: &Histogram) {
    let peak = h.counts.iter().flatten().map(|&c| (c as f32).sqrt()).fold(1.0, f32::max);
    let x = |i: usize| rect.left() + rect.width() * i as f32 / (h.counts.len() - 1) as f32;
    let colors = [
        egui::Color32::from_rgb(110, 20, 20),
        egui::Color32::from_rgb(20, 80, 20),
        egui::Color32::from_rgb(20, 30, 110),
    ];
    for (c, color) in colors.into_iter().enumerate() {
        let y = |n: &[u32; 3]| rect.bottom() - rect.height() * (n[c] as f32).sqrt() / peak;
        let points = h.counts.iter().enumerate().map(|(i, n)| egui::pos2(x(i), y(n))).collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
    }
    let zero = rect.left() + rect.width() * -h.min_stop / (h.max_stop - h.min_stop);
    painter.vline(zero, rect.y_range(), Stroke::new(1.0, theme::LIGHTER));
    let font = FontId::proportional(10.0);
    let label = |at, align, text: String| painter.text(at, align, text, font.clone(), theme::WEAK);
    label(rect.left_bottom(), Align2::LEFT_BOTTOM, format!("{} EV", h.min_stop));
    label(rect.right_bottom(), Align2::RIGHT_BOTTOM, format!("+{} EV", h.max_stop));
}
