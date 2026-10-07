use crate::image::{CameraRgb, Rec2020Mat};
use crate::node::{EvalContext, KernelError};
use crate::ports::Either;

/// Count each channel of a linear input, camera RGB included, in log2(x) bins,
/// with 0 EV at x = 1.
///
/// Out-of-range values accumulate in the end bins. Scale selects linear or logarithmic
/// count display. Produces a channel histogram view.
#[crate::node(kind = HISTOGRAM, id = "view.histogram", category = "view", name = "Histogram", outputs = [])]
fn histogram(
    p: super::ExposureSettings,
    (image,): (Either<&Rec2020Mat, &CameraRgb>,),
    _: &EvalContext<'_>,
) -> Result<(), KernelError>;
