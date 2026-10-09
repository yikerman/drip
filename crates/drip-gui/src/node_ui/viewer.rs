use super::{PAD, Part};
use crate::editing::NodeCx;
use crate::model::Node;
use crate::ui_state::{self, LayoutField};
use crate::widgets::BUTTON;
use crate::{theme, widgets};
use egui::{FontId, Rect, Ui, UiBuilder, Vec2, vec2};

const SHOWN: Part = Part::View;

/// The view's size the user set, which is the node's width and the body's
/// height without its bottom padding.
pub(super) fn size(node: &Node) -> Vec2 {
    ui_state::view_size(&node.ui)
}

pub(super) fn body(ui: &mut Ui, node: &mut NodeCx) {
    let (body, size) = (ui.max_rect(), size(&node.node()));
    let rect = Rect::from_min_size(body.min + vec2(PAD, 0.0), size - vec2(2.0 * PAD, 0.0));
    if node.popped(SHOWN) {
        let layout = egui::Layout::centered_and_justified(egui::Direction::TopDown);
        ui.scope_builder(UiBuilder::new().max_rect(rect).layout(layout), |ui| {
            ui.weak("shown in its window")
        });
    } else if let Some(view) = node.view() {
        let font = FontId::proportional(theme::SMALL_SIZE);
        view.draw(ui.painter(), rect, ui.id().with("view"), &font);
    }
    // Over the view, so on a backdrop.
    let button = Rect::from_min_size(rect.right_top() - vec2(BUTTON, 0.0), Vec2::splat(BUTTON));
    ui.painter().rect_filled(button, 0.0, theme::DARKER);
    widgets::pop_out(ui, button, node, SHOWN);
    if let Some(size) = widgets::resize(ui, body.max, size) {
        node.set_ui(LayoutField::ViewSize, size.max(ui_state::MIN_VIEW_SIZE));
    }
}

pub(super) fn window(ui: &mut Ui, node: &mut NodeCx) {
    if let Some(view) = node.view() {
        view.window(ui);
    }
}
