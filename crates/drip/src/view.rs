//! Headless presentation results; frontends prepare their own drawing resources.

use crate::image::{Rec2020, Rgb, RgbIn};
use std::sync::Arc;

/// What a node presents to frontends besides its ports.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Image(PreviewImage),
    Histogram(Arc<Histogram>),
    Scope(Arc<Scope>),
}

/// Pixel counts per channel over equal steps of log2 value (stops), which
/// suits linear data. Values at or below `2^min_stop`, zero and negative
/// included, fall in the first bin; values at or above `2^max_stop` in the last.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub min_stop: f32,
    pub max_stop: f32,
    pub counts: Vec<[u32; 3]>,
    /// Whether frontends plot the counts on a log scale rather than linearly.
    pub log: bool,
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

/// Linear Rec.2020 pixels for the preview shader, with reference semantics
/// erased only at this presentation boundary. Cannot be used as an export input.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewImage {
    data: Arc<Rgb>,
}

impl PreviewImage {
    pub fn new(image: &dyn RgbIn<Rec2020>) -> Self {
        Self { data: image.rgb().clone() }
    }
    pub fn rgb(&self) -> &Arc<Rgb> {
        &self.data
    }
}
