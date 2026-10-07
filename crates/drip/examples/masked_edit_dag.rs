//! Branch, edit, and blend working RGB through a scalar mask. The edited branch
//! deliberately puts exposure after sigmoid. Run with
//! `cargo run -p drip --example masked_edit_dag`.
use drip::eval::Evaluator;
use drip::graph::{Graph, Port};
use drip::image::{RawMat, Rec2020Mat, Rgb};
use drip::node::{EvalContext, KernelError};
use drip::nodes;
use drip::value::EdgeValue;
use std::sync::Arc;

/// Dimensionless blend weights in [0, 1], indexed in the input image's grid.
#[derive(Debug)]
pub struct Mask(RawMat<1>);
impl EdgeValue for Mask {
    const NAME: &'static str = "Blend mask";
}

#[drip::node(kind = SOURCE, id = "demo.rgb", category = "demo", name = "RGB source", outputs = ["image", "mask"])]
fn source(_: (), (): (), _: &EvalContext<'_>) -> Result<(Arc<Rec2020Mat>, Arc<Mask>), KernelError> {
    let image = Rec2020Mat::from(Arc::new(Rgb {
        width: 3,
        height: 1,
        scale: 1,
        pixels: vec![[0.18; 3], [0.7, 0.2, 0.1], [3.0; 3]],
    }));
    let mask = Mask(RawMat { width: 3, height: 1, scale: 1, pixels: vec![[0.0], [0.5], [1.0]] });
    Ok((Arc::new(image), Arc::new(mask)))
}

fn matching_grids(
    _: (),
    (base, edited, mask): (&Rec2020Mat, &Rec2020Mat, &Mask),
    _: &EvalContext<'_>,
) -> Result<(), String> {
    let grid = |width, height, scale| (width, height, scale);
    let expected = grid(base.width, base.height, base.scale);
    if expected != grid(edited.width, edited.height, edited.scale)
        || expected != grid(mask.0.width, mask.0.height, mask.0.scale)
    {
        return Err("blend inputs must have matching grids".into());
    }
    Ok(())
}

/// Index-wise blending of working RGB: base * (1 - mask) + edited * mask.
/// The images may depict unrelated scenes; correspondence is by index.
#[drip::node(kind = BLEND, id = "demo.blend", category = "demo", name = "Blend", outputs = ["image"], checks = [("matching grids", matching_grids)])]
fn blend(
    _: (),
    (base, edited, mask): (&Rec2020Mat, &Rec2020Mat, &Mask),
    _: &EvalContext<'_>,
) -> Result<(Arc<Rec2020Mat>,), KernelError> {
    let pixels = base
        .pixels
        .iter()
        .zip(&edited.pixels)
        .zip(&mask.0.pixels)
        .map(|((a, b), &[m])| std::array::from_fn(|c| a[c] * (1.0 - m) + b[c] * m))
        .collect();
    Ok((Arc::new(base.with_buffer(Arc::new(Rgb { pixels, ..**base.rgb() }))),))
}

fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = Graph::default();
    let source = graph.add_node(&SOURCE);
    let sigmoid = graph.add_node(&nodes::SIGMOID);
    let exposure = graph.add_node(&nodes::EXPOSURE);
    let blend = graph.add_node(&BLEND);
    let image = |id| Port(id, "image".into());
    graph.connect(image(source), image(sigmoid))?;
    graph.connect(image(sigmoid), image(exposure))?;
    graph.set_param(exposure, "ev", 1.0.into())?;
    graph.connect(image(source), Port(blend, "base".into()))?;
    graph.connect(image(exposure), Port(blend, "edited".into()))?;
    graph.connect(Port(source, "mask".into()), Port(blend, "mask".into()))?;
    assert!(graph.connect(Port(source, "mask".into()), image(sigmoid)).is_err());
    let mut evaluator = Evaluator::default();
    evaluator.evaluate(&graph, 0, &[blend]);
    let result = |id| {
        evaluator.result(id).unwrap().as_ref().unwrap()[0].downcast_ref::<Rec2020Mat>().unwrap()
    };
    let (base, edited, output) = (result(source), result(exposure), result(blend));
    assert_eq!(output.pixels[0], base.pixels[0]);
    assert_eq!(output.pixels[2], edited.pixels[2]);
    assert_eq!(
        output.pixels[1],
        std::array::from_fn(|c| (base.pixels[1][c] + edited.pixels[1][c]) * 0.5)
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    demo()?;
    println!("Masked edit DAG: RGB → sigmoid → exposure → blend; mask and branch checks passed.");
    Ok(())
}

#[test]
fn creative_branch_and_mask_are_composable() {
    demo().unwrap();
}
