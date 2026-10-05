//! Built-in ICC output profiles, so the common cases need no
//! profile file. Each is a matrix/shaper profile on D65 primaries.

use lcms2::{CIExyY, CIExyYTRIPLE, Locale, MLU, Profile, Tag, TagSignature, ToneCurve};

use crate::color::{D65, P3, REC709, REC2020};

/// Constructs the built-in profile selected by the export parameter schema.
pub fn built_in(name: &str) -> Profile {
    match name {
        "srgb" => rgb("sRGB", REC709, srgb_curve()),
        "display_p3" => rgb("Display P3", P3, srgb_curve()),
        "rec2020" => rgb("Rec. 2020", REC2020, rec2020_curve()),
        _ => unreachable!("parameter schemas only admit built-in names"),
    }
}

/// The working space: Rec.2020 primaries, D65, linear.
pub fn rec2020_linear() -> Profile {
    rgb("Rec. 2020 linear", REC2020, ToneCurve::new(1.0))
}

/// The sRGB transfer function \[1\] as an ICC parametric curve of type 4:
/// `Y = (aX + b)^g` for `X ≥ d`, else `cX`.
///
/// \[1\] IEC, "Multimedia systems and equipment - Colour measurement and
///     management - Part 2-1: Colour management - Default RGB colour space
///     - sRGB," IEC 61966-2-1:1999, 1999.
fn srgb_curve() -> ToneCurve {
    ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
        .expect("valid curve")
}

/// The inverse of the BT.2020 opto-electronic transfer function \[2\], with its
/// 10-bit constants α = 1.099 and β = 0.018 (so the linear segment ends at the
/// signal level 4.5β = 0.081), as type 4.
///
/// \[2\] ITU-R, "Parameter values for ultra-high definition television systems
///     for production and international programme exchange," Rec. ITU-R
///     BT.2020-2, Oct. 2015.
fn rec2020_curve() -> ToneCurve {
    ToneCurve::new_parametric(4, &[1.0 / 0.45, 1.0 / 1.099, 0.099 / 1.099, 1.0 / 4.5, 0.081])
        .expect("valid curve")
}

fn rgb(description: &str, primaries: [[f64; 2]; 3], curve: ToneCurve) -> Profile {
    let xy = |[x, y]: [f64; 2]| CIExyY { x, y, Y: 1.0 };
    let primaries =
        CIExyYTRIPLE { Red: xy(primaries[0]), Green: xy(primaries[1]), Blue: xy(primaries[2]) };
    let mut profile =
        Profile::new_rgb(&xy(D65), &primaries, &[&curve; 3]).expect("valid built-in profile");
    let mut text = MLU::new(1);
    text.set_text(description, Locale::none());
    profile.write_tag(TagSignature::ProfileDescriptionTag, Tag::MLU(&text));
    profile
}
