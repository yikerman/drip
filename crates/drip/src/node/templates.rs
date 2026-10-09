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
    let raw = p.dag.add(raw::RawSource::bind(raw::Settings { path: path.map(Path::to_owned) }))?;
    let mosaic = p.dag.typed_output(p.dag.output_port(raw, "mosaic")?)?;
    let site_gains = p.dag.typed_output(p.dag.output_port(raw, "gains")?)?;
    let clipping = p.dag.typed_output(p.dag.output_port(raw, "levels")?)?;
    let matrix = p.dag.typed_output(p.dag.output_port(raw, "matrix")?)?;
    let gains = node::bayer_gains::add(&mut p.dag, ())?;
    let levels = node::gain_levels::add(&mut p.dag, ())?;
    let highlights = node::highlights::reconstruct::add(&mut p.dag, Default::default())?;
    let demosaic = node::rcd::add(&mut p.dag, ())?;
    let color = node::camera_to_rgb::add(&mut p.dag, ())?;
    let exposure = node::exposure::add(&mut p.dag, Default::default())?;
    let sigmoid = sigmoid::apply::add(&mut p.dag, Default::default())?;
    p.dag.edit(|edit| {
        edit.connect(mosaic, gains.image)?;
        edit.connect(site_gains, gains.gains)?;
        edit.connect(clipping, levels.levels)?;
        edit.connect(site_gains, levels.gains)?;
        edit.connect(levels.output, highlights.levels)?;
        edit.connect(gains.output, highlights.image)?;
        edit.connect(highlights.output, demosaic.image)?;
        edit.connect(demosaic.output, color.image)?;
        edit.connect(matrix, color.matrix)?;
        edit.connect(color.output, exposure.image)?;
        edit.connect(exposure.output, sigmoid.image)?;
        Ok(())
    })?;
    p.target = Some(sigmoid.output.id());
    Ok(p)
}
