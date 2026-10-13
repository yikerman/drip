//! Pyramid blur: a meta node that blurs at a fraction of the input's detail.
//! It expands into Resample and Sharpen nodes, so its levels are keyed,
//! stored and cropped like any node's while the graph shows one node.
use crate::{
    Error, Result,
    expand::SubGraph,
    node::{
        data::*,
        filter::Filter,
        resample::{self, Exact, Factor, Size},
        sharpen,
    },
    ports::*,
    registry::tag,
};
pub use expand::{Definition, Ports, add, definition};

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// Halvings of the image, each followed by a blur.
    #[param(default = 3, range = 1..=8)]
    pub levels: i64,
    /// Standard deviation of the blur at each level, in that level's pixels.
    #[param(default = 1.0, range = 0.25..=4.0)]
    pub radius: f32,
}
fn validate(p: &Settings) -> Result<()> {
    if !(1..=8).contains(&p.levels) || !(0.25..=4.0).contains(&p.radius) {
        return Err(Error::Contract(
            "pyramid blur needs 1 to 8 levels and a radius in [0.25, 4]".into(),
        ));
    }
    Ok(())
}

/// Blur through a Gaussian pyramid. The image is halved `levels` times with
/// a box filter, each level is blurred with a Gaussian of `radius` pixels of
/// that level (Sharpen at amount −1), and the coarsest level is resampled
/// back to the input's full-detail size with Mitchell–Netravali. Each level
/// doubles the blur's reach in the input's pixels at a quarter of the cost,
/// so a wide blur costs a few small stencils. A flat image passes unchanged.
/// The node runs as those Resample and Sharpen nodes, and a 100 % view
/// computes only the part of each level it reads. Coordinates are kept.
#[crate::node(id = "pyramid-blur", name = "Pyramid blur", tags = [tag::LOOK], meta)]
pub fn expand(
    p: &Settings,
    graph: &mut SubGraph,
    image: Inlet<'_, Rgb>,
    output: Outlet<Rgb>,
) -> Result<()> {
    let halve =
        resample::Settings { filter: Filter::Box, size: Size::Factor(Factor { factor: 0.5 }) };
    let blur = sharpen::Settings { radius: p.radius, amount: -1.0 };
    let mut level: Option<Output<Rgb>> = None;
    for _ in 0..p.levels {
        let down = resample::add(&mut graph.dag, halve.clone())?;
        match level {
            Some(level) => graph.dag.connect(level, down.image)?,
            None => graph.input(image, down.image)?,
        }
        let soft = sharpen::add(&mut graph.dag, blur.clone())?;
        graph.dag.connect(down.output, soft.image)?;
        level = Some(soft.output);
    }
    let Extent { width, height } = image.desc.full;
    let size = Size::Exact(Exact { width: width.into(), height: height.into() });
    let up = resample::add(&mut graph.dag, resample::Settings { filter: Filter::Mitchell, size })?;
    graph.dag.connect(level.expect("validation asks for a level"), up.image)?;
    graph.output(up.output, output)
}
