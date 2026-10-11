//! Meter the node's current output, then edit its ordinary exposure parameter.
use super::{ControlCx, GuiNode};
use drip::{
    Error, Result,
    eval::Evaluator,
    graph::Dag,
    node::{color, data::ColorRgb, exposure, raw},
    ports::{Cpu, NodeId},
    runtime::RuntimeContext,
};
use egui::Ui;

struct ExposureGui;
#[drip_macros::gui_node]
impl GuiNode for ExposureGui {
    type Parameters = exposure::Exposure;
    type Presentation = ();
    const ID: &'static str = "tone.exposure";

    fn controls(ui: &mut Ui, node: &mut ControlCx<'_, '_, '_, '_, Self::Parameters>) {
        node.schema(ui);
        let error_id = ui.id().with((node.node.id(), "auto-exposure-error"));
        if ui.add_enabled(!node.node.actions_disabled(), egui::Button::new("Auto exposure"))
            .on_hover_text("Set the whole-image mean luminance to 18% grey. Meter before sigmoid; bright or dark subjects may need a manual adjustment.")
            .clicked()
        {
            // This explicit, synchronous request stops at Exposure and uses full
            // detail, independently of preview size and downstream rendering.
            let result = RuntimeContext::from_env().and_then(|runtime| {
                meter(&Evaluator::new(runtime), &node.node.graph().dag, node.node.id())
            });
            match result {
                Ok(ev) => {
                    ui.data_mut(|data| data.remove::<String>(error_id));
                    node.node.set_param("ev", serde_json::json!(ev));
                }
                Err(error) => {
                    ui.data_mut(|data| data.insert_temp(error_id, error.to_string()));
                }
            }
        }
        if let Some(error) = ui.data(|data| data.get_temp::<String>(error_id)) {
            ui.colored_label(crate::theme::ERROR, error);
        }
    }
}

