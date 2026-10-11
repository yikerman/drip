//! Shared exposure settings, reductions and plotting for scope nodes.
use drip::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
pub(super) mod device;
pub(super) mod histogram;
pub(super) mod vectorscope;
mod view;
pub(super) mod waveform;
pub use histogram::Histogram;
#[cfg(test)]
pub(super) mod tests;
#[derive(Clone, Copy, drip::Choice)]
pub enum Scale {
    #[default]
    #[choice("linear")]
    Linear,
    #[choice("log")]
    #[label("Logarithmic")]
    Log,
}
/// The exposure axis shared by histogram and waveform settings.
#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(exposure_range)]
pub struct ExposureRange {
    /// Lower exposure bound in stops relative to sample value 1. Values below it enter the first bin.
    #[param(default = -12, range = -24..=-1)]
    #[label("Minimum exposure (EV)")]
    pub min_ev: i64,
    /// Upper exposure bound in stops relative to sample value 1. Values above it enter the last bin.
    #[param(default = 4, range = 1..=10)]
    #[label("Maximum exposure (EV)")]
    pub max_ev: i64,
}
fn exposure_range(p: &ExposureRange) -> Result<()> {
    if p.min_ev >= p.max_ev || p.min_ev < -126 || p.max_ev > 127 {
        return Err(Error::Contract("exposure range needs finite ordered f32 thresholds".into()));
    }
    Ok(())
}
/// Row-major density bins, top to bottom. Waveforms use RGB counts;
/// chromaticity uses the first channel only.
#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub counts: Vec<[u32; 3]>,
    pub axes: ScopeAxes,
    pub log: bool,
    pub color_space: String,
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
    },
}

pub(super) const SIZE: usize = 256;
pub(super) fn scope(
    counts: Vec<[u32; 3]>,
    axes: ScopeAxes,
    log: bool,
    color_space: String,
) -> Arc<Scope> {
    Arc::new(Scope { counts, axes, log, color_space })
}

fn logarithmic(scale: Scale) -> bool {
    matches!(scale, Scale::Log)
}
