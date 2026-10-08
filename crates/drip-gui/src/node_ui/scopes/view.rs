//! Plot drawing and worker-prepared density meshes.

use std::sync::Arc;

use drip::color::{self, D65, REC709};
use egui::{Align2, FontId, Painter, Rect, Stroke};

use super::vectorscope_xyz;
use super::{Histogram, Scope, ScopeAxes};
use crate::render::node_views::{Drawable, ImageCache, IntoDrawable};
use crate::theme;

impl IntoDrawable for Arc<Histogram> {
    fn into_drawable(self, _: &mut ImageCache) -> Arc<dyn Drawable> {
        self
    }
}

impl Drawable for Histogram {
    fn draw(&self, painter: &Painter, rect: Rect, _: egui::Id, font: &FontId) {
        histogram(painter, rect, self, font);
    }
}

struct ScopeView {
    scope: Arc<Scope>,
    mesh: egui::Mesh,
}

impl IntoDrawable for Arc<Scope> {
    fn into_drawable(self, _: &mut ImageCache) -> Arc<dyn Drawable> {
        let mesh = scope_mesh(&self);
        Arc::new(ScopeView { scope: self, mesh })
    }
}

impl Drawable for ScopeView {
    fn draw(&self, painter: &Painter, rect: Rect, _: egui::Id, font: &FontId) {
        draw_scope(painter, rect, &self.scope, &self.mesh, font);
    }
}

/// Shared presentation for diagnostic plots, independent of the image surround.
struct Plot<'a> {
    painter: Painter,
    rect: Rect,
    font: &'a FontId,
}

impl<'a> Plot<'a> {
    fn new(painter: &Painter, rect: Rect, font: &'a FontId) -> Self {
        let painter = painter.with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
        Self { painter, rect, font }
    }

    fn at(&self, [x, y]: [f32; 2]) -> egui::Pos2 {
        self.rect.min + egui::vec2(x * self.rect.width(), y * self.rect.height())
    }

    fn label(&self, pos: egui::Pos2, align: Align2, text: &str) {
        self.painter.text(pos, align, text, self.font.clone(), egui::Color32::from_gray(180));
    }
}

fn normalized_counts(counts: &[[u32; 3]], log: bool) -> impl Fn(u32) -> f32 {
    let scale = move |n: u32| if log { (n as f32).ln_1p() } else { n as f32 };
    let peak = counts.iter().flatten().map(|&n| scale(n)).fold(1.0, f32::max);
    move |n| scale(n) / peak
}

fn channel_color(channels: [f32; 3]) -> egui::Color32 {
    let [r, g, b] = channels.map(|v| (255.0 * v).round() as u8);
    egui::Color32::from_rgb(r, g, b)
}

fn ev_position(ev: f32, min: f32, max: f32) -> f32 {
    (ev - min) / (max - min)
}

/// Each native channel's counts per log2(value) bin; the marked line is value 1.
/// A separate optional log scale makes small populations visible.
fn histogram(painter: &Painter, rect: Rect, h: &Histogram, font: &FontId) {
    let plot = Plot::new(painter, rect, font);
    let painter = &plot.painter;
    let density = normalized_counts(&h.counts, h.log);
    let x = |i: usize| rect.left() + rect.width() * (i as f32 + 0.5) / h.counts.len() as f32;
    for c in 0..3 {
        let color =
            channel_color(std::array::from_fn(|channel| if channel == c { 1.0 } else { 0.0 }));
        let y = |n: &[u32; 3]| rect.bottom() - rect.height() * density(n[c]);
        let points = h.counts.iter().enumerate().map(|(i, n)| egui::pos2(x(i), y(n))).collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
    }
    let zero = plot.at([ev_position(0.0, h.min_stop, h.max_stop), 0.0]).x;
    painter.vline(zero, rect.y_range(), Stroke::new(1.0, theme::LIGHTER));
    plot.label(rect.left_bottom(), Align2::LEFT_BOTTOM, &format!("{}", h.min_stop));
    plot.label(egui::pos2(zero + 2.0, rect.top()), Align2::LEFT_TOP, "0 · log₂ value");
    plot.label(rect.right_bottom(), Align2::RIGHT_BOTTOM, &format!("+{}", h.max_stop));
}

// Build density geometry on the worker. Drawing only transforms the shared
// result, so image size never determines UI-thread counting work.
fn scope_mesh(scope: &Scope) -> egui::Mesh {
    let density = normalized_counts(&scope.counts, scope.log);
    let xyz_to_srgb = color::inverse(&color::rgb_to_xyz(REC709, D65));
    let mut mesh = egui::Mesh::default();
    let size = scope.size as f32;
    for (i, count) in scope.counts.iter().enumerate() {
        if *count == [0; 3] {
            continue;
        }
        let color = match scope.axes {
            ScopeAxes::Waveform { .. } => channel_color(count.map(&density)),
            ScopeAxes::Vectorscope { .. } => {
                let center = [(i % scope.size) as f32 + 0.5, (i / scope.size) as f32 + 0.5];
                vectorscope_color(center.map(|v| v / size), density(count[0]), &xyz_to_srgb)
            }
        };
        let min = egui::pos2((i % scope.size) as f32 / size, (i / scope.size) as f32 / size);
        mesh.add_colored_rect(Rect::from_min_size(min, egui::vec2(1.0 / size, 1.0 / size)), color);
    }
    mesh
}

