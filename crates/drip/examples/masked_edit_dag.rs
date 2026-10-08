//! Branch a color image, render and expose it on the GPU, then blend on the CPU.
//! This deliberately includes a CPU boundary: the evaluator performs the required
//! readback, while the node owns color and alignment refinements.
use drip::eval::Evaluator;
use drip::graph::{Graph, Port};
use drip::image::{ColorImage, ColorMeaning, CoverageMask, Cpu, RawMat, Rgb, SceneRelationship};
use drip::node::KernelError;
use drip::nodes;
use std::sync::Arc;

/// Supply additive Rec.2020 coordinates and coverage weights on the same grid.
/// The source assigns intended colors, without claiming measured scene light.
#[allow(clippy::type_complexity)]
#[drip::node(kind = SOURCE, id = "demo.rgb", category = "demo", name = "Color source", outputs = ["image", "mask"])]
fn source() -> Result<(Arc<ColorImage<Cpu>>, Arc<CoverageMask<Cpu>>), KernelError> {
    let image = ColorImage::try_new(
        Arc::new(Rgb {
            width: 3,
            height: 1,
            scale: 1,
            pixels: vec![[0.18; 3], [0.7, 0.2, 0.1], [3.0; 3]],
        }),
        ColorMeaning::rec2020(),
    )?;
    let mask = CoverageMask::coverage(Arc::new(RawMat {
        width: 3,
        height: 1,
        scale: 1,
        pixels: vec![[0.0], [0.5], [1.0]],
    }))?;
    Ok((Arc::new(image), Arc::new(mask)))
}

/// Mix represented colors by coverage: (1−m)a + mb in additive coordinates.
///
/// Requires equal grids and color references. Index correspondence is explicit;
/// the images need not depict the same scene. The output drops scene provenance:
/// mixing an original and rendered branch does not reconstruct captured light.
#[drip::node(kind = BLEND, id = "demo.blend", category = "demo", name = "Coverage blend", outputs = ["image"])]
fn blend(
    base: &ColorImage<Cpu>,
    edited: &ColorImage<Cpu>,
    mask: &CoverageMask<Cpu>,
) -> Result<Arc<ColorImage<Cpu>>, KernelError> {
    base.require_additive_color()?;
    edited.require_additive_color()?;
    if base.interpretation().coordinates != edited.interpretation().coordinates {
        return Err("blend color references differ".into());
    }
    let expected = (base.width(), base.height(), base.scale());
    if expected != (edited.width(), edited.height(), edited.scale())
        || expected != (mask.width(), mask.height(), mask.scale())
    {
        return Err("blend inputs must use the same grid".into());
    }
    let pixels = base
        .pixels
        .iter()
        .zip(&edited.pixels)
        .zip(&mask.pixels)
        .map(|((a, b), &[m])| std::array::from_fn(|c| a[c] * (1.0 - m) + b[c] * m))
        .collect();
    let meaning = ColorMeaning {
        coordinates: base.interpretation().coordinates,
        scene: SceneRelationship::Unspecified,
    };
    Ok(Arc::new(ColorImage::try_new(
        Arc::new(Rgb { width: base.width(), height: base.height(), scale: base.scale(), pixels }),
        meaning,
    )?))
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
    let output = evaluator.result(blend).unwrap().as_ref().unwrap()[0]
        .downcast::<ColorImage<Cpu>>()
        .unwrap();
    evaluator.with_inputs(&graph, blend, 0, &BLEND, |_, (base, edited, _), _| {
        assert_eq!(output.pixels[0], base.pixels[0]);
        assert_eq!(output.pixels[2], edited.pixels[2]);
        assert_eq!(
            output.pixels[1],
            std::array::from_fn(|c| { (base.pixels[1][c] + edited.pixels[1][c]) * 0.5 })
        );
        assert_eq!(edited.interpretation().scene, SceneRelationship::Rendered);
        Ok(())
    })?;
    assert_eq!(output.interpretation().scene, SceneRelationship::Unspecified);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    demo()?;
    println!(
        "CPU source → GPU sigmoid/exposure → CPU coverage blend: numerical and meaning checks passed."
    );
    Ok(())
}

#[test]
fn creative_branch_and_mask_are_composable() {
    demo().unwrap();
}
