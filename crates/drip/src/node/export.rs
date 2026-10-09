//! Encode an evaluated image. File writes belong to the requesting frontend.
use crate::{
    Error, Result,
    node::data::{Color, ColorRgb, ImageDesc, Payload},
    node::profile,
};
use lcms2::PixelFormat;
use std::borrow::Cow;
use std::io::{Cursor, Seek, Write};
use tiff::encoder::colortype::{ColorType, RGB16, RGB32Float};
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression, DirectoryEncoder, Rational, TiffEncoder, TiffKind, TiffValue};
use tiff::tags::{Tag, Type};

/// Convert identity-encoded Rec.2020 through the requested output profile and
/// encode TIFF. Floating output retains the profile's floating range; u16 clips.
pub fn tiff(
    desc: &ImageDesc<Color>,
    pixels: &[[f32; 3]],
    output: &profile::Output,
    floating: bool,
    metadata: Option<&drip_raw::Metadata>,
) -> Result<Vec<u8>> {
    tiff_compressed(
        desc,
        pixels,
        output,
        floating,
        metadata,
        Compression::Deflate(DeflateLevel::Balanced),
    )
}

/// The same encoding with an explicit TIFF compression policy.
pub fn tiff_compressed(
    desc: &ImageDesc<Color>,
    pixels: &[[f32; 3]],
    output: &profile::Output,
    floating: bool,
    metadata: Option<&drip_raw::Metadata>,
    compression: Compression,
) -> Result<Vec<u8>> {
    ColorRgb::validate_desc(desc)?;
    if desc.interpretation != crate::node::raw::working_color()
        || pixels.len() != desc.extent.pixels()
    {
        return Err(Error::Contract(
            "TIFF export expects relative-white-1, identity Rec.2020/D65 RGB with matching extent"
                .into(),
        ));
    }
    let mut bytes = Cursor::new(Vec::new());
    let err = |e: tiff::TiffError| Error::Runtime(e.to_string());
    let mut encoder = TiffEncoder::new(&mut bytes).map_err(err)?.with_compression(compression);
    if floating {
        let pixels: Vec<[f32; 3]> = output.convert(PixelFormat::RGB_FLT, pixels)?;
        write_image::<RGB32Float>(
            &mut encoder,
            &desc.extent,
            &output.icc,
            metadata,
            pixels.as_flattened(),
        )
        .map_err(err)?;
    } else {
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
    Ok(bytes.into_inner())
}

type Tiff<'a> = TiffEncoder<&'a mut Cursor<Vec<u8>>>;

fn write_image<C: ColorType>(
    tiff: &mut Tiff,
    extent: &crate::node::data::Extent,
    icc: &[u8],
    metadata: Option<&drip_raw::Metadata>,
    data: &[C::Inner],
) -> tiff::TiffResult<()>
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
/// those tags are omitted. Orientation is not copied: pixels stay unrotated.
///
/// \[10\] CIPA DC-008-2019, Exif 2.32. Full reference in `THIRD_PARTY.md`.
fn write_exif(tiff: &mut Tiff, m: &drip_raw::Metadata) -> tiff::TiffResult<u32> {
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
