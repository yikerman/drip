//! Native file selection for the RAW source.
use super::{FileUi, GuiNode, ParameterUi};

struct RawGui;
#[drip_macros::gui_node]
impl GuiNode for RawGui {
    type Parameters = drip::node::raw::Settings;
    type Presentation = ();
    const ID: &'static str = "input.bayer-raw";

    fn parameter_ui(_: &Self::Parameters, path: &str) -> ParameterUi {
        ParameterUi {
            file: (path == "path").then_some(FileUi {
                title: "Choose RAW file",
                filter: "Camera RAW",
                extensions: &[
                    "arw", "cr2", "cr3", "dng", "erf", "fff", "iiq", "kdc", "mef", "mos", "mrw",
                    "nef", "nrw", "orf", "pef", "raf", "raw", "rw2", "rwl", "srw", "x3f",
                ],
            }),
        }
    }
}
