use super::{RgbField, RgbFieldDesc, index_maps};
use crate::{
    Error, Result,
    node::{data::*, filter::Filter},
    payload::DeviceBuffer,
    ports::*,
    registry::tag,
    runtime::{GlobalContext, KernelContext, dispatch_dims},
};
pub use realize::{Definition, Ports, add, definition};

/// Footprints wider than this many source samples per output sample are
/// clamped, bounding taps per output sample; stronger reductions alias. A
/// prefiltered pyramid or EWA would lift the bound (FIELD.md, open questions).
pub const MAX_FOOTPRINT: f32 = 16.0;

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct Factor {
    /// Output samples per full-detail pixel of the field's domain.
    #[param(default = 1.0, range = 0.01..=16.0)]
    pub factor: f64,
}
#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
pub struct LongEdge {
    /// Output samples along the domain's longer side.
    #[param(default = 2048, range = 1..=65535)]
    pub pixels: i64,
}
#[derive(Clone, crate::Choice)]
pub enum Size {
    #[default]
    #[choice("factor")]
    #[label("Factor")]
    Factor(Factor),
    #[choice("long_edge")]
    #[label("Long edge")]
    LongEdge(LongEdge),
}

#[derive(Clone, serde::Serialize, serde::Deserialize, crate::Parameters)]
#[serde(deny_unknown_fields)]
#[validate(validate)]
pub struct Settings {
    /// Reconstruction and antialiasing filter, scaled by each sample's footprint.
    #[param]
    pub filter: Filter,
    /// Full-detail output size, relative to the field's domain or by its long edge.
    #[param]
    pub size: Size,
}
fn validate(p: &Settings) -> Result<()> {
    let valid = match &p.size {
        Size::Factor(f) => f.factor.is_finite() && (0.01..=16.0).contains(&f.factor),
        Size::LongEdge(l) => (1..=65535).contains(&l.pixels),
    };
    if !valid {
        return Err(Error::Contract("sample size is out of range".into()));
    }
    Ok(())
}

/// Full-detail output extent for a domain of `size` plane units.
fn full_extent(p: &Settings, [w, h]: [f64; 2]) -> Result<Extent> {
    let scale = match &p.size {
        Size::Factor(f) => f.factor,
        Size::LongEdge(l) => l.pixels as f64 / w.max(h),
    };
    let side = |v: f64| (v * scale).round().clamp(1.0, f64::from(u32::MAX)) as u32;
    let extent = Extent { width: side(w), height: side(h) };
    extent.validate()?;
    Ok(extent)
}

fn contract(global: &GlobalContext, p: &Settings, field: &RgbFieldDesc) -> Result<(RgbDesc,)> {
    let full = full_extent(p, field.domain.size)?;
    let extent = Extent {
        width: full.width.div_ceil(global.scale),
        height: full.height.div_ceil(global.scale),
    };
    Ok((RgbDesc { extent, full, coordinates: field.source.coordinates.clone() },))
}

/// Realize an RGB field as RGB samples: the one place where geometry is
/// resampled. Each output sample maps through the field's per-channel maps to
/// the source; the filter is centred there and widened by the local
/// footprint (the map's derivative), so magnifying interpolates and
/// minifying antialiases with the same filter [1]. Weights are normalized.
/// Samples mapping outside the source are black; taps beyond its edge repeat
/// the edge sample. Lanczos-3 (default) is sharp but rings: in scene-linear
/// HDR it leaves signed halos beside highlights, which are kept. Size scales
/// the field's domain, or fits its long edge to a pixel count; preview
/// detail stores proportionally fewer samples of the same framing.
/// Coordinates are kept.
#[crate::node(id = "sample", name = "Sample", tags = [tag::GEOMETRY], contract = contract, references = [
    ("[1] Heckbert, Fundamentals of texture mapping and image warping", "https://www2.eecs.berkeley.edu/Pubs/TechRpts/1989/5504.html"),
])]
pub fn realize(
    ctx: &KernelContext<'_>,
    p: &Settings,
    field: Read<'_, Device<RgbField>>,
    output: Write<'_, Device<Rgb>>,
) -> Result<()> {
    render(ctx, field.desc, field.data, p.filter, &output.desc.extent, output.data)
}

