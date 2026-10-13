//! JPEG encoding shares output conversion and capture tags with TIFF.
use crate::{
    Error, Result,
    definition::Assets,
    eval::InputValues,
    node::{
        data::{CaptureMetadata, Rec2020, Rgb, RgbDesc},
        profile,
    },
    param::Parameters,
    payload::Payload,
    ports::{Read, Requirement},
    registry::tag,
    runtime::KernelContext,
};
use jpeg_encoder::{ChromaSubsamplingMethod, ColorType, Encoder, SamplingFactor};
use lcms2::PixelFormat;
use std::path::PathBuf;

#[derive(Clone, Copy, crate::Choice)]
pub enum Subsampling {
    #[default]
    #[choice("444")]
    #[label("4:4:4")]
    Full,
    #[choice("422")]
    #[label("4:2:2")]
    HalfHorizontal,
    #[choice("420")]
    #[label("4:2:0")]
    Half,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Export {
    /// Destination JPEG file, written at full processing detail.
    #[label("Output file")]
    #[param(save = ["jpg", "jpeg"])]
    #[external]
    pub path: Option<PathBuf>,
    #[param(flatten)]
    #[serde(flatten)]
    pub output: profile::Settings,
    /// Higher values retain more detail and produce larger files. JPEG remains lossy at 100.
    #[param(default = 95, range = 1..=100)]
    pub quality: i64,
    /// Chroma resolution. Reduced chroma uses area averaging before encoding.
    #[label("Chroma subsampling")]
    #[param]
    pub subsampling: Subsampling,
}

fn validate(p: &Export) -> Result<()> {
    if !(1..=100).contains(&p.quality) {
        return Err(Error::Contract("JPEG quality must be 1..=100".into()));
    }
    Ok(())
}

impl Assets for Export {
    type Loaded = profile::Output;
    fn load(&self) -> Result<Self::Loaded> {
        profile::Output::load(&self.output)
    }
}

/// Write linear Rec.2020/D65 RGB to 8-bit JPEG through an embedded RGB ICC
/// profile. Samples clip to the output range before lossy encoding. Chroma can
/// retain full resolution or be averaged to half horizontal/vertical resolution.
/// Connected capture metadata is copied without orientation; stored pixels already
/// carry their final geometry. Export uses full processing detail independently of
/// preview detail. A profile file is read at the first export; choose it again to
/// pick up changes. JPEG dimensions must each be at most 65535 pixels.
#[crate::node(
    id = "export-jpeg",
    name = "Export JPEG",
    tags = [tag::IO],
    action = export,
    assets
)]
pub fn write(
    _: &KernelContext<'_>,
    _: &Export,
    _image: Read<'_, Rgb, Rec2020>,
    _metadata: Option<Read<'_, CaptureMetadata>>,
) -> Result<()> {
    Ok(())
}

fn export(p: &Export, output: &profile::Output, inputs: &InputValues) -> Result<()> {
    let path = p.path.as_ref().ok_or_else(|| Error::Contract("no output file chosen".into()))?;
    let inputs = inputs.bind::<write::Definition>()?;
    let metadata = inputs.metadata.map(|m| m.data.host().map(|m| &**m)).transpose()?;
    let bytes = encode(p, output, inputs.image.desc, inputs.image.data.host()?, metadata)?;
    std::fs::write(path, bytes).map_err(|e| Error::Runtime(format!("{}: {e}", path.display())))
}

/// Convert linear Rec.2020/D65 through `output` and encode an 8-bit JPEG.
pub fn encode(
    p: &Export,
    output: &profile::Output,
    desc: &RgbDesc,
    pixels: &[[f32; 3]],
    metadata: Option<&drip_raw::Metadata>,
) -> Result<Vec<u8>> {
    p.validate()?;
    Rgb::validate(desc)?;
    Rec2020::check(desc)?;
    let dimension = |value| {
        u16::try_from(value)
            .map_err(|_| Error::Contract("JPEG dimensions must be at most 65535 pixels".into()))
    };
    let width = dimension(desc.extent.width)?;
    let height = dimension(desc.extent.height)?;
    if pixels.len() != desc.extent.pixels() {
        return Err(Error::Contract("JPEG export pixels do not match the image extent".into()));
    }
    let pixels: Vec<[u8; 3]> =
        output.convert(PixelFormat::RGB_8, pixels).map_err(Error::Runtime)?;
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, p.quality as u8);
    encoder.set_sampling_factor(match p.subsampling {
        Subsampling::Full => SamplingFactor::R_4_4_4,
        Subsampling::HalfHorizontal => SamplingFactor::R_4_2_2,
        Subsampling::Half => SamplingFactor::R_4_2_0,
    });
    encoder.set_chroma_subsampling_method(ChromaSubsamplingMethod::Average);
    let err = |e: jpeg_encoder::EncodingError| Error::Runtime(e.to_string());
    encoder.add_icc_profile(&output.icc).map_err(err)?;
    if let Some(metadata) = metadata {
        let exif = super::exif_bytes(metadata).map_err(|e| Error::Runtime(e.to_string()))?;
        encoder.add_exif_metadata(&exif).map_err(err)?;
    }
    encoder.encode(pixels.as_flattened(), width, height, ColorType::Rgb).map_err(err)?;
    Ok(bytes)
}
