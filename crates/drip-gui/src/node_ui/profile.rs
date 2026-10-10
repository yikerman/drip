//! File selection shared by export and preview profile parameters.
use super::{FileUi, ParameterUi};

pub fn parameter_ui(path: &str) -> ParameterUi {
    ParameterUi {
        file: (path == "profile.file.path" || path.ends_with(".profile.file.path")).then_some(
            FileUi {
                title: "Choose ICC profile",
                filter: "RGB ICC profile",
                extensions: &["icc", "icm"],
            },
        ),
    }
}
