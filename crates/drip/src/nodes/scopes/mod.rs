//! Diagnostic scopes and their shared exposure settings.

mod density;
mod histogram;

pub use density::{VECTORSCOPE, VectorscopeNode, WAVEFORM, WaveformNode};
pub use histogram::{HISTOGRAM, HistogramNode};

use crate::param::ParamKind;

#[derive(Clone, Copy, crate::Choice)]
pub enum Scale {
    #[choice("linear")]
    Linear,
    #[choice("log")]
    Log,
}

#[derive(crate::Parameters)]
pub struct ExposureSettings {
    #[param(ParamKind::Int { min: -24, max: -1, default: -12 })]
    pub min_ev: i64,
    #[param(ParamKind::Int { min: 1, max: 10, default: 4 })]
    pub max_ev: i64,
    #[param(Scale::Linear.schema())]
    pub scale: Scale,
}
