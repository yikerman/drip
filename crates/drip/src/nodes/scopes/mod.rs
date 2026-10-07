//! Diagnostic scopes and their shared exposure settings.

mod density;
mod histogram;

pub use density::{VECTORSCOPE, WAVEFORM, vectorscope_xyz};
pub use histogram::HISTOGRAM;

use crate::param::ParamKind;

#[derive(Clone, Copy, crate::Choice)]
enum Scale {
    #[choice("linear")]
    Linear,
    #[choice("log")]
    Log,
}

impl Scale {
    fn logarithmic(self) -> bool {
        match self {
            Self::Linear => false,
            Self::Log => true,
        }
    }
}

#[derive(crate::Parameters)]
pub struct ExposureSettings {
    #[param(ParamKind::Int { min: -24, max: -1, default: -12 })]
    min_ev: i64,
    #[param(ParamKind::Int { min: 1, max: 10, default: 4 })]
    max_ev: i64,
    #[param(Scale::Linear.schema())]
    scale: Scale,
}
