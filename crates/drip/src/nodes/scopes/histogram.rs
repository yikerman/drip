use crate::image::{CameraRgb, ColorImage};
use crate::node::KernelError;
use crate::ports::Either;

/// Count each channel of a ColorImage or CameraRgb in log2(x) bins,
/// with 0 EV at x = 1.
///
/// Out-of-range values accumulate in the end bins. Scale selects linear or logarithmic
/// count display. Produces a channel histogram view.
#[crate::node(kind = HISTOGRAM, id = "view.histogram", category = "view", name = "Histogram", outputs = [])]
fn histogram(
    #[params] p: super::ExposureSettings,
    image: Either<&ColorImage, &CameraRgb>,
) -> Result<(), KernelError>;
