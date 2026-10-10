//! Sensor highlight reconstruction with independently supplied clipping levels.
use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
mod opposed;
#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Fraction of each site's clipping level at which reconstruction begins.
    #[param(crate::param::ParamKind::Float { min: 0.5, max: 1.0, default: 0.98 })]
    pub threshold: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self { threshold: 0.98 }
    }
}
fn contract(
    _: &GlobalContext,
    p: &Settings,
    image: Option<&BayerDesc>,
    levels: Option<&BayerLevelDesc>,
) -> Result<(Option<BayerDesc>,)> {
    if !p.threshold.is_finite() || p.threshold <= 0.0 || p.threshold > 1.0 {
        return Err(Error::Contract("highlight threshold must be in (0,1]".into()));
    }
    if let (Some(image), Some(levels)) = (image, levels)
        && (image.phase != levels.phase || image.interpretation != levels.interpretation)
    {
        return Err(Error::Contract(
            "highlight levels must describe the Bayer image's phase and coordinates".into(),
        ));
    }
    Ok((image.cloned(),))
}
/// Reconstruct clipped Bayer samples with inpaint opposed, assuming balanced
/// camera channels. BayerLevels supplies clipping levels in matching coordinates;
/// approximate levels are allowed. Output retains the Bayer interpretation.
/// Adapted from darktable's inpaint opposed algorithm; see the linked source.
#[crate::node(id="sensor.highlights-opposed", name="Reconstruct highlights (opposed)", category="Sensor", contract=contract, references=[("darktable inpaint opposed", "https://github.com/darktable-org/darktable/blob/61dea294bedb3ab6c7cca1a45530b1ab5c0461f3/src/iop/hlreconstruct/opposed.c")])]
pub fn reconstruct(
    ctx: &KernelContext<'_>,
    p: &Settings,
    image: Read<'_, Device<Bayer>>,
    levels: Read<'_, Cpu<BayerLevels>>,
    output: Write<'_, Device<Bayer>>,
) -> Result<()> {
    if levels.data.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(Error::Contract("highlight levels must be finite and positive".into()));
    }
    opposed::process(ctx, &image, levels.data.map(|v| v * p.threshold), output.data)
}
