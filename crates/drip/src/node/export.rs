//! TIFF export: an output node whose action encodes and writes its input.
use crate::{
    Error, Result,
    definition::Assets,
    node::data::{CaptureMetadata, Rec2020, Rgb, RgbDesc},
    node::profile,
    payload::Payload,
    ports::{Cpu, Read, Requirement},
    registry::tag,
    runtime::KernelContext,
};
use ::tiff::encoder::colortype::{ColorType, RGB8, RGB16, RGB32Float};
use ::tiff::encoder::compression::DeflateLevel;
use ::tiff::encoder::{Compression, DirectoryEncoder, Rational, TiffEncoder, TiffKind, TiffValue};
use ::tiff::tags::{Tag, Type};
use lcms2::PixelFormat;
use std::borrow::Cow;
use std::io::{Cursor, Seek, Write};
use std::path::PathBuf;

#[derive(Clone, Copy, crate::Choice)]
pub enum Depth {
    #[choice("u8")]
    U8,
    #[default]
    #[choice("u16")]
    U16,
    #[choice("f32")]
    F32,
}
#[derive(Clone, crate::Choice)]
pub enum CompressionMode {
    #[choice("none")]
    None,
    #[default]
    #[choice("deflate")]
    Deflate(DeflateSettings),
}
#[derive(Clone, Copy, crate::Choice)]
pub enum CompressionLevel {
    #[choice("fast")]
    Fast,
    #[default]
    #[choice("balanced")]
    Balanced,
    #[choice("best")]
    Best,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct DeflateSettings {
    /// Trade encoding speed for compression ratio without changing pixels.
    #[param]
    pub level: CompressionLevel,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Export {
    /// Destination TIFF file, written at full processing detail.
    #[label("Output file")]
    #[param(save = ["tiff", "tif"])]
    #[external]
    pub path: Option<PathBuf>,
    #[param(flatten)]
    #[serde(flatten)]
    pub output: profile::Settings,
    /// u8/u16 clip to the output range; f32 retains the profile floating-point range.
    #[param]
    pub depth: Depth,
    /// Lossless TIFF compression, or None for an uncompressed file.
    #[param]
    pub compression: CompressionMode,
}
impl Assets for Export {
    type Loaded = profile::Output;
    fn load(&self) -> Result<profile::Output> {
        profile::Output::load(&self.output)
    }
}

/// Write linear Rec.2020/D65 RGB to TIFF through an RGB ICC profile.
/// u8/u16 clip to the output range; f32 retains the profile's floating-point range.
/// Connected capture metadata is copied when present. Export uses full processing
/// detail, independently of preview detail. Compression is lossless. A profile
/// file is read at the first export; choose it again to pick up changes.
#[crate::node(
    id = "export-tiff",
    name = "Export TIFF",
    tags = [tag::ENCODING],
    action = export,
    assets
)]
pub fn tiff(
    _: &KernelContext<'_>,
    _: &Export,
    _image: Read<'_, Cpu<Rgb>, Rec2020>,
    _metadata: Option<Read<'_, Cpu<CaptureMetadata>>>,
) -> Result<()> {
    Ok(())
}
/// Encode and write the file named by the settings.
fn export(p: &Export, output: &profile::Output, inputs: tiff::Inputs<'_>) -> Result<()> {
    let path = p.path.as_ref().ok_or_else(|| Error::Contract("no output file chosen".into()))?;
    let metadata = inputs.metadata.map(|m| &**m.data);
    let bytes = encode(p, output, inputs.image.desc, inputs.image.data, metadata)?;
    std::fs::write(path, bytes).map_err(|e| Error::Runtime(format!("{}: {e}", path.display())))
}

/// Convert linear Rec.2020/D65 through `output`, the settings' loaded profile,
/// and encode TIFF. Floating output retains the profile's floating range;
/// integers clip.
pub fn encode(
    p: &Export,
    output: &profile::Output,
    desc: &RgbDesc,
    pixels: &[[f32; 3]],
    metadata: Option<&drip_raw::Metadata>,
) -> Result<Vec<u8>> {
    let compression = match &p.compression {
        CompressionMode::None => Compression::Uncompressed,
        CompressionMode::Deflate(settings) => Compression::Deflate(match settings.level {
            CompressionLevel::Fast => DeflateLevel::Fast,
            CompressionLevel::Balanced => DeflateLevel::Balanced,
            CompressionLevel::Best => DeflateLevel::Best,
        }),
    };
    Rgb::validate(desc)?;
    Rec2020::check(desc)?;
    if pixels.len() != desc.extent.pixels() {
        return Err(Error::Contract("TIFF export pixels do not match the image extent".into()));
    }
    let mut bytes = Cursor::new(Vec::new());
    let err = |e: ::tiff::TiffError| Error::Runtime(e.to_string());
    let mut encoder = TiffEncoder::new(&mut bytes).map_err(err)?.with_compression(compression);
    match p.depth {
        Depth::U8 => {
            let pixels: Vec<[u8; 3]> = output.convert(PixelFormat::RGB_8, pixels)?;
            write_image::<RGB8>(
                &mut encoder,
                &desc.extent,
                &output.icc,
                metadata,
                pixels.as_flattened(),
            )
            .map_err(err)?;
        }
        Depth::F32 => {
            let pixels: Vec<[f32; 3]> = output.convert(PixelFormat::RGB_FLT, pixels)?;
            write_image::<RGB32Float>(
                &mut encoder,
                &desc.extent,
                &output.icc,
                metadata,
                pixels.as_flattened(),
            )
            .map_err(err)?;
        }
        Depth::U16 => {
            let pixels: Vec<[u16; 3]> = output.convert(PixelFormat::RGB_16, pixels)?;
            write_image::<RGB16>(
                &mut encoder,
                &desc.extent,
                &output.icc,
                metadata,
                pixels.as_flattened(),
            )
            .map_err(err)?;
        }
    }
    Ok(bytes.into_inner())
}

type Tiff<'a> = TiffEncoder<&'a mut Cursor<Vec<u8>>>;

fn write_image<C: ColorType>(
    tiff: &mut Tiff,
    extent: &crate::node::data::Extent,
    icc: &[u8],
    metadata: Option<&drip_raw::Metadata>,
    data: &[C::Inner],
) -> ::tiff::TiffResult<()>
where
    [C::Inner]: TiffValue,
{
    let exif = metadata.map(|m| write_exif(tiff, m)).transpose()?;
    let mut encoder = tiff.new_image::<C>(extent.width, extent.height)?;
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
/// those tags are omitted. Orientation is not copied: geometry nodes determine
/// the stored pixel orientation, independently of the original capture metadata.
///
/// \[10\] CIPA DC-008-2019, Exif 2.32. Full reference in `THIRD_PARTY.md`.
fn write_exif(tiff: &mut Tiff, m: &drip_raw::Metadata) -> ::tiff::TiffResult<u32> {
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
) -> ::tiff::TiffResult<()> {
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