fn meter(evaluator: &Evaluator, dag: &Dag, id: NodeId) -> Result<f32> {
    let output = dag.typed_output(dag.output_port(id, "output")?)?;
    let image = evaluator.evaluate::<Cpu<ColorRgb>>(dag, &Default::default(), output)?;
    if image.desc.interpretation != raw::working_color() {
        return Err(Error::Contract(
            "auto exposure requires identity Rec.2020/D65 with relative white 1".into(),
        ));
    }
    // Like darktable's area exposure mapping [1], compare linear luminance
    // against a target. Here the area is the whole image and the target is
    // exactly Y=0.18, rather than its default L*=50 (Y≈0.1842).
    // [1] darktable contributors, “exposure,” development user manual.
    // https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/exposure/
    let weights = color::rgb_to_xyz(color::REC2020, color::D65)[1];
    let mean = image
        .data
        .iter()
        .map(|p| (0..3).map(|c| f64::from(p[c]) * weights[c]).sum::<f64>())
        .sum::<f64>()
        / image.data.len() as f64;
    // Retain signed/HDR samples; f64 accumulation avoids f32 sum overflow.
    if !mean.is_finite() || mean <= 0.0 {
        return Err(Error::Contract(
            "auto exposure needs a finite, positive mean luminance".into(),
        ));
    }
    let current: exposure::Exposure = serde_json::from_value(dag.node(id)?.parameters()?)
        .map_err(|error| Error::Contract(error.to_string()))?;
    // No reset to zero is needed: mean already includes 2^current.ev, so adding
    // log2(0.18 / mean) cancels the current gain. This reaches the same fixed
    // point as metering at zero EV (apart from floating-point rounding).
    let ev = (f64::from(current.ev) + (0.18 / mean).log2()) as f32;
    if !ev.is_finite() || !ev.exp2().is_finite() || ev.exp2() == 0.0 {
        return Err(Error::Contract("auto exposure gain is not representable".into()));
    }
    Ok(ev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use drip::{node, node::data::*};

    fn graph(pixels: Vec<[f32; 3]>) -> (Dag, NodeId) {
        let mut dag = Dag::new();
        let input = node::source::<ColorRgb>(
            &mut dag,
            ImageDesc {
                extent: Extent { width: pixels.len() as u32, height: 1 },
                interpretation: raw::working_color(),
            },
            pixels,
        )
        .unwrap();
        let exposure = exposure::add(&mut dag, Default::default()).unwrap();
        dag.connect(input, exposure.image).unwrap();
        let downstream = node::sigmoid::apply::add(&mut dag, Default::default()).unwrap();
        dag.connect(exposure.output, downstream.image).unwrap();
        (dag, exposure.output.node())
    }

    #[test]
    fn metering_reaches_the_same_fixed_point_from_different_ev() {
        let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
        let (mut dag, id) = graph(vec![[0.; 3], [0.045; 3], [0.09; 3]]);
        for start in [-3., 0., 4.] {
            dag.set_parameters(id, serde_json::json!({"ev": start})).unwrap();
            let ev = meter(&evaluator, &dag, id).unwrap();
            assert!((ev - 2.).abs() < 1e-6, "starting at {start}: {ev}");
            dag.set_parameters(id, serde_json::json!({"ev": ev})).unwrap();
            assert!((meter(&evaluator, &dag, id).unwrap() - ev).abs() < 1e-6);
        }
    }

    #[test]
    fn metering_weights_linear_luminance_and_keeps_signed_hdr_samples() {
        let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
        let (dag, id) = graph(vec![[0.5, 0., 0.], [0., 0.1, 0.], [0., 0., 8.], [-0.01; 3]]);
        // Rec.2020 Y coefficients; arithmetic mean, including the negative pixel.
        let mean: f64 = (0.5 * 0.262700212 + 0.1 * 0.677998072 + 8. * 0.059301716 - 0.01) / 4.;
        assert!(
            (f64::from(meter(&evaluator, &dag, id).unwrap()) - (0.18 / mean).log2()).abs() < 1e-6
        );
        for value in [0., -1., f32::NAN, f32::INFINITY] {
            let (dag, id) = graph(vec![[value; 3]]);
            assert!(meter(&evaluator, &dag, id).is_err(), "{value}");
        }
    }

    #[test]
    fn button_sets_ev_through_normal_edits_and_respects_loading() {
        use crate::{
            editing::{Frame, NodeCx, Report},
            model::{Graph, Registry},
        };
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };
        // The source need not have GUI registration: the panel only edits Exposure.
        let (dag, id) = graph(vec![[0.045; 3]]);
        let mut graph = Graph::default();
        graph.dag = dag;
        assert_eq!(graph.node(id).unwrap().kind.id, Registry.get("tone.exposure").unwrap().id);
        graph.set_param(id, "ev", serde_json::json!(3.)).unwrap();
        struct State {
            graph: Graph,
            disabled: bool,
            changed: bool,
        }
        let mut h = Harness::builder().with_size(egui::vec2(450., 300.)).build_ui_state(
            move |ui, state: &mut State| {
                let popped = Default::default();
                let mut frame = Frame::new(&|_| None, &popped, state.disabled);
                super::super::parameters::controls(
                    ui,
                    &mut NodeCx::new(&mut state.graph, id, &mut frame),
                );
                let Report { changed, edited, .. } = frame.report;
                state.changed |= changed && edited;
            },
            State { graph, disabled: false, changed: false },
        );
        h.get_by_label("Auto exposure").click();
        h.run();
        assert!(h.state().changed);
        assert!(
            (h.state().graph.node(id).unwrap().params["ev"].as_f64().unwrap() - 2.).abs() < 1e-6
        );
        h.get_by_label("Auto exposure").click();
        h.run();
        assert!(
            (h.state().graph.node(id).unwrap().params["ev"].as_f64().unwrap() - 2.).abs() < 1e-6
        );
        h.state_mut().disabled = true;
        h.run();
        assert!(h.get_by_label("Auto exposure").accesskit_node().is_disabled());
    }
}
