//! Node views (DESIGN U1): prepared for drawing on the worker, then drawn into
//! a rectangle of the editor.

use std::sync::Arc;

use drip::value::{Histogram, Rgb, View};
use egui::{Align2, FontId, Painter, Rect, Stroke};

use crate::preview::{self, Image};
use crate::theme;

/// A view in the form the UI thread draws without further CPU work.
#[derive(Clone)]
pub enum PreparedView {
    Image(Arc<Image>),
    Histogram(Arc<Histogram>),
}

/// Prepared images by source, so an unchanged image is packed once. The owner
/// keeps every image until nothing else does, so the last drop happens there.
#[derive(Default)]
pub struct Prepared(Vec<(Arc<Rgb>, Arc<Image>)>);

impl Prepared {
    pub fn view(&mut self, view: &View) -> PreparedView {
        match view {
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
            painter.add(preview::shape(shown, id, image.clone()));
        }
        PreparedView::Histogram(h) => histogram(painter, rect, h, font),
    }
}

/// Each channel's counts per stop, scaled by the square root so that small
/// populations stay visible; the marked line is 1.0 (0 EV).
fn histogram(painter: &Painter, rect: Rect, h: &Histogram, font: &FontId) {
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
    let label = |at, align, text: String| painter.text(at, align, text, font.clone(), theme::WEAK);
    label(rect.left_bottom(), Align2::LEFT_BOTTOM, format!("{} EV", h.min_stop));
    label(egui::pos2(zero + 2.0, rect.top()), Align2::LEFT_TOP, "0 EV".into());
    label(rect.right_bottom(), Align2::RIGHT_BOTTOM, format!("+{} EV", h.max_stop));
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
