//! Node selection, independent of graph edits and canvas coordinates.

use drip::node::{NodeKind, Registry};
use egui::Ui;

pub(super) fn show(ui: &mut Ui, registry: &Registry) -> Option<&'static NodeKind> {
    let mut kinds: Vec<_> = registry.kinds().collect();
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
