//! Node selection, independent of graph edits and canvas coordinates.

use crate::model::Kind;
use crate::registry::REGISTRY;
use crate::theme::Tip;
use egui::Ui;

/// One submenu per tag in registry order; a kind appears under each of its tags.
pub(super) fn show(ui: &mut Ui) -> Option<&'static Kind> {
    let mut selected = None;
    for &tag in drip::registry::TAGS {
        let mut kinds = REGISTRY.kinds().filter(|kind| kind.tags.iter().any(|t| t.id == tag.id));
        let Some(first) = kinds.next() else { continue };
        ui.menu_button(tag.id, |ui| {
            for kind in std::iter::once(first).chain(kinds) {
                if ui.button(kind.name).clicked() {
                    selected = Some(kind);
                    ui.close();
                }
            }
        })
        .response
        .tip(tag.description);
    }
    selected
}
