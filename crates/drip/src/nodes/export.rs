//! TIFF export with an embedded output profile and, if connected, camera metadata.

use std::borrow::Cow;
use std::io::{Cursor, Seek, Write};
use std::path::Path;

use lcms2::PixelFormat;
use tiff::encoder::colortype::{ColorType, RGB16, RGB32Float};
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression, DirectoryEncoder, Rational, TiffEncoder, TiffKind, TiffValue};
use tiff::tags::{Tag, Type};

use crate::image::{DisplayRec2020, RawMetadata, Rgb, ThreeChannelMatrix};
use crate::node::{EvalContext, Evaluated, KernelError, NodeKernel, NodeKind, TypedAction};
use crate::param::{ParamKind, ParamSpec, Params};
use crate::ports::{Optional, Read};
use crate::profile;

pub static TIFF: NodeKind = NodeKind::new::<TiffExport>(
    "export.tiff",
    "export",
    "Export",
    &[
        ParamSpec::new("path", ParamKind::Path { output: true }).external(),
        profile::PROFILE,
        profile::PROFILE_FILE,
        profile::INTENT,
        profile::BLACK_POINT_COMPENSATION,
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
    ) -> Result<Evaluated<()>, KernelError> {
        Ok(Evaluated::default())
    }
}

fn export(
    p: Params,
    (input, metadata): (&DisplayRec2020, Option<&RawMetadata>),
    _: &EvalContext,
) -> Result<(), KernelError> {
    let path = p.path("path").ok_or(KernelError::Incomplete("no output file chosen"))?;
    let in_file = |e: &dyn std::fmt::Display, path: &Path| format!("{}: {e}", path.display());
    let output = profile::Output::load(p)?;
    let compression = match (p.choice("compression"), p.choice("deflate_level")) {
        ("none", _) => Compression::Uncompressed,
        (_, "fast") => Compression::Deflate(DeflateLevel::Fast),
        (_, "balanced") => Compression::Deflate(DeflateLevel::Balanced),
        _ => Compression::Deflate(DeflateLevel::Best),
    };
    let image = input.rgb();
    // Encoded in memory and written at once, so every I/O error is reported
    // and a failed encoding leaves no partial file.
    let write = |encode: &dyn Fn(&mut Tiff) -> tiff::TiffResult<()>| {
        let mut bytes = Cursor::new(Vec::new());
        let mut tiff =
            TiffEncoder::new(&mut bytes).map_err(|e| e.to_string())?.with_compression(compression);
        encode(&mut tiff).map_err(|e| e.to_string())?;
        std::fs::write(path, bytes.into_inner()).map_err(|e| in_file(&e, path))
    };
    Ok(match p.choice("depth") {
        "u16" => {
            // LittleCMS saturates out-of-range values when encoding to 16 bit.
            let pixels: Vec<[u16; 3]> = output.convert(PixelFormat::RGB_16, &image.pixels)?;
            write(&|tiff| {
                write_image::<RGB16>(tiff, image, &output.icc, metadata, pixels.as_flattened())
            })
        }
        _ => {
            let pixels: Vec<[f32; 3]> = output.convert(PixelFormat::RGB_FLT, &image.pixels)?;
            let data = pixels.as_flattened();
            write(&|tiff| write_image::<RGB32Float>(tiff, image, &output.icc, metadata, data))
        }
    }?)
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