// Scope colors are sRGB UI annotations: clip out-of-gamut components and
// normalize the peak before encoding. Density scales the visible color, while
// neutral chromaticities stay white and the plot's geometry stays unchanged.
fn vectorscope_color(position: [f32; 2], density: f32, xyz_to_srgb: &color::Mat3) -> egui::Color32 {
    let rgb = color::apply(xyz_to_srgb, vectorscope_xyz(position)).map(|v| v.max(0.0));
    let peak = rgb.into_iter().fold(0.0, f64::max);
    let encoded = rgb.map(|v| {
        let linear = v / peak;
        let srgb = if linear <= 0.0031308 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (255.0 * srgb * f64::from(density)).round() as u8
    });
    egui::Color32::from_rgb(encoded[0], encoded[1], encoded[2])
}

fn draw_scope(painter: &Painter, rect: Rect, scope: &Scope, mesh: &egui::Mesh, font: &FontId) {
    let rect = match scope.axes {
        ScopeAxes::Waveform { .. } => rect,
        ScopeAxes::Vectorscope { .. } => Rect::from_center_size(
            rect.center(),
            egui::Vec2::splat(rect.width().min(rect.height())),
        ),
    };
    let plot = Plot::new(painter, rect, font);
    let painter = &plot.painter;
    let at = |point| plot.at(point);
    let mut mesh = mesh.clone();
    for vertex in &mut mesh.vertices {
        vertex.pos = at([vertex.pos.x, vertex.pos.y]);
    }
    painter.add(egui::Shape::mesh(mesh));
    match scope.axes {
        ScopeAxes::Waveform { min_stop, max_stop } => {
            let zero = at([0.0, 1.0 - ev_position(0.0, min_stop, max_stop)]).y;
            painter.hline(rect.x_range(), zero, Stroke::new(1.0, theme::LIGHTER));
            plot.label(rect.left_top(), Align2::LEFT_TOP, &format!("+{max_stop}"));
            plot.label(egui::pos2(rect.left(), zero), Align2::LEFT_BOTTOM, "0 · log₂ value");
            plot.label(rect.left_bottom(), Align2::LEFT_BOTTOM, &format!("{min_stop}"));
            plot.label(rect.right_bottom(), Align2::RIGHT_BOTTOM, "image x");
        }
        ScopeAxes::Vectorscope { primaries, color_space } => {
            painter.hline(rect.x_range(), rect.center().y, Stroke::new(0.5, theme::LIGHTER));
            painter.vline(rect.center().x, rect.y_range(), Stroke::new(0.5, theme::LIGHTER));
            let points = primaries.into_iter().chain([primaries[0]]).map(at).collect();
            painter.add(egui::Shape::line(points, Stroke::new(0.5, theme::LIGHTER)));
            for (point, name) in primaries.into_iter().zip(["R", "G", "B"]) {
                painter.circle_stroke(at(point), 3.0, Stroke::new(1.0, theme::LIGHTER));
                plot.label(at(point) + egui::vec2(4.0, 0.0), Align2::LEFT_CENTER, name);
            }
            plot.label(rect.left_top(), Align2::LEFT_TOP, "u'v' · D65");
            plot.label(rect.right_bottom(), Align2::RIGHT_BOTTOM, color_space);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectorscope_colors_preserve_neutrals_primaries_and_density() {
        let matrix = color::inverse(&color::rgb_to_xyz(REC709, D65));
        assert_eq!(vectorscope_color([0.5; 2], 1.0, &matrix), egui::Color32::WHITE);
        assert_eq!(vectorscope_color([0.5; 2], 0.25, &matrix), egui::Color32::from_gray(64));
        for (channel, [x, y]) in REC709.into_iter().enumerate() {
            let denominator = -2.0 * x + 12.0 * y + 3.0;
            let position = [
                (0.5 + 4.0 * x / denominator - 0.1978300066) as f32,
                (0.5 - 9.0 * y / denominator + 0.4683199949) as f32,
            ];
            let color = vectorscope_color(position, 1.0, &matrix).to_array();
            assert_eq!(color[channel], 255);
            for (other, &value) in color[..3].iter().enumerate() {
                if other != channel {
                    assert!(value <= 1, "{color:?}");
                }
            }
        }
    }
}
