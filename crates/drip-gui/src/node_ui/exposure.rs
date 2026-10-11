//! Meter the node's current output, then edit its ordinary exposure parameter.
use super::{ControlCx, GuiNode};
use cubecl::{prelude::*, server::Handle};
#[cfg(test)]
use drip::runtime::RuntimeContext;
use drip::{
    Error, Result,
    eval::Evaluator,
    graph::Dag,
    node::{color, data::ColorRgb, exposure, raw},
    ports::{Device, NodeId},
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
        if ui.add_enabled(!node.node.actions_disabled(), egui::Button::new("Auto exposure"))
            .on_hover_text("Set the whole-image mean luminance to 18% grey. Meter before sigmoid; bright or dark subjects may need a manual adjustment.")
            .clicked()
        {
            node.node.meter_exposure();
        }
    }
}

pub(crate) fn meter(evaluator: &Evaluator, dag: &Dag, id: NodeId) -> Result<f32> {
    let output = dag.typed_output(dag.output_port(id, "output")?)?;
    let image = evaluator.evaluate::<Device<ColorRgb>>(dag, &Default::default(), output)?;
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
    let mean =
        mean_luminance(evaluator.client()?, image.data.argument(), image.desc.extent.pixels())?;
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

fn argument(handle: &Handle) -> BufferArg {
    // SAFETY: reduction scratch is a scalar f32 array on every pass.
    unsafe { BufferArg::from_raw_parts(handle.clone(), 1) }
}

fn mean_luminance(client: &Client, mut input: BufferArg, pixels: usize) -> Result<f64> {
    // Normalize before summing to keep HDR headroom. Each invocation sums at
    // most 256 values; successive passes reduce those contributions to one f32.
    let weights =
        color::rgb_to_xyz(color::REC2020, color::D65)[1].map(|v| (v / pixels as f64) as f32);
    let dim = CubeDim::new_1d(client.properties().hardware.max_units_per_cube.min(256));
    let mut len = pixels;
    let mut rgb = true;
    loop {
        let count = len.div_ceil(256);
        let output = client.empty(count * 4);
        kernel::reduce::launch(
            client,
            CubeCount::Static(count.div_ceil(dim.num_elems() as usize) as u32, 1, 1),
            dim,
            input,
            argument(&output),
            weights[0],
            weights[1],
            weights[2],
            rgb,
        );
        if count == 1 {
            let bytes = client.read_one(output).map_err(|e| Error::Runtime(e.to_string()))?;
            return Ok(f64::from(f32::from_ne_bytes(bytes[..4].try_into().unwrap())));
        }
        input = argument(&output);
        len = count;
        rgb = false;
    }
}

mod kernel {
    use cubecl::prelude::*;

    #[cube(launch)]
    pub fn reduce(
        input: &[f32],
        output: &mut [f32],
        r: f32,
        g: f32,
        b: f32,
        #[comptime] rgb: bool,
    ) {
        let i = ABSOLUTE_POS;
        if i < output.len() {
            let mut sum = 0.0f32;
            let mut pixel = i;
            let count = if rgb { input.len() / 3 } else { input.len() };
            // Striding by the output length keeps adjacent lanes' reads coalesced.
            while pixel < count {
                if rgb {
                    sum +=
                        input[pixel * 3] * r + input[pixel * 3 + 1] * g + input[pixel * 3 + 2] * b;
                } else {
                    sum += input[pixel];
                }
                pixel += output.len();
            }
            output[i] = sum;
        }
    }
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
    fn device_mean_matches_reference_across_reduction_boundaries() {
        let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
        for count in [1, 255, 256, 257, 65_535, 65_536, 65_537, 1_000_003] {
            let pixels: Vec<_> = (0..count)
                .map(|i| {
                    [
                        ((i * 17) % 101) as f32 * 0.003 - 0.01,
                        ((i * 7) % 53) as f32 * 0.02,
                        ((i * 3) % 97) as f32 * 0.1,
                    ]
                })
                .collect();
            let weights = color::rgb_to_xyz(color::REC2020, color::D65)[1];
            let expected = pixels
                .iter()
                .map(|p| (0..3).map(|c| f64::from(p[c]) * weights[c]).sum::<f64>())
                .sum::<f64>()
                / count as f64;
            let (dag, id) = graph(pixels);
            let output = dag.typed_output(dag.output_port(id, "output").unwrap()).unwrap();
            let image =
                evaluator.evaluate::<Device<ColorRgb>>(&dag, &Default::default(), output).unwrap();
            assert_eq!(image.statistics.downloads, 0);
            let actual =
                mean_luminance(evaluator.client().unwrap(), image.data.argument(), count).unwrap();
            assert!(
                (actual - expected).abs() <= expected.abs() * 2e-6,
                "{count}: {actual} vs {expected}"
            );
        }
        // A plain f32 sum over this image overflows, while its mean is finite.
        let (dag, id) = graph(vec![[1e35; 3]; 65_537]);
        let ev = meter(&evaluator, &dag, id).unwrap();
        assert!((f64::from(ev) - (0.18 / f64::from(1e35f32)).log2()).abs() < 1e-5);
    }

    #[test]
    fn button_requests_background_metering_and_respects_loading() {
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
            requested: Option<NodeId>,
        }
        let mut h = Harness::builder().with_size(egui::vec2(450., 300.)).build_ui_state(
            move |ui, state: &mut State| {
                let popped = Default::default();
                let mut frame = Frame::new(&|_| None, &popped, state.disabled);
                super::super::parameters::controls(
                    ui,
                    &mut NodeCx::new(&mut state.graph, id, &mut frame),
                );
                let Report { meter, .. } = frame.report;
                state.requested = state.requested.or(meter);
            },
            State { graph, disabled: false, requested: None },
        );
        h.get_by_label("Auto exposure").click();
        h.run();
        assert_eq!(h.state().requested, Some(id));
        assert_eq!(h.state().graph.node(id).unwrap().params["ev"], serde_json::json!(3.));
        h.state_mut().disabled = true;
        h.run();
        assert!(h.get_by_label("Auto exposure").accesskit_node().is_disabled());
    }
}
