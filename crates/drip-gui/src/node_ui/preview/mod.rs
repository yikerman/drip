//! Image preview preparation; display output remains in the renderer.

use std::sync::Arc;

use crate::node_ui::data::HostImage;
use drip::Error as KernelError;
use drip::node::profile;

use proofing::{gamutcheck, softproof};
#[cfg(test)]
pub use view::ImageView;
pub use view::PreviewImage;

#[cfg(test)]
mod export_tests;
mod proofing;
#[cfg(test)]
mod tests;
mod view;

/// `output` is the proof mode's loaded output profile.
pub fn prepare(
    p: &Preview,
    output: Option<&profile::Output>,
    image: &Arc<HostImage>,
) -> Result<PreviewImage, KernelError> {
    let output = || output.expect("proof modes load their output");
    let pixels = match p.mode {
        Mode::None => None,
        Mode::Softproof(_) => Some(softproof(output(), &image.pixels)?),
        Mode::Gamutcheck(_) => Some(gamutcheck(output(), &image.pixels)?),
    };
    let mut view = match pixels {
        None => PreviewImage::new(image),
        Some(pixels) => {
            let proof = Arc::new(HostImage { pixels: pixels.into(), ..(**image).clone() });
            PreviewImage::new(&proof)
        }
    };
    view.interpolation = p.interpolation;
    Ok(view)
}

#[drip_macros::gui_node]
impl super::GuiNode for observe::Definition {
    type Presentation = PreviewImage;
    const PREPARE: Option<super::Prepare<Self>> = Some(|node, inputs, ctx| {
        let image = Arc::new(HostImage::new(&inputs.image, ctx.global.scale));
        prepare(&node.parameters, node.assets()?.as_ref(), &image)
    });
}

use drip::{
    definition::Assets,
    node::data::{Rec2020, Rgb},
    ports::{Cpu, Read},
    runtime::KernelContext,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, drip::Choice)]
pub enum Mode {
    #[default]
    #[choice("none")]
    None,
    #[choice("softproof")]
    Softproof(profile::Settings),
    #[choice("gamutcheck")]
    #[label("Gamut check")]
    Gamutcheck(profile::Settings),
}
#[derive(Clone, Serialize, Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    /// Use bilinear display sampling when enabled, nearest-neighbor sampling otherwise.
    #[param(default = false)]
    pub interpolation: bool,
    /// Show the image directly, simulate the output profile, or mark out-of-gamut colors in cyan.
    #[label("Proof mode")]
    #[param]
    pub mode: Mode,
}
impl Assets for Preview {
    type Loaded = Option<profile::Output>;
    fn load(&self) -> drip::Result<Self::Loaded> {
        match &self.mode {
            Mode::None => Ok(None),
            Mode::Softproof(output) | Mode::Gamutcheck(output) => {
                profile::Output::load(output).map(Some)
            }
        }
    }
}
/// Display linear Rec.2020/D65 RGB. Softproof simulates an output profile; Gamut
/// check marks out-of-gamut colors in cyan. Each proof mode owns its output
/// settings; a profile file is read when chosen. Interpolation selects bilinear
/// or nearest-neighbor display sampling and does not alter processing pixels.
#[drip::node(id = "view.preview", name = "Preview", category = "View", assets)]
pub fn observe(
    _: &KernelContext<'_>,
    _: &Preview,
    _image: Read<'_, Cpu<Rgb>, Rec2020>,
) -> drip::Result<()> {
    Ok(())
}
