//! Worker-prepared node drawing and shared image packing.

use std::{any::Any, sync::Arc};

use drip::image::Rgb;
use egui::{FontId, Painter, Rect, Ui};

use crate::render::image::Image;
use crate::theme;

/// A view the UI thread draws without further image processing.
pub trait PreparedView: Send + Sync + Any {
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

impl dyn PreparedView {
    #[cfg(test)]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }
}

/// Each presentation type owns its final worker-side drawing preparation.
pub trait IntoPrepared {
    fn prepare(self, cache: &mut Prepared) -> Arc<dyn PreparedView>;
}

/// Packed pixels and sampling policy; navigation belongs to the displaying surface.
pub struct ImageView {
    pub image: Arc<Image>,
    pub interpolation: bool,
}

/// Prepared images by source, so an unchanged image is packed once. The owner
/// keeps every image until nothing else does, so the last drop happens there.
#[derive(Default)]
pub struct Prepared(Vec<(Arc<Rgb>, Arc<Image>)>);

impl Prepared {
    pub fn image(&mut self, source: &Arc<Rgb>) -> Arc<Image> {
        if let Some((_, image)) = self.0.iter().find(|(rgb, _)| Arc::ptr_eq(rgb, source)) {
            return image.clone();
        }
        let image = Arc::new(Image::new(source));
        self.0.push((source.clone(), image.clone()));
        image
    }

    /// Drops the images no one else holds.
    pub fn collect(&mut self) {
        self.0.retain(|(_, image)| Arc::strong_count(image) > 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_ui::preview::PreviewImage;
    use drip::image::Rec2020Mat;

    #[test]
    fn preparation_reuses_images_and_keeps_destruction_on_its_owner() {
        let source =
            Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[-1.0, 0.5, 2.0]] });
        let raw = Arc::downgrade(&source);
        let mut value = PreviewImage::new(&Rec2020Mat::from(source));
        let mut prepared = Prepared::default();
        let first = {
            let view = value.clone().prepare(&mut prepared);
            let view = view.downcast_ref::<ImageView>().unwrap();
            assert!(!view.interpolation);
            view.image.clone()
        };
        value.interpolation = true;
        let second = {
            let view = value.prepare(&mut prepared);
            let view = view.downcast_ref::<ImageView>().unwrap();
            assert!(view.interpolation);
            view.image.clone()
        };
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            first.texels,
            [0xbc00u16, 0x3800, 0x4000, 0x3c00]
                .into_iter()
                .flat_map(u16::to_ne_bytes)
                .collect::<Vec<_>>()
        );
        let pixels = Arc::downgrade(&first);
        drop(first);
        prepared.collect();
        assert!(raw.upgrade().is_some(), "renderer still owns the prepared image");
        drop(second);
        assert!(pixels.upgrade().is_some(), "the owner does the final destruction");
        prepared.collect();
        assert!(pixels.upgrade().is_none() && raw.upgrade().is_none());
    }
}
