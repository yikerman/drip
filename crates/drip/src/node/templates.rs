//! Checked starting graphs. Loading is explicit and precedes connection checks.
use crate::{
    Result,
    node::{self, raw, sigmoid},
    project::Project,
};
use std::path::Path;

pub fn raw_to_rgb(path: &Path) -> Result<Project> {
    raw_pipeline(Some(path))
}

/// The editable starting pipeline can be wired before its source is selected.
pub fn raw_pipeline(path: Option<&Path>) -> Result<Project> {
    let mut p = Project::default();
    let raw = raw::add(&mut p.dag, raw::Settings { path: path.map(Path::to_owned) })?;
    let (mosaic, metadata) = (raw.mosaic, raw.metadata);
    let gains = node::bayer_gains::add(&mut p.dag, ())?;
    let highlights = node::highlights::reconstruct::add(&mut p.dag, Default::default())?;
    let demosaic = node::rcd::add(&mut p.dag, ())?;
    let color = node::camera_to_rgb::add(&mut p.dag, ())?;
    let adaptation = node::chromatic_adaptation::add(&mut p.dag, Default::default())?;
    let orientation = node::orientation::add(&mut p.dag, Default::default())?;
    let exposure = node::exposure::add(&mut p.dag, Default::default())?;
    let sigmoid = sigmoid::apply::add(&mut p.dag, Default::default())?;
    p.dag.edit(|edit| {
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
        Ok(())
    })?;
    p.target = Some(sigmoid.output.id());
    Ok(p)
}
