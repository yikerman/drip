//! The processing chains of the built-in templates, wired before their RAW
//! and output files are chosen. A frontend adds its views and layout. A
//! batch run sets the paths and evaluates the exports.
//!
//! Every chain develops the RAW file, applies a neutral look and exports a
//! TIFF. The templates differ in the steps they add, in brackets below:
//!
//! ```text
//! Bayer RAW ─► [Hot pixel removal] ─► Bayer site gains ─► Opposed highlights
//!   ─► RCD demosaic ─► [Lens correction] ─► Camera RGB to color RGB
//!   ─► Chromatic adaptation ─► Orientation ─► [Sharpen] ─► Exposure ─► Sigmoid
//!   ─► [Perspective Transform] ─► [Resample] ─► Export TIFF, [Export JPEG]
//! ```
//!
//! | Template | Adds |
//! |---|---|
//! | [`neutral_restore`] | nothing |
//! | [`neutral_correction`] | hot pixel removal, lens correction, sharpening, perspective |
//! | [`all`] | the corrections, resampling and a JPEG export |
//!
//! Lens correction applies the correction the camera embedded, if any. The
//! other added steps start where they change little or nothing: the
//! perspective keeps the frame and the resample keeps the size.
use crate::{
    Result,
    graph::Dag,
    node::{self, data::*},
    ports::Output,
};

/// Handles of a built-in chain, for frontends and tests that extend it.
pub struct Chain {
    pub raw: node::raw::Ports,
    pub develop: Develop,
    /// The finished image, which the exports and a frontend's views read.
    pub output: Output<Rgb>,
    pub export: node::export::tiff::Ports,
}

/// RAW development: sensor levels, highlight reconstruction and demosaic,
/// then camera RGB to Rec.2020 under the shot's white balance, upright.
pub struct Develop {
    pub gains: node::bayer_gains::Ports,
    pub highlights: node::highlights::reconstruct::Ports,
    pub demosaic: node::rcd::Ports,
    /// Lens correction on the sensor's channels, in the templates that
    /// correct.
    pub lens: Option<node::lens::Ports>,
    pub color: node::camera_to_rgb::Ports,
    pub adaptation: node::chromatic_adaptation::Ports,
    pub orientation: node::orientation::Ports,
}

/// Develop, look and TIFF export, nothing else.
pub fn neutral_restore(dag: &mut Dag) -> Result<Chain> {
    let raw = node::raw::add(dag, Default::default())?;
    let develop = develop(dag, raw.mosaic, raw.metadata, false)?;
    let output = look(dag, develop.orientation.output)?;
    export(dag, raw, develop, output)
}

/// Neutral restore with the corrections a capture usually needs: hot pixel
/// removal on the mosaic, the camera's lens correction after demosaic,
/// sharpening before the look, and a perspective transform after it.
pub fn neutral_correction(dag: &mut Dag) -> Result<Chain> {
    let (raw, develop, output) = corrected(dag)?;
    export(dag, raw, develop, output)
}

/// Neutral correction, resampled, and exported as a JPEG beside the TIFF.
pub fn all(dag: &mut Dag) -> Result<Chain> {
    let (raw, develop, corrected) = corrected(dag)?;
    let resample = node::resample::add(dag, Default::default())?;
    dag.connect(corrected, resample.image)?;
    let jpeg = node::export::jpeg::write::add(dag, Default::default())?;
    dag.connect(resample.output, jpeg.image)?;
    dag.connect(raw.metadata, jpeg.metadata)?;
    export(dag, raw, develop, resample.output)
}

fn corrected(dag: &mut Dag) -> Result<(node::raw::Ports, Develop, Output<Rgb>)> {
    let raw = node::raw::add(dag, Default::default())?;
    let hot_pixels = node::hot_pixels::add(dag, Default::default())?;
    dag.connect(raw.mosaic, hot_pixels.image)?;
    let develop = develop(dag, hot_pixels.output, raw.metadata, true)?;
    let sharpen = node::sharpen::add(dag, Default::default())?;
    dag.connect(develop.orientation.output, sharpen.image)?;
    let finished = look(dag, sharpen.output)?;
    let perspective = node::perspective::add(dag, Default::default())?;
    dag.connect(finished, perspective.image)?;
    Ok((raw, develop, perspective.output))
}

fn develop(
    dag: &mut Dag,
    mosaic: Output<Bayer>,
    metadata: Output<CaptureMetadata>,
    correct_lens: bool,
) -> Result<Develop> {
    let gains = node::bayer_gains::add(dag, ())?;
    let highlights = node::highlights::reconstruct::add(dag, Default::default())?;
    let demosaic = node::rcd::add(dag, ())?;
    let lens = correct_lens.then(|| node::lens::add(dag, Default::default())).transpose()?;
    let color = node::camera_to_rgb::add(dag, ())?;
    let adaptation = node::chromatic_adaptation::add(dag, Default::default())?;
    let orientation = node::orientation::add(dag, Default::default())?;
    dag.edit(|edit| {
        edit.connect(mosaic, gains.image)?;
        edit.connect(metadata, gains.metadata)?;
        edit.connect(gains.levels, highlights.levels)?;
        edit.connect(gains.output, highlights.image)?;
        edit.connect(highlights.output, demosaic.image)?;
        match &lens {
            Some(lens) => {
                edit.connect(demosaic.output, lens.image)?;
                edit.connect(metadata, lens.metadata)?;
                edit.connect(lens.output, color.image)?;
            }
            None => edit.connect(demosaic.output, color.image)?,
        }
        edit.connect(metadata, color.metadata)?;
        edit.connect(color.output, adaptation.image)?;
        edit.connect(metadata, adaptation.metadata)?;
        edit.connect(adaptation.output, orientation.image)?;
        edit.connect(metadata, orientation.metadata)
    })?;
    Ok(Develop { gains, highlights, demosaic, lens, color, adaptation, orientation })
}

/// Exposure, then Sigmoid's tone curve to display range.
fn look(dag: &mut Dag, image: Output<Rgb>) -> Result<Output<Rgb>> {
    let exposure = node::exposure::add(dag, Default::default())?;
    let sigmoid = node::sigmoid::apply::add(dag, Default::default())?;
    dag.connect(image, exposure.image)?;
    dag.connect(exposure.output, sigmoid.image)?;
    Ok(sigmoid.output)
}

fn export(
    dag: &mut Dag,
    raw: node::raw::Ports,
    develop: Develop,
    output: Output<Rgb>,
) -> Result<Chain> {
    let export = node::export::tiff::add(dag, Default::default())?;
    dag.connect(output, export.image)?;
    dag.connect(raw.metadata, export.metadata)?;
    Ok(Chain { raw, develop, output, export })
}
