//! Colorimetry shared by nodes and export.
use crate::body::matrix::{Mat3, apply, determinant, inverse, mul, transpose};

/// CIE 1931 xy chromaticity of D65 \[1\].
pub const D65: [f64; 2] = [0.3127, 0.3290];

/// xy chromaticities of the Rec.2020 red, green and blue primaries \[1\].
pub const REC2020: [[f64; 2]; 3] = [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]];

/// xy chromaticities of the BT.709 primaries, shared by sRGB \[4\].
pub const REC709: [[f64; 2]; 3] = [[0.640, 0.330], [0.300, 0.600], [0.150, 0.060]];

/// xy chromaticities of the P3 primaries, used with D65 by Display P3 \[5\].
pub const P3: [[f64; 2]; 3] = [[0.680, 0.320], [0.265, 0.690], [0.150, 0.060]];

/// Linear RGB to CIE XYZ for the given primaries and white point, with the
/// white point at Y = 1 \[2\].
///
/// \[1\] ITU-R, "Parameter values for ultra-high definition television systems
///     for production and international programme exchange," Rec. ITU-R
///     BT.2020-2, Oct. 2015.
/// \[2\] SMPTE, "Derivation of basic television color equations," SMPTE RP
///     177-1993, 1993.
/// \[4\] ITU-R, "Parameter values for the HDTV standards for production and
///     international programme exchange," Rec. ITU-R BT.709-6, Jun. 2015.
/// \[5\] SMPTE, "D-Cinema quality - Reference projector and environment,"
///     SMPTE RP 431-2:2011, 2011.
pub fn rgb_to_xyz(primaries: [[f64; 2]; 3], white: [f64; 2]) -> Mat3 {
    let xyz = |[x, y]: [f64; 2]| [x / y, 1.0, (1.0 - x - y) / y];
    let p = transpose(primaries.map(xyz));
    let s = apply(&inverse(&p), xyz(white));
    p.map(|row| [row[0] * s[0], row[1] * s[1], row[2] * s[2]])
}

/// The dcraw camera matrix \[3\]: maps white-balanced camera RGB to linear RGB
/// with the given RGB-to-XYZ matrix, such that camera neutral (1, 1, 1) maps
/// to the RGB white. Input gains must correspond to that reference white;
/// scene-illuminant adaptation is separate.
/// `None` if the camera matrix is degenerate.
///
/// \[3\] D. Coffin, "dcraw.c," `cam_xyz_coeff()`. \[Online\]. Available:
///     <https://www.dechifro.org/dcraw/>
pub fn camera_to_rgb(xyz_to_cam: &Mat3, rgb_to_xyz: &Mat3) -> Option<Mat3> {
    let rgb_to_cam = mul(xyz_to_cam, rgb_to_xyz).map(|row| {
        let sum: f64 = row.iter().sum();
        row.map(|v| v / sum)
    });
    // Rows now sum to 1, so a usable matrix has a determinant far from 0.
    let usable =
        determinant(&rgb_to_cam).abs() > 1e-6 && rgb_to_cam.iter().flatten().all(|v| v.is_finite());
    usable.then(|| inverse(&rgb_to_cam))
}
