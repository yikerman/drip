//! Auto exposure: meter the node's input at full detail, then set its EV.
use super::{Action, GuiNode, data::argument};
use cubecl::prelude::*;
use drip::{
    Error, Result,
    node::{
        color,
        data::{Rec2020, Rgb},
        exposure,
    },
    ports::{Device, Read, Requirement},
};

#[drip_macros::gui_node]
impl GuiNode for exposure::Definition {
    type Presentation = ();
    // Meter before sigmoid; bright or dark subjects may need a manual adjustment.
    const ACTION: Option<Action<Self>> = Some(Action {
        name: "Auto exposure",
        run: |_, inputs, ctx| {
            Ok(Some(exposure::Exposure { ev: ev(&inputs.image, ctx.client()?)? }))
        },
    });
}

/// The EV that brings the whole-image mean luminance of `image` to 18% grey.
fn ev(image: &Read<'_, Device<Rgb>>, client: &Client) -> Result<f32> {
    Rec2020::check(image.desc)
        .map_err(|e| Error::Contract(format!("auto exposure requires {}: {e}", Rec2020::LABEL)))?;
    // Like darktable's area exposure mapping [1], compare linear luminance
    // against a target. Here the area is the whole image and the target is
    // exactly Y=0.18, rather than its default L*=50 (Y≈0.1842).
    // [1] darktable contributors, “exposure,” development user manual.
    // https://docs.darktable.org/usermanual/development/en/module-reference/processing-modules/exposure/
    let mean = mean_luminance(client, image.data.argument(), image.desc.extent.pixels())?;
    if !mean.is_finite() || mean <= 0.0 {
        return Err(Error::Contract(
            "auto exposure needs a finite, positive mean luminance".into(),
        ));
    }
    let params = exposure::Exposure { ev: (0.18 / mean).log2() as f32 };
    drip::param::Parameters::validate(&params)
        .map_err(|_| Error::Contract("auto exposure gain is not representable".into()))?;
    Ok(params.ev)
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
pub(crate) mod tests {
    use super::*;
    use crate::node_ui::data::PrepareContext;
    use drip::{
        eval::Evaluator, graph::Dag, node, node::data::*, ports::NodeId, runtime::RuntimeContext,
    };

    /// Source → Exposure → Sigmoid; returns Exposure's node.
    pub(crate) fn graph(pixels: Vec<[f32; 3]>) -> (Dag, NodeId) {
        let mut dag = Dag::new();
        let input = node::source::<Rgb>(
            &mut dag,
            RgbDesc {
                extent: Extent { width: pixels.len() as u32, height: 1 },
                coordinates: Coordinates::Space(REC2020_D65),
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

    /// Runs the action as the worker does: on the node's inputs at full detail.
    fn meter(evaluator: &Evaluator, dag: &Dag, id: NodeId) -> Result<f32> {
        let inputs =
            evaluator.evaluate_inputs(dag, &Default::default(), &[id]).remove(&id).unwrap()?;
        let ctx = PrepareContext {
            client: evaluator.client().ok().cloned(),
            resources: Default::default(),
            global: Default::default(),
        };
        let run = <exposure::Definition as GuiNode>::ACTION.unwrap().run;
        let current = serde_json::from_value(dag.node(id)?.parameters()?).unwrap();
        Ok(run(current, inputs.bind::<exposure::Definition>()?, &ctx)?.unwrap().ev)
    }

    #[test]
    fn metering_reads_the_input_whatever_the_current_ev() {
        let evaluator = Evaluator::new(RuntimeContext::from_env().unwrap());
        let (mut dag, id) = graph(vec![[0.; 3], [0.045; 3], [0.09; 3]]);
        for start in [-3., 0., 4.] {
            dag.set_parameters(id, serde_json::json!({"ev": start})).unwrap();
            let ev = meter(&evaluator, &dag, id).unwrap();
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
                evaluator.evaluate::<Device<Rgb>>(&dag, &Default::default(), output).unwrap();
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
}
