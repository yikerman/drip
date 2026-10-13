//! Scope nodes: Histogram, Waveform and Vectorscope are bins nodes whose
//! counts the store keeps, and whose views plot the counts. Each declares a
//! `#[cube]` bin body, its only device code. The exposure axis, its bin
//! edges and plotting are shared here and in `view`.
use cubecl::prelude::*;
use drip::{
    Error, Result,
    node::data::{Counts, CountsDesc, RgbDesc},
    ports::Read,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
pub(crate) mod histogram;
pub(crate) mod vectorscope;
mod view;
pub(crate) mod waveform;
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

/// The lower edge of each of the `SIZE` bins of `range`, `2^ev` for equal
/// steps of `ev`. Bodies compare samples with these f32 values, so a sample
/// on an edge falls in the same bin on every runtime and on the host.
fn edges(range: &ExposureRange) -> [f32; SIZE] {
    let (min, max) = (range.min_ev as f32, range.max_ev as f32);
    std::array::from_fn(|i| 2f32.powf(min + i as f32 * (max - min) / SIZE as f32))
}

/// The bin of `value` among the `SIZE` ascending `edges`: the last edge at
/// or below it. Values below the first edge, and NaN, fall in bin 0.
#[cube]
fn stop(value: f32, edges: &[f32]) -> u32 {
    let mut lo = 0usize;
    let mut hi = 256usize;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if value >= edges[mid] {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    u32::cast_from(usize::max(lo, 1) - 1)
}

/// What a scope with `bins` bins of `channels` channels counts of `image`.
fn counts(bins: usize, channels: usize, image: &RgbDesc) -> CountsDesc {
    CountsDesc { bins, channels, coordinates: image.coordinates.clone() }
}

/// Counts with three channels per bin, the missing ones zero.
fn per_bin(counts: &Read<'_, Counts>) -> Result<Vec<[u32; 3]>> {
    let host = counts.data.host()?;
    let bin = |c: &[u32]| std::array::from_fn(|i| c.get(i).copied().unwrap_or(0));
    Ok(host.chunks(counts.desc.channels).map(bin).collect())
}
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
