use crate::image::{CameraRgb, ColorImage};
use crate::node::KernelError;
use crate::ports::Either;

/// Plot channel values of a ColorImage or CameraRgb by image column.
///
/// Levels are log2(x), with 0 EV at x = 1. Brightness indicates sample count.
/// Produces a channel waveform view.
#[crate::node(kind = WAVEFORM, id = "view.waveform", category = "view", name = "Waveform", outputs = [])]
fn waveform(
    #[params] p: super::ExposureSettings,
    image: Either<&ColorImage, &CameraRgb>,
) -> Result<(), KernelError>;

/// Plot CIE u'v' chromaticity of a CPU ColorImage.
///
/// The frontend must require additive Rec.2020/D65 coordinates before computing
/// XYZ. The plot concerns represented colors, not original-scene measurements.
///
/// Markers show the Rec.2020 primaries. Black is omitted. Negative channels are
/// clipped for this view only. Produces a chromaticity scope view.
#[crate::node(kind = VECTORSCOPE, id = "view.vectorscope", category = "view", name = "Vectorscope", outputs = [], references = [("CIE: u’v’ chromaticity", "https://cie.co.at/eilvterm/17-23-073")])]
fn vectorscope(image: &ColorImage) -> Result<(), KernelError>;
