//! File dialogs for the library's TIFF export node.
use drip::node::export::{Export, tiff};

#[drip_macros::gui_node]
impl super::GuiNode for tiff::Definition {
    type Presentation = ();
    fn parameter_ui(_: &Export, path: &str) -> super::ParameterUi {
        if path == "path" {
            super::ParameterUi {
                file: Some(super::FileUi {
                    title: "Choose TIFF destination",
                    filter: "TIFF image",
                    extensions: &["tiff", "tif"],
                }),
            }
        } else {
            super::profile::parameter_ui(path)
        }
    }
}
