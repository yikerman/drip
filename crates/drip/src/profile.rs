//! ICC output configuration shared by export and preview proofing.
//! Built-in profiles are matrix/shaper profiles on D65 primaries.

use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, Flags, Intent, Locale, MLU, PixelFormat, Profile,
    ProfileClassSignature, Tag, TagSignature, ToneCurve, Transform,
};

use crate::color::{D65, P3, REC709, REC2020};
use crate::node::KernelError;
use crate::param::{ParamKind, ParamSpec, Params};

pub(crate) const PROFILE: ParamSpec = ParamSpec::new(
    "profile",
    ParamKind::Choice { options: &["srgb", "display_p3", "rec2020", "file"], default: "srgb" },
);
pub(crate) const PROFILE_FILE: ParamSpec =
    ParamSpec::new("profile_file", ParamKind::Path { output: false });
pub(crate) const INTENT: ParamSpec = ParamSpec::new(
    "intent",
    ParamKind::Choice {
        options: &["perceptual", "relative", "saturation", "absolute"],
        default: "relative",
    },
);
pub(crate) const BLACK_POINT_COMPENSATION: ParamSpec =
    ParamSpec::new("black_point_compensation", ParamKind::Bool { default: true });

/// Loaded output settings, retaining the original ICC bytes for embedding.
pub(crate) struct Output {
    pub profile: Profile,
    pub icc: Vec<u8>,
    pub intent: Intent,
    pub flags: Flags,
}

impl Output {
    pub fn load(p: Params<'_>) -> Result<Self, KernelError> {
        let (profile, icc) = match p.choice("profile") {
            "file" => {
                let file = p
                    .path("profile_file")
                    .ok_or(KernelError::Incomplete("no output profile file chosen"))?;
                let in_file = |e| format!("{}: {e}", file.display());
                let icc = std::fs::read(file).map_err(|e| in_file(e.to_string()))?;
                (output_profile(&icc).map_err(in_file)?, icc)
            }
            name => {
                let profile = built_in(name);
                let icc = profile.icc().map_err(|e| e.to_string())?;
                (profile, icc)
            }
        };
        let intent = match p.choice("intent") {
            "perceptual" => Intent::Perceptual,
            "relative" => Intent::RelativeColorimetric,
            "saturation" => Intent::Saturation,
            _ => Intent::AbsoluteColorimetric,
        };
        let flags = if p.bool("black_point_compensation") {
            Flags::BLACKPOINT_COMPENSATION
        } else {
            Flags::default()
        };
        Ok(Self { profile, icc, intent, flags })
    }

    /// Converts linear Rec.2020 to the selected device encoding.
    pub fn convert<O: lcms2::Pod + Default>(
        &self,
        format: PixelFormat,
        pixels: &[[f32; 3]],
    ) -> Result<Vec<[O; 3]>, String> {
        let transform = Transform::new_flags(
            &rec2020_linear(),
            PixelFormat::RGB_FLT,
            &self.profile,
            format,
            self.intent,
            self.flags,
        )
        .map_err(|e| e.to_string())?;
        let mut result = vec![[O::default(); 3]; pixels.len()];
        transform.transform_pixels(pixels, &mut result);
        Ok(result)
    }
}

/// Constructs the built-in profile selected by the output parameter schema.
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

/// Parses an output profile, accepting only RGB profiles that can be a
/// destination: display, output and color space classes.
fn output_profile(icc: &[u8]) -> Result<Profile, String> {
    let profile = Profile::new_icc(icc).map_err(|e| format!("not an ICC profile ({e})"))?;
    let class = profile.device_class();
    let usable = [
        ProfileClassSignature::DisplayClass,
        ProfileClassSignature::OutputClass,
        ProfileClassSignature::ColorSpaceClass,
    ];
    if profile.color_space() != ColorSpaceSignature::RgbData || !usable.contains(&class) {
        return Err(format!("not an RGB output profile ({:?}, {class:?})", profile.color_space()));
    }
    Ok(profile)
}
