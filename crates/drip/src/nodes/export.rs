//! TIFF export with an embedded output profile and, if connected, camera metadata.

use std::borrow::Cow;
use std::io::{Cursor, Seek, Write};
use std::path::Path;

use lcms2::PixelFormat;
use tiff::encoder::colortype::{ColorType, RGB16, RGB32Float};
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression, DirectoryEncoder, Rational, TiffEncoder, TiffKind, TiffValue};
use tiff::tags::{Tag, Type};

use crate::image::{RawMetadata, Rec2020Rgb, Rgb};
use crate::node::{EvalContext, KernelError};
use crate::param::ParamKind;
use crate::ports::MatRef;
use crate::profile;

#[derive(Clone, Copy, crate::Choice)]
enum Depth {
    #[choice("u16")]
    U16,
    #[choice("f32")]
    F32,
}

#[derive(Clone, Copy, crate::Choice)]
enum CompressionMode {
    #[choice("none")]
    None,
    #[choice("deflate")]
    Deflate,
}

#[derive(Clone, Copy, crate::Choice)]
enum CompressionLevel {
    #[choice("fast")]
    Fast,
    #[choice("balanced")]
    Balanced,
    #[choice("best")]
    Best,
}

#[derive(crate::Parameters)]
pub struct Export {
    #[param(ParamKind::Path { output: true })]
    #[external]
    path: Option<std::path::PathBuf>,
    #[param(flatten)]
    output: profile::Settings,
    #[param(Depth::U16.schema())]
    depth: Depth,
    #[param(CompressionMode::Deflate.schema())]
    compression: CompressionMode,
    #[param(CompressionLevel::Balanced.schema())]
    deflate_level: CompressionLevel,
}

/// Export Rec.2020 RGB to a TIFF file at full detail through the selected RGB ICC
/// profile, including its transfer encoding.
///
/// Tone mapping beforehand is optional. u16 saturates out-of-range values.
/// RAW metadata, if connected, is also copied.
#[crate::node(kind = TIFF, id = "export.tiff", category = "export", name = "Export", outputs = [], actions = [("export", export)])]
fn tiff(
    _: Export,
    (image, metadata): (MatRef<'_, 3, dyn Rec2020Rgb>, Option<&RawMetadata>),
    _: &EvalContext<'_>,
) -> Result<(), KernelError>;

fn export(
    p: Export,
    (input, metadata): (MatRef<'_, 3, dyn Rec2020Rgb>, Option<&RawMetadata>),
    ctx: &EvalContext,
) -> Result<(), KernelError> {
    let path = p.path.as_deref().ok_or(KernelError::Incomplete("no output file chosen"))?;
    let in_file = |e: &dyn std::fmt::Display, path: &Path| format!("{}: {e}", path.display());
    let output = profile::Output::load(&p.output, ctx.resources())?;
    let compression = match p.compression {
        CompressionMode::None => Compression::Uncompressed,
        CompressionMode::Deflate => Compression::Deflate(match p.deflate_level {
            CompressionLevel::Fast => DeflateLevel::Fast,
            CompressionLevel::Balanced => DeflateLevel::Balanced,
            CompressionLevel::Best => DeflateLevel::Best,
        }),
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
    Ok(match p.depth {
        Depth::U16 => {
            // LittleCMS saturates out-of-range values when encoding to 16 bit.
            let pixels: Vec<[u16; 3]> = output.convert(PixelFormat::RGB_16, &image.pixels)?;
            write(&|tiff| {
                write_image::<RGB16>(tiff, image, &output.icc, metadata, pixels.as_flattened())
            })
        }
        Depth::F32 => {
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
