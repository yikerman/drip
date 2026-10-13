//! Continuous RGB fields: stored samples plus deferred coordinate maps.
//! Field nodes only extend the description; Sample filters once. Design and
//! references: `agent-docs/FIELD.md`.
use crate::{
    Error, Result,
    node::{
        color::{self, Mat3},
        data::*,
    },
    payload::DeviceLayout,
};
use cubecl::bytes::Bytes;

pub mod interpolate;
pub mod perspective;
pub mod sample;

/// An axis-aligned rectangle in plane units (full-detail pixels).
#[derive(Clone, Debug, PartialEq)]
pub struct Rect {
    pub origin: [f64; 2],
    pub size: [f64; 2],
}

/// One deferred map from a field's plane to its input's plane.
#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    /// Homogeneous `[x, y, 1]` columns; points with `w ≤ 0` have no source.
    /// Sample tests `w` of the composed chain, which matches testing every
    /// stage unless two stages both put their horizon inside the frame.
    Projective(Mat3),
}

/// Maps from the output plane to the source plane, newest stage first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chain(pub Vec<Stage>);
impl Chain {
    /// This chain after a newer stage that maps a new output plane into it.
    pub fn after(&self, stage: Stage) -> Self {
        Self(std::iter::once(stage).chain(self.0.iter().cloned()).collect())
    }
    /// The whole chain as one matrix. Exact composition while every stage is
    /// projective.
    pub fn matrix(&self) -> Mat3 {
        self.0.iter().fold(IDENTITY, |acc, Stage::Projective(m)| color::mul(m, &acc))
    }
}

pub const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// `source` holds the stored samples and their color meaning; `domain` is
/// where the field is defined in its own plane; `maps` take that plane to the
/// source plane per channel, so channels may be displaced independently.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbFieldDesc {
    pub source: RgbDesc,
    pub domain: Rect,
    pub maps: [Chain; 3],
}

/// An RGB field: storage is the source's packed samples, unchanged.
pub struct RgbField;
impl Payload for RgbField {
    const NAME: &'static str = "RGB field";
    type Desc = RgbFieldDesc;
    type Host = HostBuffer<[f32; 3]>;
    fn validate(d: &RgbFieldDesc) -> Result<()> {
        Rgb::validate(&d.source)?;
        let [w, h] = d.domain.size;
        if !(d.domain.origin.iter().all(|v| v.is_finite()) && w > 0.0 && h > 0.0)
            || !(w.is_finite() && h.is_finite())
        {
            return Err(Error::Contract("field domain must be finite and nonempty".into()));
        }
        let finite = |m: &Mat3| m.as_flattened().iter().all(|v| v.is_finite());
        if !d.maps.iter().flat_map(|c| &c.0).all(|Stage::Projective(m)| finite(m)) {
            return Err(Error::Contract("field maps must be finite".into()));
        }
        Ok(())
    }
    fn check(d: &RgbFieldDesc, host: &Self::Host) -> Result<()> {
        Rgb::check(&d.source, host)
    }
    fn allocate(d: &RgbFieldDesc) -> Self::Host {
        Rgb::allocate(&d.source)
    }
}
impl DeviceLayout for RgbField {
    fn byte_len(d: &RgbFieldDesc) -> usize {
        Rgb::byte_len(&d.source)
    }
    fn encode(d: &RgbFieldDesc, host: &Self::Host) -> Result<Bytes> {
        Rgb::encode(&d.source, host)
    }
    fn decode(d: &RgbFieldDesc, bytes: Bytes) -> Result<Self::Host> {
        Rgb::decode(&d.source, bytes)
    }
}

/// Per channel, the map from output sample indices to source sample indices
/// for `output` samples over the field's domain. Index `i` sits at plane
/// `(i + 0.5) · full / extent`; folding both conversions into the chain keeps
/// the kernel to one projective map and keeps identities exact in f64.
pub fn index_maps(field: &RgbFieldDesc, output: &Extent) -> [Mat3; 3] {
    let Rect { origin: [ox, oy], size: [w, h] } = field.domain;
    let (sx, sy) = (w / f64::from(output.width), h / f64::from(output.height));
    let to_plane = [[sx, 0.0, ox + 0.5 * sx], [0.0, sy, oy + 0.5 * sy], [0.0, 0.0, 1.0]];
    let source = &field.source;
    let rx = f64::from(source.extent.width) / f64::from(source.full.width);
    let ry = f64::from(source.extent.height) / f64::from(source.full.height);
    let to_index = [[rx, 0.0, -0.5], [0.0, ry, -0.5], [0.0, 0.0, 1.0]];
    field
        .maps
        .each_ref()
        .map(|chain| color::mul(&to_index, &color::mul(&chain.matrix(), &to_plane)))
}
