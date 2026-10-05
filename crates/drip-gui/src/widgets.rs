//! Widgets node GUIs and the editor's frame share (DESIGN G13), in the units
//! of the `Ui` they are drawn into.

use egui::{Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::editing::NodeCx;
use crate::node_ui::Part;
use crate::theme;

/// Side of a pop-out button.
pub const BUTTON: f32 = 20.0;

/// One screen point in the units of `ui`, which differ on the zoomed canvas.
pub fn point(ui: &Ui) -> f32 {
    ui.ctx().layer_transform_to_global(ui.layer_id()).map_or(1.0, |t| 1.0 / t.scaling)
}

/// A button over `rect` that pops `part` of the node out, or back in.
pub fn pop_out(ui: &mut Ui, rect: Rect, node: &mut NodeCx, part: Part) {
    let popped = node.popped(part);
    let icon = match part {
        Part::Parameters => "⚙",
        Part::Gui(_) => "🗗",
    };
    let button = egui::Button::new(icon).frame(false).selected(popped);
    let hint = if popped { "close its window" } else { "show in its own window" };
    if ui.put(rect, button).on_hover_text(format!("{}: {hint}", part.name())).clicked() {
        node.toggle(part);
    }
}

/// A handle ending at `corner` that resizes something of `size`; returns the
/// new size while dragged.
pub fn resize(ui: &mut Ui, corner: Pos2, size: Vec2) -> Option<Vec2> {
    let corner = Rect::from_min_max(corner - Vec2::splat(10.0), corner);
    ui.painter().line_segment(
        [corner.left_bottom(), corner.right_top()],
        Stroke::new(point(ui), theme::WEAK),
    );
    let handle = ui.interact(corner, ui.id().with("resize"), Sense::drag());
    handle.dragged().then(|| size + handle.drag_delta())
}
