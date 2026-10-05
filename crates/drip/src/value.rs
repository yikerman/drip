//! Values flowing along graph edges and the semantic port types that classify
//! them. Types are semantic rather than structural: scene- and display-referred
//! Rec.2020 share a layout but are distinct, so the graph can refuse to export
//! an image that has not been tone mapped.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub use drip_libraw::Metadata as RawMetadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortType {
    /// Normalized sensor data behind a color filter array.
    Mosaic,
    /// RGB in the camera's own primaries.
    CameraRgb,
    /// Scene-referred linear Rec.2020.
    SceneRec2020,
    /// Display-referred linear Rec.2020, nominally within [0, 1].
    DisplayRec2020,
    RawMetadata,
}

/// A port value. Payloads are immutable and shared, so cloning is cheap and
/// cached results can be handed to any number of consumers or threads.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Mosaic(Arc<Mosaic>),
    CameraRgb(Arc<Rgb>, Arc<Camera>),
    SceneRec2020(Arc<Rgb>),
    DisplayRec2020(Arc<Rgb>),
    RawMetadata(Arc<RawMetadata>),
}

impl Value {
    pub fn port_type(&self) -> PortType {
        match self {
            Value::Mosaic(_) => PortType::Mosaic,
            Value::CameraRgb(..) => PortType::CameraRgb,
            Value::SceneRec2020(_) => PortType::SceneRec2020,
            Value::DisplayRec2020(_) => PortType::DisplayRec2020,
            Value::RawMetadata(_) => PortType::RawMetadata,
        }
    }

    /// The image of an RGB-typed value.
    pub fn rgb(&self) -> &Arc<Rgb> {
        match self {
            Value::CameraRgb(image, _)
            | Value::SceneRec2020(image)
            | Value::DisplayRec2020(image) => image,
            Value::Mosaic(_) | Value::RawMetadata(_) => {
                unreachable!("ports guarantee an RGB value")
            }
        }
    }

    pub fn mosaic(&self) -> &Arc<Mosaic> {
        let Value::Mosaic(mosaic) = self else { unreachable!("ports guarantee a mosaic") };
        mosaic
    }
}

/// Interleaved 3-channel f32 image, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub width: usize,
    pub height: usize,
    /// Sensor pixels per image pixel along each axis.
    pub scale: u32,
    pub pixels: Vec<[f32; 3]>,
}

impl Rgb {
    /// The same geometry with `f` applied to every pixel.
    pub fn map(&self, f: impl Fn([f32; 3]) -> [f32; 3]) -> Rgb {
        Rgb { pixels: self.pixels.iter().map(|&p| f(p)).collect(), ..*self }
    }
}

/// One color sample per site, normalized so that black is 0 and sensor
/// saturation is 1. Values outside [0, 1] are kept, not clipped.
#[derive(Debug, Clone, PartialEq)]
pub struct Mosaic {
    pub width: usize,
    pub height: usize,
    pub scale: u32,
    pub cfa: Cfa,
    /// Conservative saturation per CFA color in the same units as `data`.
    /// White balance scales these with the samples; highlight detection must
    /// not assume a balanced channel still clips at 1.
    pub white: [f32; 4],
    pub data: Vec<f32>,
    pub camera: Arc<Camera>,
}

/// A color filter array repeating every `size` sites in both directions;
/// colors are 0 R, 1 G, 2 B and 3 for a Bayer cell's second G, which some
/// cameras balance separately.
#[derive(Debug, Clone, PartialEq)]
pub struct Cfa {
    pub size: usize,
    pub colors: Vec<u8>,
}

impl Cfa {
    pub fn color(&self, row: usize, col: usize) -> u8 {
        self.colors[row % self.size * self.size + col % self.size]
    }
}

/// How to interpret a camera's RGB (DESIGN P3).
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// CIE XYZ (D65) to camera RGB.
    pub xyz_to_cam: [[f32; 3]; 3],
    /// As-shot white balance multipliers per CFA color, with G = 1.
    pub white_balance: [f32; 4],
}

/// What a node presents to frontends besides its ports (DESIGN U1).
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Image(Value),
    Histogram(Arc<Histogram>),
    Scope(Arc<Scope>),
}

/// Pixel counts per channel over equal steps of log2 value (stops), which
/// suits linear data. Values at or below `2^min_stop`, zero and negative
/// included, fall in the first bin; values at or above `2^max_stop` in the last.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub min_stop: f32,
    pub max_stop: f32,
    pub counts: Vec<[u32; 3]>,
    /// Whether frontends plot the counts on a log scale rather than linearly.
    pub log: bool,
}

/// Row-major density bins, top to bottom. Waveforms use RGB counts;
/// chromaticity uses the first channel only.
#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub size: usize,
    pub counts: Vec<[u32; 3]>,
    pub axes: ScopeAxes,
    pub log: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScopeAxes {
    Waveform {
        min_stop: f32,
        max_stop: f32,
    },
    /// Rec.2020 primary markers in normalized plot coordinates; D65 is centered.
    Vectorscope {
        primaries: [[f32; 2]; 3],
    },
}
