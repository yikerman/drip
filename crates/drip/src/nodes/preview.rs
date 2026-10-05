//! Image presentation without processing side effects.

use crate::image::{Rec2020, RgbIn};
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind};
use crate::param::Params;
use crate::ports::Read;
use crate::view::{PreviewImage, View};

/// Shows a Rec.2020 image; the frontend handles the display transform.
pub static PREVIEW: NodeKind =
    NodeKind::new::<Preview>("view.preview", "preview", &[], &["image"], &[]);

struct Preview;
impl NodeKernel for Preview {
    type Inputs = (Read<dyn RgbIn<Rec2020>>,);
    type Outputs = ();
    fn eval(
        _: Params<'_>,
        (image,): (&dyn RgbIn<Rec2020>,),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<Self::Outputs>, String> {
        Ok(Evaluated { outputs: (), view: Some(View::Image(PreviewImage::new(image))) })
    }
}
