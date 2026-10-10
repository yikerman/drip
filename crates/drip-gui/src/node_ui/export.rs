//! Full-resolution file action over explicitly connected image and metadata.
use ::tiff::encoder::{Compression, compression::DeflateLevel};
use drip::{
    Error, Result, node::data::*, node::profile, param::ParamKind, ports::*, runtime::KernelContext,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, drip::Choice)]
pub enum Depth {
    #[default]
    #[choice("u16")]
    U16,
    #[choice("f32")]
    F32,
}
#[derive(Clone, drip::Choice)]
pub enum CompressionMode {
    #[choice("none")]
    None,
    #[choice("deflate")]
    Deflate(DeflateSettings),
}
#[derive(Clone, Copy, Default, drip::Choice)]
pub enum CompressionLevel {
    #[choice("fast")]
    Fast,
    #[default]
    #[choice("balanced")]
    Balanced,
    #[choice("best")]
    Best,
}
#[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Export {
    /// Destination TIFF file, written at full processing detail.
    #[label("Output file")]
    #[param(ParamKind::Path { output: true })]
    #[external]
    pub path: Option<std::path::PathBuf>,
    #[param(flatten)]
    #[serde(flatten)]
    pub output: profile::Settings,
    /// u16 clips to the output range; f32 retains the profile floating-point range.
    #[param(Depth::U16.schema())]
    pub depth: Depth,
    /// Lossless TIFF compression, or None for an uncompressed file.
    #[param(CompressionMode::Deflate(DeflateSettings { level: CompressionLevel::Balanced }).schema())]
    pub compression: CompressionMode,
}
#[derive(Clone, Default, Serialize, Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
pub struct DeflateSettings {
    /// Trade encoding speed for compression ratio without changing pixels.
    #[param(CompressionLevel::Balanced.schema())]
    pub level: CompressionLevel,
}
impl Default for CompressionMode {
    fn default() -> Self {
        Self::Deflate(DeflateSettings::default())
    }
}

fn contract(
    _: &drip::runtime::GlobalContext,
    _: &Export,
    image: Option<&ImageDesc<Color>>,
    _: Option<&MetadataDesc>,
) -> Result<()> {
    crate::node_ui::contracts::working(image)
}
/// Write identity-encoded Rec.2020/D65 ColorRgb to TIFF through an RGB ICC profile.
/// u16 clips to the output range; f32 retains the profile's floating-point range.
/// Connected CaptureMetadata is copied when present. Export uses full processing
/// detail, independently of preview detail. Compression is lossless.
#[drip::node(id="output.tiff", name="Export TIFF", category="Output", contract=contract)]
fn tiff(
    _: &KernelContext<'_>,
    _: &Export,
    image: Read<'_, Cpu<ColorRgb>>,
    metadata: Option<Read<'_, Cpu<CaptureMetadata>>>,
) -> Result<()> {
    let _ = (image, metadata);
    Ok(())
}
struct ExportGui;
#[drip_macros::gui_node]
impl super::GuiNode for ExportGui {
    type Parameters = Export;
    type Presentation = ();
    const ID: &'static str = "output.tiff";
    const ACTION: Option<super::binding::Action<Self>> = Some(export);
    fn parameter_ui(_: &Export, path: &str) -> super::ParameterUi {
        if path == "path" {
            super::ParameterUi {
                file: Some(super::FileUi {
                    title: "Choose TIFF destination",
                    filter: "TIFF image",
                    extensions: &["tiff", "tif"],
                }),
            }
        } else {
            super::profile::parameter_ui(path)
        }
    }
}
fn export(
    p: Export,
    inputs: &drip::eval::InputValues,
    ctx: &crate::node_ui::data::PrepareContext,
) -> Result<()> {
    let path = p.path.ok_or_else(|| Error::Contract("no output file chosen".into()))?;
    let (desc, pixels) = inputs
        .get("image")
        .ok_or_else(|| Error::Contract("missing export image".into()))?
        .get::<Cpu<ColorRgb>>()?;
    let metadata = inputs
        .get("metadata")
        .map(|v| v.get::<Cpu<CaptureMetadata>>().map(|(_, v)| v))
        .transpose()?;
    let output = profile::Output::load(&p.output, &ctx.resources)?;
    let compression = match p.compression {
        CompressionMode::None => Compression::Uncompressed,
        CompressionMode::Deflate(settings) => Compression::Deflate(match settings.level {
            CompressionLevel::Fast => DeflateLevel::Fast,
            CompressionLevel::Balanced => DeflateLevel::Balanced,
            CompressionLevel::Best => DeflateLevel::Best,
        }),
    };
    let bytes = drip::node::export::tiff_compressed(
        desc,
        pixels,
        &output,
        matches!(p.depth, Depth::F32),
        metadata,
        compression,
    )?;
    std::fs::write(&path, bytes).map_err(|e| Error::Runtime(format!("{}: {e}", path.display())))
}