/// Sample `field` over its whole domain into `output`, `extent` samples. Also
/// serves frontends that preview a field without a Sample node.
pub fn render(
    ctx: &KernelContext<'_>,
    field: &RgbFieldDesc,
    samples: &DeviceBuffer<RgbField>,
    filter: Filter,
    extent: &Extent,
    output: &DeviceBuffer<Rgb>,
) -> Result<()> {
    let maps: Vec<f32> = index_maps(field, extent)
        .iter()
        .flat_map(|m| m.as_flattened())
        .map(|&v| v as f32)
        .collect();
    if maps.iter().any(|v| !v.is_finite()) {
        return Err(Error::Contract("field maps do not fit f32".into()));
    }
    let maps = ctx.constants(&maps)?;
    let (count, dim) = dispatch_dims(extent.pixels() * 3);
    kernel::sample::launch(
        ctx.client()?,
        count,
        dim,
        samples.argument(),
        maps.argument(),
        output.argument(),
        field.source.extent.width as usize,
        field.source.extent.height as usize,
        extent.width as usize,
        filter.id(),
        filter.support(),
    );
    Ok(())
}

mod kernel {
    use super::MAX_FOOTPRINT;
    use crate::node::filter::weight;
    use cubecl::prelude::*;

    /// One output channel per invocation. `maps` holds a 3×3 row-major map
    /// per channel from output sample indices to source sample indices.
    #[cube(launch)]
    pub fn sample(
        input: &[f32],
        maps: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
        out_width: usize,
        filter: u32,
        support: f32,
    ) {
        let i = ABSOLUTE_POS;
        if i < output.len() {
            let c = i % 3;
            let p = i / 3;
            let x = f32::cast_from(p % out_width);
            let y = f32::cast_from(p / out_width);
            let m = c * 9;
            let hw = maps[m + 6] * x + maps[m + 7] * y + maps[m + 8];
            let mut value = 0.0f32;
            if hw > 0.0f32 {
                let u = (maps[m] * x + maps[m + 1] * y + maps[m + 2]) / hw;
                let v = (maps[m + 3] * x + maps[m + 4] * y + maps[m + 5]) / hw;
                let last_u = f32::cast_from(width) - 0.5f32;
                let last_v = f32::cast_from(height) - 0.5f32;
                if u >= -0.5f32 && u <= last_u && v >= -0.5f32 && v <= last_v {
                    // Derivatives of the projective map: the source extent of
                    // one output step along each source axis sets the footprint.
                    let ux = (maps[m] - u * maps[m + 6]) / hw;
                    let uy = (maps[m + 1] - u * maps[m + 7]) / hw;
                    let vx = (maps[m + 3] - v * maps[m + 6]) / hw;
                    let vy = (maps[m + 4] - v * maps[m + 7]) / hw;
                    let su = f32::clamp((ux * ux + uy * uy).sqrt(), 1.0f32, MAX_FOOTPRINT);
                    let sv = f32::clamp((vx * vx + vy * vy).sqrt(), 1.0f32, MAX_FOOTPRINT);
                    let u0 = i32::cast_from((u - support * su).ceil());
                    let u1 = i32::cast_from((u + support * su).floor());
                    let v0 = i32::cast_from((v - support * sv).ceil());
                    let v1 = i32::cast_from((v + support * sv).floor());
                    let max_col = i32::cast_from(width) - 1i32;
                    let max_row = i32::cast_from(height) - 1i32;
                    // −0 is the additive identity, so an exact hit keeps −0.
                    let mut sum = f32::reinterpret(0x8000_0000u32);
                    let mut total = 0.0f32;
                    for b in v0..v1 + 1 {
                        let wy = weight(filter, (f32::cast_from(b) - v) / sv);
                        let row = usize::cast_from(i32::clamp(b, 0i32, max_row)) * width;
                        for a in u0..u1 + 1 {
                            let w = weight(filter, (f32::cast_from(a) - u) / su) * wy;
                            // Zero weights skip their tap, so non-finite
                            // neighbours of an exact hit cannot leak in.
                            if w != 0.0f32 {
                                let col = usize::cast_from(i32::clamp(a, 0i32, max_col));
                                sum += w * input[(row + col) * 3 + c];
                                total += w;
                            }
                        }
                    }
                    if total != 0.0f32 {
                        value = sum / total;
                    }
                }
            }
            output[i] = value;
        }
    }
}
