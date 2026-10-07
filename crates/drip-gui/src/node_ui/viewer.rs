use super::{NodeView, PAD, Part};
use crate::editing::{NodeCx, pair};
use crate::render::node_views;
use crate::widgets::BUTTON;
use crate::{theme, widgets};
use drip::graph::Node;
use egui::{FontId, Rect, Ui, UiBuilder, Vec2, vec2};

/// Draws the node's view at a size the user sets from the body's corner; the
/// view pops out into a window filled by it.
pub(super) struct Viewer;

/// Default size of a view, in graph units.
const VIEW: Vec2 = vec2(280.0, 190.0);
const SHOWN: Part = Part::Gui("view");

/// The view's size the user set, which is the node's width and the body's
/// height without its bottom padding.
fn view_size(node: &Node) -> Vec2 {
    pair(&node.ui["size"]).unwrap_or(VIEW)
}

impl NodeView for Viewer {
    fn size(&self, node: &Node) -> Vec2 {
        view_size(node) + vec2(0.0, PAD)
    }

    fn body(&self, ui: &mut Ui, node: &mut NodeCx) {
        let (body, size) = (ui.max_rect(), view_size(node.node()));
        let rect = Rect::from_min_size(body.min + vec2(PAD, 0.0), size - vec2(2.0 * PAD, 0.0));
        if node.popped(SHOWN) {
            let layout = egui::Layout::centered_and_justified(egui::Direction::TopDown);
            ui.scope_builder(UiBuilder::new().max_rect(rect).layout(layout), |ui| {
                ui.weak("shown in its window")
            });
        } else if let Some(view) = node.view() {
            let font = FontId::proportional(theme::SMALL_SIZE);
            node_views::draw(ui.painter(), rect, ui.id().with("view"), view, &font);
        }
        // Over the view, so on a backdrop.
        let button = Rect::from_min_size(rect.right_top() - vec2(BUTTON, 0.0), Vec2::splat(BUTTON));
        ui.painter().rect_filled(button, 0.0, theme::DARKER);
        widgets::pop_out(ui, button, node, SHOWN);
        if let Some(size) = widgets::resize(ui, body.max, size) {
            node.set_ui("size", size.max(vec2(80.0, 60.0)));
        }
    }

    fn window_size(&self, _name: &'static str, node: &Node) -> Vec2 {
        view_size(node)
    }

    fn window(&self, ui: &mut Ui, _name: &'static str, node: &mut NodeCx) {
        if let Some(view) = node.view() {
            if let node_views::PreparedView::Image(image, interpolation) = view {
                super::image_view::show(ui, image, *interpolation);
                return;
            }
            let (rect, font) =
                (ui.available_rect_before_wrap(), FontId::proportional(theme::BODY_SIZE));
            node_views::draw(ui.painter(), rect, ui.id().with("view"), view, &font);
        }
    }
}
