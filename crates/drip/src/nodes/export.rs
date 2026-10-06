//! TIFF export with an embedded output profile and, if connected, camera metadata.

use std::borrow::Cow;
use std::io::{Cursor, Seek, Write};
use std::path::Path;

use lcms2::{
    ColorSpaceSignature, Flags, Intent, PixelFormat, Profile, ProfileClassSignature, Transform,
};
use tiff::encoder::colortype::{ColorType, RGB16, RGB32Float};
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression, DirectoryEncoder, Rational, TiffEncoder, TiffKind, TiffValue};
use tiff::tags::{Tag, Type};

use crate::image::{DisplayRec2020, RawMetadata, Rgb, ThreeChannelMatrix};
use crate::node::{EvalContext, Evaluated, NodeKernel, NodeKind, TypedAction};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::ports::{Optional, Read};
use crate::profile;

const INTENTS: &[&str] = &["perceptual", "relative", "saturation", "absolute"];

/// The built-in profiles, or `file` for `profile_file`.
const PROFILES: &[&str] = &["srgb", "display_p3", "rec2020", "file"];

pub static TIFF: NodeKind = NodeKind::new::<TiffExport>(
    "export.tiff",
    "export",
    &[
        ParamSpec::new("path", ParamKind::Path { output: true }).external(),
        ParamSpec::new("profile", ParamKind::Choice { options: PROFILES, default: "srgb" }),
        ParamSpec::new("profile_file", ParamKind::Path { output: false }),
        ParamSpec::new("intent", ParamKind::Choice { options: INTENTS, default: "relative" }),
        ParamSpec::new("black_point_compensation", ParamKind::Bool { default: true }),
        ParamSpec::new("depth", ParamKind::Choice { options: &["u16", "f32"], default: "u16" }),
        ParamSpec::new(
            "compression",
            ParamKind::Choice { options: &["none", "deflate"], default: "deflate" },
        ),
        ParamSpec::new(
            "deflate_level",
            ParamKind::Choice { options: &["fast", "balanced", "best"], default: "balanced" },
        ),
    ],
    &["image", "metadata"],
    &[],
);
struct TiffExport;
impl NodeKernel for TiffExport {
    type Inputs = (Read<DisplayRec2020>, Optional<Read<RawMetadata>>);
    type Outputs = ();
    const ACTIONS: &'static [TypedAction<Self>] = &[TypedAction { name: "export", run: export }];
    fn eval(
        _: Params<'_>,
        _: (&DisplayRec2020, Option<&RawMetadata>),
        _: &EvalContext<'_>,
    ) -> Result<Evaluated<()>, String> {
        Ok(Evaluated::default())
    }
}

