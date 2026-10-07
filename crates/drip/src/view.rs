//! Headless presentation results; frontends prepare their own drawing resources.

use crate::image::{RealMat, Rec2020Rgb, Rgb};
use std::sync::Arc;

mod sealed {
    pub trait Presentation {}
    impl Presentation for () {}
    impl Presentation for super::View {}
    impl Presentation for super::PreviewImage {}
    impl Presentation for std::sync::Arc<super::Histogram> {}
    impl Presentation for std::sync::Arc<super::Scope> {}
    impl<T: super::Presentation> Presentation for Option<T> {}
}

/// Supported headless presentation results. Sealed so view availability and
/// conversion cannot disagree through a user-supplied implementation.
pub trait Presentation: sealed::Presentation {
    const HAS_VIEW: bool;
    fn into_view(self) -> Option<View>;
}
impl Presentation for () {
    const HAS_VIEW: bool = false;
    fn into_view(self) -> Option<View> {
        None
    }
}
impl Presentation for View {
    const HAS_VIEW: bool = true;
    fn into_view(self) -> Option<View> {
        Some(self)
    }
}
impl Presentation for PreviewImage {
    const HAS_VIEW: bool = true;
    fn into_view(self) -> Option<View> {
        Some(View::Image(self))
    }
}
impl Presentation for Arc<Histogram> {
    const HAS_VIEW: bool = true;
    fn into_view(self) -> Option<View> {
        Some(View::Histogram(self))
    }
}
impl Presentation for Arc<Scope> {
    const HAS_VIEW: bool = true;
    fn into_view(self) -> Option<View> {
        Some(View::Scope(self))
    }
}
impl<T: Presentation> Presentation for Option<T> {
    const HAS_VIEW: bool = T::HAS_VIEW;
    fn into_view(self) -> Option<View> {
        self.and_then(Presentation::into_view)
    }
}

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
    /// Bilinear display sampling when true, nearest-neighbor otherwise.
    pub interpolation: bool,
    data: Arc<Rgb>,
}

impl PreviewImage {
    pub fn new<I: Rec2020Rgb>(image: &RealMat<3, I>) -> Self {
        Self { data: image.rgb().clone(), interpolation: false }
    }
    pub fn from_input(image: &crate::ports::MatRef<'_, 3, dyn Rec2020Rgb>) -> Self {
        Self { data: image.rgb().clone(), interpolation: false }
    }
    pub fn rgb(&self) -> &Arc<Rgb> {
        &self.data
    }
}
