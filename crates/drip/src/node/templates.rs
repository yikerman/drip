//! Checked starting graphs. Loading is explicit and precedes connection checks.
use crate::{
    Result,
    graph::Dag,
    node::{self, data::Rgb, raw, sigmoid},
    ports::Output,
};

/// RAW to TIFF: the editable starting pipeline, wired before its RAW and
/// output files are chosen, and its rendered output for further consumers.
pub fn raw_to_tiff() -> Result<(Dag, Output<Rgb>)> {
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
    Ok((dag, sigmoid.output))
}