fn export(
    p: Params,
    (input, metadata): (&DisplayRec2020, Option<&RawMetadata>),
    _: &EvalContext,
) -> Result<(), String> {
    let path = p.path("path").ok_or("no output file chosen")?;
    let in_file = |e: &dyn std::fmt::Display, path: &Path| format!("{}: {e}", path.display());
    let (output, icc) = match p.choice("profile") {
        "file" => {
            let file = p.path("profile_file").ok_or("no output profile file chosen")?;
            let icc = std::fs::read(file).map_err(|e| in_file(&e, file))?;
            (output_profile(&icc).map_err(|e| in_file(&e, file))?, icc)
        }
        name => {
            let built_in = profile::built_in(name);
            let icc = built_in.icc().map_err(|e| e.to_string())?;
            (built_in, icc)
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
    let compression = match (p.choice("compression"), p.choice("deflate_level")) {
        ("none", _) => Compression::Uncompressed,
        (_, "fast") => Compression::Deflate(DeflateLevel::Fast),
        (_, "balanced") => Compression::Deflate(DeflateLevel::Balanced),
        _ => Compression::Deflate(DeflateLevel::Best),
    };
    let image = input.rgb();
    let convert = |format| Conversion {
        source: profile::rec2020_linear(),
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
            write(&|tiff| write_image::<RGB16>(tiff, image, &icc, metadata, pixels.as_flattened()))
        }
        _ => {
            let pixels: Vec<[f32; 3]> = convert(PixelFormat::RGB_FLT).run()?;
            let data = pixels.as_flattened();
            write(&|tiff| write_image::<RGB32Float>(tiff, image, &icc, metadata, data))
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
    metadata: Option<&RawMetadata>,
    data: &[C::Inner],
) -> tiff::TiffResult<()>
where
    [C::Inner]: TiffValue,
{
    let exif = metadata.map(|m| write_exif(tiff, m)).transpose()?;
    let mut encoder = tiff.new_image::<C>(image.width as u32, image.height as u32)?;
    let ifd = encoder.encoder();
    ifd.write_tag(Tag::IccProfile, Undefined(icc))?;
    if let Some((m, exif)) = metadata.zip(exif) {
        write_text(ifd, Tag::Make, &m.make)?;
        write_text(ifd, Tag::Model, &m.model)?;
        ifd.write_tag(Tag::ExifDirectory, exif)?;
    }
    encoder.write_data(data)
}

/// Writes the capture settings to an Exif IFD \[10\] and returns its offset for
/// the image IFD's pointer. LibRaw reports unknown values as zero or empty, and
/// those tags are omitted. Orientation is not copied: pixels stay unrotated.
///
/// \[10\] CIPA DC-008-2019, Exif 2.32. Full reference in `THIRD_PARTY.md`.
fn write_exif(tiff: &mut Tiff, m: &RawMetadata) -> tiff::TiffResult<u32> {
    const EXPOSURE_TIME: u16 = 0x829a;
    const F_NUMBER: u16 = 0x829d;
    const PHOTOGRAPHIC_SENSITIVITY: u16 = 0x8827;
    const DATE_TIME_ORIGINAL: u16 = 0x9003;
    const FOCAL_LENGTH: u16 = 0x920a;

    let mut ifd = tiff.extra_directory()?;
    ifd.write_tag(Tag::ExifVersion, Undefined(b"0232"))?;
    for (tag, value) in
        [(EXPOSURE_TIME, m.shutter), (F_NUMBER, m.aperture), (FOCAL_LENGTH, m.focal_length)]
    {
        if value > 0.0 {
            ifd.write_tag(Tag::Unknown(tag), rational(value))?;
        }
    }
    if m.iso > 0.0 {
        // Saturates at 65535, which Exif prescribes for higher sensitivities.
        ifd.write_tag(Tag::Unknown(PHOTOGRAPHIC_SENSITIVITY), m.iso.round() as u16)?;
    }
    write_text(&mut ifd, Tag::Unknown(DATE_TIME_ORIGINAL), &m.datetime)?;
    Ok(ifd.finish_with_offsets()?.offset)
}

fn write_text<W: Write + Seek, K: TiffKind>(
    ifd: &mut DirectoryEncoder<W, K>,
    tag: Tag,
    text: &str,
) -> tiff::TiffResult<()> {
    if text.is_empty() { Ok(()) } else { ifd.write_tag(tag, text) }
}

/// The first continued-fraction convergent within 0.01% of `x > 0`, so values
/// read as photographers write them: 1/320 s rather than 0.003125 s, f/6.3.
fn rational(x: f32) -> Rational {
    let x = f64::from(x);
    // Convergents h/k, each with its predecessor.
    let ((mut h, mut h0), (mut k, mut k0), mut rest) = ((1, 0), (0, 1), x);
    loop {
        let a = rest.floor();
        (h, h0) = (a as u64 * h + h0, h);
        (k, k0) = (a as u64 * k + k0, k);
        if (h as f64 / k as f64 - x).abs() <= x * 1e-4 || rest == a {
            return Rational { n: h as u32, d: k as u32 };
        }
        rest = 1.0 / (rest - a);
    }
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
