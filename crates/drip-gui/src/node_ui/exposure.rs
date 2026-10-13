//! Auto exposure: meter the node's input at full detail with the library's
//! [`exposure::meter`], then set its EV.
use super::{Action, GuiNode};
use drip::{image::Image, node::exposure};

impl GuiNode for exposure::Definition {
    type Presentation = ();
    const ACTION: Option<Action<Self>> = Some(Action {
        name: "Auto exposure",
        run: |_, inputs, ctx| {
            let image = Image::from(inputs.image.data.clone());
            Ok(Some(exposure::meter(&image, &ctx.kernels())?))
        },
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::node_ui::data::PrepareContext;
    use drip::{
        Result,
        definition::Typed,
        eval::Evaluator,
        graph::Dag,
        node::{self, color, data::*},
        ports::NodeId,
        runtime::{KernelContext, RuntimeContext},
    };

    /// Source → Exposure → Sigmoid; returns Exposure's node.
    pub(crate) fn graph(pixels: Vec<[f32; 3]>) -> (Dag, NodeId) {
        let mut dag = Dag::new();
        let input = node::source::<Rgb>(
            &mut dag,
            RgbDesc::new(
                Extent { width: pixels.len() as u32, height: 1 },
                Coordinates::Space(REC2020_D65),
            ),
            pixels,
        )
        .unwrap();
        let exposure = exposure::add(&mut dag, Default::default()).unwrap();
        dag.connect(input, exposure.image).unwrap();
        let downstream = node::sigmoid::apply::add(&mut dag, Default::default()).unwrap();
        dag.connect(exposure.output, downstream.image).unwrap();
        (dag, exposure.output.node())
    }

    /// Runs the action as the worker does: on the node's inputs at full detail.
    fn metered(evaluator: &Evaluator, dag: &Dag, id: NodeId) -> Result<f32> {
        let inputs =
            evaluator.evaluate_targets(dag, &Default::default(), &[id]).remove(&id).unwrap()?;
        let ctx =
            PrepareContext { runtime: evaluator.runtime().clone(), global: Default::default() };
        let run = <exposure::Definition as GuiNode>::ACTION.unwrap().run;
        let node = exposure::Definition::of(dag.node(id)?)?;
        Ok(run(node, inputs.bind::<exposure::Definition>()?, &ctx)?.unwrap().ev)
    }

    #[test]
    fn metering_reads_the_input_whatever_the_current_ev() {
        let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
        let (mut dag, id) = graph(vec![[0.; 3], [0.045; 3], [0.09; 3]]);
        for start in [-3., 0., 4.] {
            dag.set_parameters(id, serde_json::json!({"ev": start})).unwrap();
            let ev = metered(&evaluator, &dag, id).unwrap();
            assert!((ev - 2.).abs() < 1e-6, "starting at {start}: {ev}");
        }
    }

    #[test]
    fn metering_weights_linear_luminance_and_keeps_signed_hdr_samples() {
        let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
        let (dag, id) = graph(vec![[0.5, 0., 0.], [0., 0.1, 0.], [0., 0., 8.], [-0.01; 3]]);
        // Rec.2020 Y coefficients; arithmetic mean, including the negative pixel.
        let mean: f64 = (0.5 * 0.262700212 + 0.1 * 0.677998072 + 8. * 0.059301716 - 0.01) / 4.;
        assert!(
            (f64::from(metered(&evaluator, &dag, id).unwrap()) - (0.18 / mean).log2()).abs() < 1e-6
        );
        for value in [0., -1., f32::NAN, f32::INFINITY] {
            let (dag, id) = graph(vec![[value; 3]]);
            assert!(metered(&evaluator, &dag, id).is_err(), "{value}");
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
            let image = evaluator.evaluate::<Rgb>(&dag, &Default::default(), output).unwrap();
            assert_eq!(image.statistics.downloads, 0);
            let global = Default::default();
            let kernels = KernelContext::new(&global, evaluator.runtime());
            let actual = exposure::mean_luminance(&Image::from(image.data), &kernels).unwrap();
            assert!(
                (actual - expected).abs() <= expected.abs() * 2e-6,
                "{count}: {actual} vs {expected}"
            );
        }
        // A plain f32 sum over this image overflows, while its mean is finite.
        let (dag, id) = graph(vec![[1e35; 3]; 65_537]);
        let ev = metered(&evaluator, &dag, id).unwrap();
        assert!((f64::from(ev) - (0.18 / f64::from(1e35f32)).log2()).abs() < 1e-5);
    }
}
