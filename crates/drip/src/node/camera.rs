//! Calibration derived from decoded capture facts. Each consumer derives what it
//! applies, so metadata stays a record of what the decoder reported.
//!
//! Bayer site gains apply the as-shot balance and record it in the Bayer
//! response, and it stays in place through highlights and demosaic. Camera
//! conversion then replaces it with D65 reference gains inside its
//! [`matrix`], and Chromatic adaptation maps the as-shot camera neutral
//! through the same matrix for its source white ([`as_shot_white`]).
use crate::{
    Error, Result,
    node::{
        color::{self, Mat3},
        data::BayerPhase,
    },
};
use drip_raw::{Calibration, Metadata};

/// The Bayer phase of the sensor's local 2×2 color pattern.
pub fn phase(m: &Metadata) -> Result<BayerPhase> {
    let pattern = m.calibration.cfa.map(|r| r.map(|c| if c == 3 { 1 } else { c }));
    Ok(match pattern {
        [[0, 1], [1, 2]] => BayerPhase::Rggb,
        [[1, 0], [2, 1]] => BayerPhase::Grbg,
        [[1, 2], [0, 1]] => BayerPhase::Gbrg,
        [[2, 1], [1, 0]] => BayerPhase::Bggr,
        _ => return Err(Error::Contract("capture is not a supported Bayer pattern".into())),
    })
}

/// As-shot white balance per LibRaw color index, relative to green. LibRaw uses
/// zero for an unspecified second green; that site uses the first green's gain.
/// This decoder convention does not choose or infer a white balance.
pub fn as_shot_balance(m: &Metadata) -> Result<[f32; 4]> {
    let mut wb = m.calibration.as_shot;
    if wb[3] <= 0.0 {
        wb[3] = wb[1];
    }
    let balance = wb.map(|v| v / wb[1]);
    if !balance.iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err(Error::Contract("capture has no usable as-shot white balance".into()));
    }
    Ok(balance)
}

/// Per-color values arranged by local Bayer site (row-major 2×2).
pub fn sites(m: &Metadata, per_color: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| per_color[m.calibration.cfa[i / 2][i % 2] as usize])
}

pub fn black_at(c: &Calibration, row: usize, col: usize) -> u64 {
    u64::from(c.black)
        + u64::from(c.channel_black[c.cfa[row % 2][col % 2] as usize])
        + u64::from(c.pattern.at(row, col))
}

/// The common black subtracted from every sample, so normalization introduces
/// no hidden per-channel balance. Repeating patterns larger than one Bayer cell
/// use the smallest component of each term.
pub fn black_reference(c: &Calibration) -> u64 {
    if (1..=2).contains(&c.pattern.height) && (1..=2).contains(&c.pattern.width) {
        (0..4).map(|i| black_at(c, i / 2, i % 2)).min().unwrap()
    } else {
        u64::from(c.black)
            + u64::from(*c.channel_black.iter().min().unwrap())
            + u64::from(c.pattern.values.iter().copied().min().unwrap_or(0))
    }
}

/// The normalization denominator: white minus the black reference.
pub fn white_range(c: &Calibration) -> Result<f32> {
    let black = black_reference(c);
    if u64::from(c.maximum) <= black {
        return Err(Error::Contract("white reference is not above black".into()));
    }
    Ok((u64::from(c.maximum) - black) as f32)
}

/// Clipping level of each local site in normalized sample coordinates. This
/// four-site approximation uses the first Bayer cell; larger black grids would
/// need a spatial level field to describe clipping exactly.
pub fn site_levels(m: &Metadata) -> Result<[f32; 4]> {
    let c = &m.calibration;
    let range = white_range(c)?;
    Ok(std::array::from_fn(|i| (c.maximum as f32 - black_at(c, i / 2, i % 2) as f32) / range))
}

