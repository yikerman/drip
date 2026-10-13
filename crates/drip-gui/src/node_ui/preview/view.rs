//! Image drawing and navigation over worker-packed pixels.

use std::sync::Arc;

use egui::{FontId, Painter, Rect, Ui};

use crate::editing::NodeCx;
use crate::node_ui::data::HostImage;
use crate::render::image;
use crate::render::node_views::{Drawable, Images, IntoDrawable};

/// Shared linear Rec.2020 pixels for frontend presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewImage {
    /// Bilinear display sampling when true, nearest-neighbor otherwise.
    pub interpolation: bool,
    data: Arc<HostImage>,
}

impl PreviewImage {
    pub fn new(image: &Arc<HostImage>) -> Self {
        Self { data: image.clone(), interpolation: false }
    }
    pub fn rgb(&self) -> &Arc<HostImage> {
        &self.data
    }
}

impl IntoDrawable for PreviewImage {
    fn into_drawable(self, images: &mut Images) -> Arc<dyn Drawable> {
        Arc::new(ImageView { image: images.pack(self.rgb()), interpolation: self.interpolation })
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
        super::super::image_view::show(ui, &self.image, None, self.interpolation, false);
    }

    /// Draws the full-detail crop the worker holds over the preview, and asks
    /// for the visible region while the view magnifies the preview.
    fn interact(&self, ui: &mut Ui, node: &mut NodeCx) {
        let detail = node.detail().and_then(|view| view.downcast_ref::<ImageView>());
        let detail = detail.map(|view| &view.image);
        let shown =
            super::super::image_view::show(ui, &self.image, detail, self.interpolation, false);
        node.request_detail(shown.and_then(|shown| shown.detail));
    }
}
