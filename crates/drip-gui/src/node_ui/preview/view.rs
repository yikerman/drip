//! Image drawing and navigation over worker-packed pixels.

use std::sync::Arc;

use egui::{FontId, Painter, Rect, Ui};

use crate::render::image;
use crate::render::node_views::{Drawable, ImageCache, IntoDrawable};
use drip::image::{RealMat, Rec2020Rgb, Rgb};

/// Linear Rec.2020 pixels for the preview shader, with reference semantics
/// erased only at this presentation boundary. Cannot be used as an export input.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewImage {
    /// Bilinear display sampling when true, nearest-neighbor otherwise.
    pub interpolation: bool,
    data: Arc<Rgb>,
}

impl PreviewImage {
    pub fn new<I: Rec2020Rgb>(image: &RealMat<3, I>) -> Self {
        Self { data: image.rgb().clone(), interpolation: false }
    }
    pub fn from_input(image: &drip::ports::MatRef<'_, 3, dyn Rec2020Rgb>) -> Self {
        Self { data: image.rgb().clone(), interpolation: false }
    }
    pub fn rgb(&self) -> &Arc<Rgb> {
        &self.data
    }
}

impl IntoDrawable for PreviewImage {
    fn into_drawable(self, cache: &mut ImageCache) -> Arc<dyn Drawable> {
        Arc::new(ImageView { image: cache.image(self.rgb()), interpolation: self.interpolation })
    }
}

/// Packed pixels and sampling policy; navigation belongs to the displaying surface.
pub struct ImageView {
    pub image: Arc<image::Image>,
    pub interpolation: bool,
}

impl Drawable for ImageView {
    fn draw(&self, painter: &Painter, rect: Rect, id: egui::Id, _: &FontId) {
        let fit =
            (rect.width() / self.image.width as f32).min(rect.height() / self.image.height as f32);
        let size = egui::vec2(self.image.width as f32, self.image.height as f32) * fit;
        let shown = Rect::from_center_size(rect.center(), size);
        image::draw(painter, shown, id, self.image.clone(), self.interpolation);
    }

    fn window(&self, ui: &mut Ui) {
        super::super::image_view::show(ui, &self.image, self.interpolation);
    }
}
