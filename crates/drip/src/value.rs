//! Values flowing along graph edges and the semantic port types that classify
//! them. Types are semantic rather than structural: scene- and display-referred
//! Rec.2020 share a layout but are distinct, so the graph can refuse to export
//! an image that has not been tone mapped.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortType {
    /// Scene-referred linear Rec.2020.
    SceneRec2020,
    /// Display-referred linear Rec.2020, nominally within [0, 1].
    DisplayRec2020,
}

/// A port value. Payloads are immutable and shared, so cloning is cheap and
/// cached results can be handed to any number of consumers or threads.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    SceneRec2020(Arc<Rgb>),
    DisplayRec2020(Arc<Rgb>),
}

impl Value {
    pub fn port_type(&self) -> PortType {
        match self {
            Value::SceneRec2020(_) => PortType::SceneRec2020,
            Value::DisplayRec2020(_) => PortType::DisplayRec2020,
        }
    }

    /// The image of any RGB-typed value.
    pub fn rgb(&self) -> &Arc<Rgb> {
        match self {
            Value::SceneRec2020(image) | Value::DisplayRec2020(image) => image,
        }
    }
}

/// Interleaved 3-channel f32 image, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[f32; 3]>,
}

/// What a node presents to frontends besides its ports (DESIGN U1).
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Image(Value),
}
