//! Diagnostic scopes and their shared exposure settings.

mod density;
mod histogram;

pub use density::{VECTORSCOPE, WAVEFORM, vectorscope_xyz};
pub use histogram::HISTOGRAM;

use crate::param::{ParamKind, ParamSpec};

const EXPOSURE_PARAMS: &[ParamSpec] = &[
    ParamSpec::new("min_ev", ParamKind::Int { min: -24, max: -1, default: -12 }),
    ParamSpec::new("max_ev", ParamKind::Int { min: 1, max: 10, default: 4 }),
    ParamSpec::new("scale", ParamKind::Choice { options: &["linear", "log"], default: "linear" }),
];
