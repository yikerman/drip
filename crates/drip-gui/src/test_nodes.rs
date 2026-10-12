//! Test-only node kinds that the registry, and so project loading, must find.
use drip::{
    node::data::{Coordinates, Extent, REC2020_D65, Rgb, RgbDesc},
    ports::{Cpu, Write},
    runtime::KernelContext,
};

pub static NODES: &[&drip::definition::Kind] = &[&display::KIND];

const PIXELS: [[f32; 3]; 4] = [[1.0; 3], [0.18; 3], [0.0, 1.0, 0.0], [0.0; 3]];

fn source_contract(_: &drip::runtime::GlobalContext, _: &()) -> drip::Result<(RgbDesc,)> {
    Ok((RgbDesc {
        extent: Extent { width: 2, height: 2 },
        coordinates: Coordinates::Space(REC2020_D65),
    },))
}
#[drip::node(id="test.proof-source", name="display", category="test", contract=source_contract)]
pub fn display(_: &KernelContext<'_>, _: &(), image: Write<'_, Cpu<Rgb>>) -> drip::Result<()> {
    image.data.copy_from_slice(&PIXELS);
    Ok(())
}
