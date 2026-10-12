//! Node selection, independent of graph edits and canvas coordinates.

use crate::model::Kind;
use egui::Ui;

pub(super) fn show(ui: &mut Ui) -> Option<&'static Kind> {
    let mut kinds: Vec<_> = crate::model::REGISTRY.kinds().collect();
    kinds.sort_by_key(|kind| (kind.category, kind.name, kind.id));
    let mut selected = None;
    for category in kinds.chunk_by(|a, b| a.category == b.category) {
        ui.menu_button(category[0].category, |ui| {
            for &kind in category {
                if ui.button(kind.name).clicked() {
                    selected = Some(kind);
                    ui.close();
                }
            }
        });
    }
    selected
}
