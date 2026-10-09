//! Shared exposure settings, reductions and plotting for scope nodes.
use drip::{Error, Result, node::data::*, param::ParamKind};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
pub(super) mod histogram;
mod view;
pub(super) mod waveform;
pub use super::vectorscope::vectorscope_xyz;
pub use histogram::Histogram;
#[cfg(test)]
mod tests;
#[derive(Clone, Copy, Default, drip::Choice)]
pub enum Scale {
    #[default]
    #[choice("linear")]
    Linear,
    #[choice("log")]
    Log,
}
#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
pub struct ExposureSettings {
    #[param(ParamKind::Int { min: -24, max: -1, default: -12 })]
    pub min_ev: i64,
    #[param(ParamKind::Int { min: 1, max: 10, default: 4 })]
    pub max_ev: i64,
    #[param(Scale::Linear.schema())]
    pub scale: Scale,
}
impl Default for ExposureSettings {
    fn default() -> Self {
        Self { min_ev: -12, max_ev: 4, scale: Scale::Linear }
    }
}
pub(super) fn scope_contract(
    _: &drip::runtime::GlobalContext,
    p: &ExposureSettings,
    image: Option<&ImageDesc<Color>>,
) -> Result<()> {
    if image.is_some_and(|d| d.interpretation.encoding != Encoding::Identity) {
        return Err(Error::Contract("channel exposure requires identity encoding".into()));
    }
    exposure_range(p)
}
pub(super) fn camera_scope_contract(
    _: &drip::runtime::GlobalContext,
    p: &ExposureSettings,
    _: Option<&ImageDesc<Camera>>,
) -> Result<()> {
    exposure_range(p)
}
fn exposure_range(p: &ExposureSettings) -> Result<()> {
    if p.min_ev >= p.max_ev || p.min_ev < -126 || p.max_ev > 127 {
        return Err(Error::Contract("exposure range needs finite ordered f32 thresholds".into()));
    }
    Ok(())
}
/// Row-major density bins, top to bottom. Waveforms use RGB counts;
/// chromaticity uses the first channel only.
#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub size: usize,
    pub counts: Vec<[u32; 3]>,
    pub axes: ScopeAxes,
    pub log: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScopeAxes {
    Waveform {
        min_stop: f32,
        max_stop: f32,
    },
    /// Source primary markers in normalized plot coordinates; D65 is centered.
    Vectorscope {
        primaries: [[f32; 2]; 3],
        color_space: &'static str,
    },
}

pub(super) const SIZE: usize = 256;
pub(super) fn scope(counts: Vec<[u32; 3]>, axes: ScopeAxes, log: bool) -> Arc<Scope> {
    Arc::new(Scope { size: SIZE, counts, axes, log })
}

fn logarithmic(scale: Scale) -> bool {
    matches!(scale, Scale::Log)
}
