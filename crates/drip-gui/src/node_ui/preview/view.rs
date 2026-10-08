//! Image drawing and navigation over resident samples or CPU proofing results.

use std::sync::Arc;

use egui::{FontId, Painter, Rect, Ui};

use crate::render::image;
use crate::render::node_views::{Drawable, ImageCache, IntoDrawable};
use drip::image::{ColorImage, Gpu, Rgb};

/// Additive Rec.2020 presentation values. Normal previews retain their device
/// allocation; proofing results remain on the host until the renderer needs them.
#[derive(Debug, Clone)]
pub struct PreviewImage {
    pub interpolation: bool,
    source: Source,
}
#[derive(Debug, Clone)]
enum Source {
    Host(Arc<Rgb>),
    Resident(Arc<ColorImage<Gpu>>),
}
impl PartialEq for PreviewImage {
    fn eq(&self, rhs: &Self) -> bool {
        self.interpolation == rhs.interpolation
            && match (&self.source, &rhs.source) {
                (Source::Host(a), Source::Host(b)) => a == b,
                (Source::Resident(a), Source::Resident(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}
impl PreviewImage {
    pub fn new(image: &ColorImage) -> Self {
        Self { source: Source::Host(image.rgb().clone()), interpolation: false }
    }
    pub fn resident(image: &ColorImage<Gpu>) -> Self {
        Self { source: Source::Resident(Arc::new(image.clone())), interpolation: false }
    }
    #[cfg(test)]
    pub fn gpu(&self) -> Option<&ColorImage<Gpu>> {
        match &self.source {
            Source::Resident(image) => Some(image),
            Source::Host(_) => None,
        }
    }
    /// CPU proofing results, used by independent numerical reference tests.
    #[cfg(test)]
    pub fn rgb(&self) -> &Arc<Rgb> {
        match &self.source {
            Source::Host(image) => image,
            Source::Resident(_) => panic!("resident preview requires explicit readback"),
        }
    }
}
impl IntoDrawable for PreviewImage {
    fn into_drawable(self, cache: &mut ImageCache) -> Arc<dyn Drawable> {
        let image = match self.source {
            Source::Host(image) => cache.image(&image),
            Source::Resident(image) => Arc::new(image::Image::resident(image)),
        };
        Arc::new(ImageView { image, interpolation: self.interpolation })
    }
}

/// Resident samples and sampling policy; navigation belongs to the displaying surface.
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
