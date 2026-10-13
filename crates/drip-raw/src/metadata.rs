//! Owned common LibRaw fields. Numeric sentinels retain LibRaw's meaning;
//! absent GPS is explicit, and decoded text is UTF-8. No decoder pointers escape.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Metadata {
    pub make: String,
    pub model: String,
    pub iso: f32,
    /// Seconds.
    pub shutter: f32,
    pub aperture: f32,
    /// Millimeters.
    pub focal_length: f32,
    pub timestamp: i64,
    /// Camera wall-clock time, without an inferred time zone.
    pub datetime: String,
    /// EXIF 1–8; unspecified orientation becomes identity (1).
    pub orientation: u8,
    pub artist: String,
    pub description: String,
    pub shot_order: u32,
    pub body_serial: String,
    pub internal_body_serial: String,
    /// LibRaw drive, focus, metering, AF point, exposure mode/program, stabilization.
    pub shooting_modes: [i32; 7],
    pub sensor: Sensor,
    pub calibration: Calibration,
    pub lens: Lens,
    pub gps: Option<Gps>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            make: String::new(),
            model: String::new(),
            iso: 0.0,
            shutter: 0.0,
            aperture: 0.0,
            focal_length: 0.0,
            timestamp: 0,
            datetime: String::new(),
            orientation: 1,
            artist: String::new(),
            description: String::new(),
            shot_order: 0,
            body_serial: String::new(),
            internal_body_serial: String::new(),
            shooting_modes: [0; 7],
            sensor: Sensor::default(),
            calibration: Calibration::default(),
            lens: Lens::default(),
            gps: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Sensor {
    /// Visible sensor area copied into the decoded pixel buffer.
    pub width: usize,
    pub height: usize,
    pub raw_width: u32,
    pub raw_height: u32,
    pub left_margin: u32,
    pub top_margin: u32,
    pub pixel_aspect: f64,
    /// LibRaw inset crops: left, top, width, height; unknown origins are 0xffff.
    pub inset_crops: [[u32; 4]; 2],
    /// Masked sensor rectangles: top, left, bottom, right.
    pub masked_areas: [[i32; 4]; 8],
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Calibration {
    /// Local Bayer sites: 0 R, 1 G, 2 B, 3 second G.
    pub cfa: [[u8; 2]; 2],
    /// Black = common + channel_black[CFA] + repeating pattern.
    pub black: u32,
    pub channel_black: [u32; 4],
    pub pattern: BlackPattern,
    pub maximum: u32,
    pub linear_max: [u32; 4],
    /// LibRaw cam_xyz: XYZ to camera RGB.
    pub xyz_to_cam: [[f32; 3]; 4],
    pub as_shot: [f32; 4],
    /// LibRaw pre_mul; kept separately from as-shot gains.
    pub reference_gains: [f32; 4],
    pub analog_balance: [f32; 4],
    pub rgb_cam: [[f32; 4]; 3],
    pub camera_matrix: [[f32; 4]; 3],
    pub dng_color: [DngColor; 2],
    pub dng_white_level: [u32; 4],
    pub dng_as_shot_neutral: [f32; 4],
    pub dng_default_crop: [u32; 4],
    pub dng_user_crop: [f32; 4],
    pub baseline_exposure: f32,
}

/// Local C shim layout, filled by field name from LibRaw's DNG records.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct DngColor {
    pub parsed_fields: u32,
    pub illuminant: u32,
    pub calibration: [[f32; 4]; 4],
    pub color_matrix: [[f32; 3]; 4],
    pub forward_matrix: [[f32; 4]; 3],
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct BlackPattern {
    pub height: usize,
    pub width: usize,
    pub values: Vec<u32>,
}
impl BlackPattern {
    pub fn at(&self, row: usize, col: usize) -> u32 {
        if self.values.is_empty() {
            0
        } else {
            self.values[row % self.height * self.width + col % self.width]
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Lens {
    pub make: String,
    pub model: String,
    pub serial: String,
    pub internal_serial: String,
    /// Vendor-specific identifier; not globally unique. LibRaw sentinels retained.
    pub id: u64,
    pub maker_note_model: String,
    pub min_focal: f32,
    pub max_focal: f32,
    pub max_aperture_at_min_focal: f32,
    pub max_aperture_at_max_focal: f32,
    pub focal_length_35mm: f32,
    /// Minimum focusing capability, NOT the focus distance of this exposure.
    pub min_focus_distance: f32,
    pub lens_mount: u32,
    pub camera_mount: u32,
    pub lens_format: u32,
    pub camera_format: u32,
    pub teleconverter: String,
    pub teleconverter_id: u64,
    pub adapter: String,
    pub adapter_id: u64,
    pub attachment: String,
    pub attachment_id: u64,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Gps {
    /// Degrees, minutes, seconds; hemisphere comes from the reference byte.
    pub latitude: [f32; 3],
    pub longitude: [f32; 3],
    /// GPS hours, minutes, seconds.
    pub timestamp: [f32; 3],
    pub altitude: f32,
    pub latitude_ref: u8,
    pub longitude_ref: u8,
    pub altitude_ref: u8,
    pub status: u8,
}
