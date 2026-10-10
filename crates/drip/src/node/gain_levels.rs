use crate::{
    Error, Result,
    node::data::*,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
pub use compute::{Definition, Ports, add, definition};
fn gain_contract(
    _: &GlobalContext,
    _: &(),
    levels: Option<&BayerLevelDesc>,
    gains: Option<&BayerGainDesc>,
) -> Result<(Option<BayerLevelDesc>,)> {
    if let (Some(levels), Some(gains)) = (levels, gains) {
        if levels.phase != gains.phase || levels.interpretation != gains.source {
            return Err(Error::Contract(
                "levels and gains describe different sensor coordinates".into(),
            ));
        }
        return Ok((Some(BayerLevelDesc {
            phase: levels.phase,
            interpretation: gains.target.clone(),
        }),));
    }
    Ok((None,))
}
/// Apply BayerGains to BayerLevels in matching response coordinates and CFA phase.
/// Output is BayerLevels in the gains' target interpretation. Use the same gains
/// as the Bayer image to keep its clipping levels in matching coordinates.
#[crate::node(id="sensor.clipping-level-gains", name="Apply clipping-level gains", category="Sensor", contract=gain_contract)]
pub fn compute(
    _: &KernelContext<'_>,
    _: &(),
    levels: Read<'_, Cpu<BayerLevels>>,
    gains: Read<'_, Cpu<BayerGains>>,
    output: Write<'_, Cpu<BayerLevels>>,
) -> Result<()> {
    for i in 0..4 {
        output.data[i] = levels.data[i] * gains.data[i];
    }
    Ok(())
}
