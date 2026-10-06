//! The displayed curve is sampled from the processing implementation.

use super::{NodeUi, parameters};
use crate::{editing::NodeCx, theme};
use drip::{nodes::sigmoid, param::Params};
use egui::{Color32, Sense, Shape, Stroke, Ui, vec2};

pub(super) struct Controls;

impl NodeUi for Controls {
    fn controls(&self, ui: &mut Ui, node: &mut NodeCx) {
        parameters::schema(ui, node);
        let curve = sigmoid::settings(Params::validated(&node.node().params));
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
                point(x, curve.curve(sigmoid::GREY * (x * 16.0 - 8.0).exp2()))
            })
            .collect();
        ui.painter()
            .line_segment([point(0.5, 0.0), point(0.5, 1.0)], Stroke::new(1.0, theme::WEAK));
        ui.painter().add(Shape::line(points, Stroke::new(1.5, Color32::BLACK)));
        ui.small("−8 … +8 EV relative to middle grey");
    }
}
