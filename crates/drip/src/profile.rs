//! ICC output configuration shared by export and preview proofing.
//! Built-in profiles are matrix/shaper profiles on D65 primaries.

use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, Flags, Intent, Locale, MLU, PixelFormat, Profile,
    ProfileClassSignature, Tag, TagSignature, ToneCurve, Transform,
};
use rayon::prelude::*;

use crate::color::{D65, P3, REC709, REC2020};
use crate::node::KernelError;
use crate::param::ParamKind;

#[derive(Clone, Copy, crate::Choice)]
pub enum ProfileSource {
    #[choice("srgb")]
    Srgb,
    #[choice("display_p3")]
    DisplayP3,
    #[choice("rec2020")]
    Rec2020,
    #[choice("file")]
    File,
}

#[derive(Clone, Copy, crate::Choice)]
enum RenderingIntent {
    #[choice("perceptual")]
    Perceptual,
    #[choice("relative")]
    Relative,
    #[choice("saturation")]
    Saturation,
    #[choice("absolute")]
    Absolute,
}

#[derive(crate::Parameters)]
pub(crate) struct Settings {
    #[param(ProfileSource::Srgb.schema())]
    profile: ProfileSource,
    #[param(ParamKind::Path { output: false })]
    profile_file: Option<std::path::PathBuf>,
    #[param(RenderingIntent::Relative.schema())]
    intent: RenderingIntent,
    #[param(ParamKind::Bool { default: true })]
    black_point_compensation: bool,
}

/// Loaded output settings, retaining the original ICC bytes for embedding.
pub(crate) struct Output {
    pub profile: Profile,
    pub icc: Vec<u8>,
    pub intent: Intent,
    pub flags: Flags,
}

impl Output {
    pub fn load(p: &Settings, resources: &crate::resource::Resources) -> Result<Self, KernelError> {
        let (profile, icc) = match p.profile.built_in() {
            None => {
                let file = p
                    .profile_file
                    .as_deref()
                    .ok_or(KernelError::Incomplete("no output profile file chosen"))?;
                let in_file = |e| format!("{}: {e}", file.display());
                let icc = resources
                    .load(file, |file| std::fs::read(file).map_err(|e| in_file(e.to_string())))?;
                let icc = (*icc).clone();
                (output_profile(&icc).map_err(in_file)?, icc)
            }
            Some(profile) => {
                let icc = profile.icc().map_err(|e| e.to_string())?;
                (profile, icc)
            }
        };
        let intent = match p.intent {
            RenderingIntent::Perceptual => Intent::Perceptual,
            RenderingIntent::Relative => Intent::RelativeColorimetric,
            RenderingIntent::Saturation => Intent::Saturation,
            RenderingIntent::Absolute => Intent::AbsoluteColorimetric,
        };
        let flags = if p.black_point_compensation {
            Flags::BLACKPOINT_COMPENSATION
        } else {
            Flags::default()
        };
        Ok(Self { profile, icc, intent, flags })
    }

    /// Converts linear Rec.2020 to the selected device encoding.
    pub fn convert<O: lcms2::Pod + Default + Send>(
        &self,
        format: PixelFormat,
        pixels: &[[f32; 3]],
    ) -> Result<Vec<[O; 3]>, String> {
        let transform = Transform::new_flags_context(
            lcms2::GlobalContext::new(),
            &rec2020_linear(),
            PixelFormat::RGB_FLT,
            &self.profile,
            format,
            self.intent,
            self.flags | Flags::NO_CACHE,
        )
        .map_err(|e| e.to_string())?;
        Ok(convert_pixels(pixels, |input, output| transform.transform_pixels(input, output)))
    }
}

// Bound per-job work while giving Rayon enough chunks to balance the workers.
const PIXEL_CHUNK: usize = 16 * 1024;

/// The caller supplies a Sync transform (LCMS requires NO_CACHE).
pub(crate) fn convert_pixels<I: Sync, O: Default + Clone + Send>(
    pixels: &[I],
    convert: impl Fn(&[I], &mut [O]) + Sync,
) -> Vec<O> {
    let mut result = vec![O::default(); pixels.len()];
    pixels
        .par_chunks(PIXEL_CHUNK)
        .zip(result.par_chunks_mut(PIXEL_CHUNK))
        .for_each(|(input, output)| convert(input, output));
    result
}

pub(crate) fn convert_in_place<T: Send>(pixels: &mut [T], convert: impl Fn(&mut [T]) + Sync) {
    pixels.par_chunks_mut(PIXEL_CHUNK).for_each(&convert);
}

impl ProfileSource {
    /// Construct a built-in profile, or request external ICC bytes for `File`.
    pub fn built_in(self) -> Option<Profile> {
        match self {
            Self::Srgb => Some(rgb("sRGB", REC709, srgb_curve())),
            Self::DisplayP3 => Some(rgb("Display P3", P3, srgb_curve())),
            Self::Rec2020 => Some(rgb("Rec. 2020", REC2020, rec2020_curve())),
            Self::File => None,
        }
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
