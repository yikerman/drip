//! Widgets node GUIs and the editor's frame share, in the units
//! of the `Ui` they are drawn into.

use egui::{Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::editing::NodeCx;
use crate::node_ui::Part;
use crate::theme;

/// Side of a pop-out button.
pub const BUTTON: f32 = 20.0;

/// A dropdown whose wheel navigation follows menu order and stops at either end.
pub fn dropdown<T: Copy + PartialEq>(
    ui: &mut Ui,
    combo: egui::ComboBox,
    value: &mut T,
    options: &[T],
    label: impl Fn(T) -> String,
) -> egui::Response {
    let previous = *value;
    let mut selected = options.iter().position(|option| option == value).expect("listed value");
    let mut response = combo.show_index(ui, &mut selected, options.len(), |i| label(options[i]));
    let steps = scroll_steps(ui, &response);
    selected = (selected as f64 - steps).clamp(0.0, (options.len() - 1) as f64) as usize;
    *value = options[selected];
    if *value != previous {
        response.mark_changed();
        ui.ctx().request_repaint();
    }
    response
}

pub fn scroll_steps(ui: &mut Ui, response: &egui::Response) -> f64 {
    let id = response.id.with("scroll");
    if response.hover_pos().is_none() {
        ui.data_mut(|data| data.remove::<f64>(id));
        return 0.0;
    }
    let delta = ui.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta.y));
    let line = ui.ctx().options(|options| options.input_options.line_scroll_speed);
    // egui smooths wheel input across frames; retain only the unfinished step,
    // never a second copy of the control's value.
    ui.data_mut(|data| {
        let remainder = data.get_temp_mut_or_default::<f64>(id);
        *remainder += f64::from(delta / line);
        let steps = remainder.round();
        *remainder -= steps;
        steps
    })
}

pub fn link(ui: &mut Ui, label: &str, url: &str) -> egui::Response {
    let text = egui::RichText::new(label).color(ui.visuals().hyperlink_color).underline();
    ui.hyperlink_to(text, url)
}

/// One screen point in the units of `ui`, which differ on the zoomed canvas.
pub fn point(ui: &Ui) -> f32 {
    ui.ctx().layer_transform_to_global(ui.layer_id()).map_or(1.0, |t| 1.0 / t.scaling)
}

/// A button over `rect` that pops `part` of the node out, or back in.
pub fn pop_out(ui: &mut Ui, rect: Rect, node: &mut NodeCx, part: Part) {
    let popped = node.popped(part);
    let icon = match part {
        Part::Parameters => "⚙",
        Part::View => "🗗",
    };
    let button = egui::Button::new(icon).frame(false).selected(popped);
    let hint = if popped { "Close window" } else { "Open separate window" };
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

#[cfg(test)]
mod tests;
