use crate::image::{LinearRgb, Linearity};
use crate::node::{EvalContext, KernelError};
use crate::ports::MatRef;

/// Plot channel values of a linear input by image column.
///
/// Levels are log2(x), with 0 EV at x = 1. Brightness indicates sample count.
/// Produces a channel waveform view.
#[crate::node(kind = WAVEFORM, id = "view.waveform", category = "view", name = "Waveform", outputs = [])]
fn waveform(
    p: super::ExposureSettings,
    (image,): (MatRef<'_, 3, dyn Linearity>,),
    _: &EvalContext<'_>,
) -> Result<(), KernelError>;

/// Plot CIE u'v' chromaticity relative to D65 using the input's color space.
///
/// Markers show that color space's primaries. Black is omitted. Negative channels are
/// clipped for this view only. Produces a chromaticity scope view.
#[crate::node(kind = VECTORSCOPE, id = "view.vectorscope", category = "view", name = "Vectorscope", outputs = [], references = [("CIE: u’v’ chromaticity", "https://cie.co.at/eilvterm/17-23-073")])]
fn vectorscope(
    _: (),
    (image,): (MatRef<'_, 3, dyn LinearRgb>,),
    _: &EvalContext<'_>,
) -> Result<(), KernelError>;
