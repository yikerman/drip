//! TIFF export with an embedded output profile (DESIGN C5).

use std::borrow::Cow;
use std::io::Cursor;
use std::path::Path;

use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, Flags, Intent, PixelFormat, Profile,
    ProfileClassSignature, ToneCurve, Transform,
};
use tiff::encoder::colortype::{ColorType, RGB16, RGB32Float};
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression, TiffEncoder, TiffValue};
use tiff::tags::{Tag, Type};

use crate::color::{D65, REC2020};
use crate::node::{Action, Evaluated, InputSpec, NodeKind};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::value::{PortType, Rgb, Value};

const INTENTS: &[&str] = &["perceptual", "relative", "saturation", "absolute"];

pub static TIFF: NodeKind = NodeKind {
    name: "export.tiff",
    version: 1,
    params: &[
        ParamSpec { name: "path", kind: ParamKind::Path { output: true } },
        ParamSpec { name: "profile", kind: ParamKind::Path { output: false } },
        ParamSpec {
            name: "intent",
            kind: ParamKind::Choice { options: INTENTS, default: "relative" },
        },
        ParamSpec { name: "black_point_compensation", kind: ParamKind::Bool { default: true } },
        ParamSpec {
            name: "depth",
            kind: ParamKind::Choice { options: &["u16", "f32"], default: "u16" },
        },
        ParamSpec {
            name: "compression",
            kind: ParamKind::Choice { options: &["none", "deflate"], default: "deflate" },
        },
        ParamSpec {
            name: "deflate_level",
            kind: ParamKind::Choice { options: &["fast", "balanced", "best"], default: "balanced" },
        },
    ],
    inputs: &[InputSpec { name: "image", accepts: &[PortType::DisplayRec2020] }],
    outputs: &[],
    eval: |_, _, _| Ok(Evaluated::default()),
    actions: &[Action { name: "export", run: export }],
    migrate: None,
};

fn export(p: Params, inputs: &[Value]) -> Result<(), String> {
    let path = p.path("path").ok_or("no output file chosen")?;
    let profile_path = p.path("profile").ok_or("no output profile chosen")?;
    let in_file = |e: &dyn std::fmt::Display, path: &Path| format!("{}: {e}", path.display());
    let icc = std::fs::read(profile_path).map_err(|e| in_file(&e, profile_path))?;
    let output = output_profile(&icc).map_err(|e| in_file(&e, profile_path))?;

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
    let compression = match (p.choice("compression"), p.choice("deflate_level")) {
        ("none", _) => Compression::Uncompressed,
        (_, "fast") => Compression::Deflate(DeflateLevel::Fast),
        (_, "balanced") => Compression::Deflate(DeflateLevel::Balanced),
        _ => Compression::Deflate(DeflateLevel::Best),
    };
    let image = inputs[0].rgb();
    let convert = |format| Conversion {
        source: rec2020_linear(),
        output: &output,
        format,
        intent,
        flags,
        image,
    };
    // Encoded in memory and written at once, so every I/O error is reported
    // and a failed encoding leaves no partial file.
    let write = |encode: &dyn Fn(&mut Tiff) -> tiff::TiffResult<()>| {
        let mut bytes = Cursor::new(Vec::new());
        let mut tiff =
            TiffEncoder::new(&mut bytes).map_err(|e| e.to_string())?.with_compression(compression);
        encode(&mut tiff).map_err(|e| e.to_string())?;
        std::fs::write(path, bytes.into_inner()).map_err(|e| in_file(&e, path))
    };
    match p.choice("depth") {
        "u16" => {
            // LittleCMS saturates out-of-range values when encoding to 16 bit.
            let pixels: Vec<[u16; 3]> = convert(PixelFormat::RGB_16).run()?;
            write(&|tiff| write_image::<RGB16>(tiff, image, &icc, pixels.as_flattened()))
        }
        _ => {
            let pixels: Vec<[f32; 3]> = convert(PixelFormat::RGB_FLT).run()?;
            write(&|tiff| write_image::<RGB32Float>(tiff, image, &icc, pixels.as_flattened()))
        }
    }
}

struct Conversion<'a> {
    source: Profile,
    output: &'a Profile,
    format: PixelFormat,
    intent: Intent,
    flags: Flags,
    image: &'a Rgb,
}

impl Conversion<'_> {
    fn run<O: lcms2::Pod + Default>(&self) -> Result<Vec<[O; 3]>, String> {
        let t = Transform::new_flags(
            &self.source,
            PixelFormat::RGB_FLT,
            self.output,
            self.format,
            self.intent,
            self.flags,
        )
        .map_err(|e| e.to_string())?;
        let mut pixels = vec![[O::default(); 3]; self.image.pixels.len()];
        t.transform_pixels(&self.image.pixels, &mut pixels);
        Ok(pixels)
    }
}

type Tiff<'a> = TiffEncoder<&'a mut Cursor<Vec<u8>>>;

fn write_image<C: ColorType>(
    tiff: &mut Tiff,
    image: &Rgb,
    icc: &[u8],
    data: &[C::Inner],
) -> tiff::TiffResult<()>
where
    [C::Inner]: TiffValue,
{
    let mut encoder = tiff.new_image::<C>(image.width as u32, image.height as u32)?;
    encoder.encoder().write_tag(Tag::IccProfile, Undefined(icc))?;
    encoder.write_data(data)
}

/// Opaque bytes, which TIFF types as UNDEFINED; `&[u8]` would be typed BYTE.
struct Undefined<'a>(&'a [u8]);

impl TiffValue for Undefined<'_> {
    const BYTE_LEN: u8 = 1;
    const FIELD_TYPE: Type = Type::UNDEFINED;

    fn count(&self) -> usize {
        self.0.len()
    }

    fn data(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(self.0)
    }
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

/// The working space: Rec.2020 primaries, D65, linear.
pub fn rec2020_linear() -> Profile {
    let xy = |[x, y]: [f64; 2]| CIExyY { x, y, Y: 1.0 };
    let primaries =
        CIExyYTRIPLE { Red: xy(REC2020[0]), Green: xy(REC2020[1]), Blue: xy(REC2020[2]) };
    let linear = ToneCurve::new(1.0);
    Profile::new_rgb(&xy(D65), &primaries, &[&linear; 3]).expect("valid built-in profile")
}
