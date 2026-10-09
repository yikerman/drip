//! The displayed curve is sampled from the processing implementation.

use super::{ControlCx, GuiNode};
use crate::theme;
use drip::node::sigmoid;
use egui::{Color32, Sense, Shape, Stroke, Ui, vec2};

struct SigmoidGui;
#[drip_macros::gui_node]
impl GuiNode for SigmoidGui {
    type Parameters = sigmoid::Settings;
    type Presentation = ();
    const ID: &'static str = "apply-rec2020-sigmoid";
    fn controls(ui: &mut Ui, node: &mut ControlCx<'_, '_, '_, '_, Self::Parameters>) {
        node.schema(ui);
        let settings = node.parameters();
        let curve = settings.curve();
        // Bound the plot's preferred width when a pop-out measures its contents.
        let (rect, _) = ui.allocate_exact_size(
            vec2(ui.available_width().clamp(160.0, 320.0), 100.0),
            Sense::hover(),
        );
        let point = |x: f32, y: f32| {
            egui::pos2(rect.left() + x * rect.width(), rect.bottom() - y * rect.height())
        };
        // Log scene exposure on x, linear display value on y.
        let points = (0..=128)
            .map(|i| {
                let x = i as f32 / 128.0;
                point(x, curve(0.18 * (x * 16.0 - 8.0).exp2()))
            })
            .collect();
        ui.painter()
            .line_segment([point(0.5, 0.0), point(0.5, 1.0)], Stroke::new(1.0, theme::WEAK));
        ui.painter().add(Shape::line(points, Stroke::new(1.5, Color32::BLACK)));
        ui.small("−8 … +8 EV relative to middle grey");
    }
}
