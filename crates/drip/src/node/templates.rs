//! Checked starting graphs. Loading is explicit and precedes connection checks.
use crate::{
    Result,
    graph::Dag,
    node::{self, raw, sigmoid},
};

/// Handles of the starting pipeline, for frontend layout and further consumers.
pub struct RawToTiff {
    pub raw: raw::Ports,
    pub gains: node::bayer_gains::Ports,
    pub highlights: node::highlights::reconstruct::Ports,
    pub demosaic: node::rcd::Ports,
    pub color: node::camera_to_rgb::Ports,
    pub adaptation: node::chromatic_adaptation::Ports,
    pub orientation: node::orientation::Ports,
    pub exposure: node::exposure::Ports,
    pub sigmoid: sigmoid::apply::Ports,
    pub export: node::export::tiff::Ports,
}

/// RAW to TIFF, wired before its RAW and output files are chosen.
pub fn raw_to_tiff() -> Result<(Dag, RawToTiff)> {
    let mut dag = Dag::new();
    let raw = raw::add(&mut dag, Default::default())?;
    let (mosaic, metadata) = (raw.mosaic, raw.metadata);
    let gains = node::bayer_gains::add(&mut dag, ())?;
    let highlights = node::highlights::reconstruct::add(&mut dag, Default::default())?;
    let demosaic = node::rcd::add(&mut dag, ())?;
    let color = node::camera_to_rgb::add(&mut dag, ())?;
    let adaptation = node::chromatic_adaptation::add(&mut dag, Default::default())?;
    let orientation = node::orientation::add(&mut dag, Default::default())?;
    let exposure = node::exposure::add(&mut dag, Default::default())?;
    let sigmoid = sigmoid::apply::add(&mut dag, Default::default())?;
    let export = node::export::tiff::add(&mut dag, Default::default())?;
    dag.edit(|edit| {
        edit.connect(mosaic, gains.image)?;
        edit.connect(metadata, gains.metadata)?;
        edit.connect(gains.levels, highlights.levels)?;
        edit.connect(gains.output, highlights.image)?;
        edit.connect(highlights.output, demosaic.image)?;
        edit.connect(demosaic.output, color.image)?;
        edit.connect(metadata, color.metadata)?;
        edit.connect(color.output, adaptation.image)?;
        edit.connect(metadata, adaptation.metadata)?;
        edit.connect(adaptation.output, orientation.image)?;
        edit.connect(metadata, orientation.metadata)?;
        edit.connect(orientation.output, exposure.image)?;
        edit.connect(exposure.output, sigmoid.image)?;
        edit.connect(sigmoid.output, export.image)?;
        edit.connect(metadata, export.metadata)?;
        Ok(())
    })?;
    Ok((
        dag,
        RawToTiff {
            raw,
            gains,
            highlights,
            demosaic,
            color,
            adaptation,
            orientation,
            exposure,
            sigmoid,
            export,
        },
    ))
}