/// Camera RGB with the given balance to linear Rec.2020/D65. Reference (D65)
/// gains replace the applied balance inside the matrix, following darktable's
/// “as shot to reference” placement (`THIRD_PARTY.md` \[22\]). Camera neutral
/// under the reference white maps to RGB white. Unbalanced responses use unit
/// balance.
pub fn matrix(m: &Metadata, balance: Option<[f32; 4]>) -> Result<Mat3> {
    let xyz_to_cam = color::to_f64(&std::array::from_fn(|i| m.calibration.xyz_to_cam[i]));
    let rgb_to_xyz = color::rgb_to_xyz(color::REC2020, color::D65);
    let reference = color::apply(&xyz_to_cam, color::apply(&rgb_to_xyz, [1.; 3]));
    let balance = balance.unwrap_or([1.; 4]);
    let unusable = || Error::Contract("capture has no usable camera color matrix".into());
    if reference.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(unusable());
    }
    let correction: [f64; 3] =
        std::array::from_fn(|c| reference[1] / reference[c] / f64::from(balance[c]));
    let matrix = color::camera_to_rgb(&xyz_to_cam, &rgb_to_xyz)
        .ok_or_else(unusable)?
        .map(|row| std::array::from_fn(|c| row[c] * correction[c]));
    if !matrix.iter().flatten().all(|v| (*v as f32).is_finite()) {
        return Err(unusable());
    }
    Ok(matrix)
}

/// CIE 1931 xy of the scene white implied by the as-shot balance: as-shot
/// camera neutral mapped through the reference matrix.
pub fn as_shot_white(m: &Metadata) -> Result<[f64; 2]> {
    let matrix = matrix(m, Some(as_shot_balance(m)?))?;
    let rgb_to_xyz = color::rgb_to_xyz(color::REC2020, color::D65);
    let white = color::apply(&rgb_to_xyz, color::apply(&matrix, [1.; 3]));
    let sum: f64 = white.iter().sum();
    if !white[1].is_finite() || white[1] <= 0.0 || !sum.is_finite() || sum <= 0.0 {
        return Err(Error::Contract("camera matrix has no usable as-shot white".into()));
    }
    Ok([white[0] / sum, white[1] / sum])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_cancels_applied_balance() {
        // An ideal XYZ camera recovers the original tristimulus values,
        // whatever balance conditioned Bayer processing.
        let mut m = Metadata::default();
        m.calibration.xyz_to_cam = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]];
        let rgb_to_xyz = color::rgb_to_xyz(color::REC2020, color::D65);
        let native = [0.2, 0.3, 0.4];
        for balance in [[1., 1., 1., 1.], [2., 1., 4., 1.], [2., 1., 1.5, 1.]] {
            let balanced: [f64; 3] = std::array::from_fn(|c| native[c] * f64::from(balance[c]));
            let matrix = matrix(&m, Some(balance)).unwrap();
            let recovered = color::apply(&rgb_to_xyz, color::apply(&matrix, balanced));
            for (actual, expected) in recovered.into_iter().zip(native) {
                assert!((actual - expected).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn levels_and_balance_follow_sites() {
        let mut m = Metadata::default();
        m.calibration.cfa = [[0, 1], [3, 2]];
        m.calibration.black = 100;
        m.calibration.channel_black = [10, 20, 30, 40];
        m.calibration.pattern = drip_raw::BlackPattern { width: 2, height: 1, values: vec![5, 7] };
        m.calibration.maximum = 1100;
        m.calibration.as_shot = [2., 1., 1.5, 0.];
        assert_eq!(phase(&m).unwrap(), BayerPhase::Rggb);
        assert_eq!(sites(&m, as_shot_balance(&m).unwrap()), [2., 1., 1., 1.5]);
        assert_eq!(site_levels(&m).unwrap(), [1., 973. / 985., 955. / 985., 963. / 985.]);
        m.calibration.as_shot = [0.; 4];
        assert!(as_shot_balance(&m).is_err());
    }
}
