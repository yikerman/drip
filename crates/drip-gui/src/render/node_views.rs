//! Worker-prepared node drawing and shared image packing.

use std::{any::Any, sync::Arc};

use crate::node_ui::data::HostImage;
use egui::{FontId, Painter, Rect, Ui};

use crate::render::image::Image;
use crate::theme;

/// A view the UI thread draws without further image processing.
pub trait Drawable: Send + Sync + Any {
    fn draw(&self, painter: &Painter, rect: Rect, id: egui::Id, font: &FontId);

    fn window(&self, ui: &mut Ui) {
        self.draw(
            ui.painter(),
            ui.available_rect_before_wrap(),
            ui.id().with("view"),
            &FontId::proportional(theme::BODY_SIZE),
        );
    }
}

impl dyn Drawable {
    #[cfg(test)]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }
}

/// Each presentation type owns its final worker-side drawing preparation.
pub trait IntoDrawable {
    fn into_drawable(self, images: &mut Images) -> Arc<dyn Drawable>;
}

/// Keeps packed images until nothing else does, so the last drop happens on
/// the worker. Source pixels are released as soon as preparation finishes.
#[derive(Default)]
pub struct Images(Vec<Arc<Image>>);

impl Images {
    pub fn pack(&mut self, source: &HostImage) -> Arc<Image> {
        let image = Arc::new(Image::new(source));
        self.0.push(image.clone());
        image
    }

    /// Drops the images no one else holds.
    pub fn collect(&mut self) {
        self.0.retain(|image| Arc::strong_count(image) > 1);
    }
}

impl Drawable for () {
    fn draw(&self, _: &egui::Painter, _: egui::Rect, _: egui::Id, _: &egui::FontId) {}
}
impl IntoDrawable for () {
    fn into_drawable(self, _: &mut Images) -> Arc<dyn Drawable> {
        Arc::new(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_ui::preview::{ImageView, PreviewImage};

    #[test]
    fn preparation_releases_source_and_keeps_packed_destruction_on_its_owner() {
        let source = Arc::new(HostImage {
            width: 1,
            height: 1,
            requested_scale: 1,
            color_space: "Rec.2020 / D65".into(),
            pixels: vec![[-1.0, 0.5, 2.0]].into(),
        });
        let raw = Arc::downgrade(&source);
        let mut value = PreviewImage::new(&source);
        drop(source);
        value.interpolation = true;
        let mut images = Images::default();
        let first = {
            let view = value.into_drawable(&mut images);
            let view = view.downcast_ref::<ImageView>().unwrap();
            assert!(view.interpolation);
            view.image.clone()
        };
        assert!(raw.upgrade().is_none(), "packing must release the source pixels");
        assert_eq!(
            first.texels,
            [0xbc00u16, 0x3800, 0x4000, 0x3c00]
                .into_iter()
                .flat_map(u16::to_ne_bytes)
                .collect::<Vec<_>>()
        );
        let pixels = Arc::downgrade(&first);
        images.collect();
        assert!(pixels.upgrade().is_some(), "renderer still owns the prepared image");
        drop(first);
        assert!(pixels.upgrade().is_some(), "the owner does the final destruction");
        images.collect();
        assert!(pixels.upgrade().is_none() && raw.upgrade().is_none());
    }
}
