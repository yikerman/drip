//! How evaluation runs a node kind.
//!
//! Every [`Kind`](crate::definition::Kind) declares a [`Shape`], which
//! `#[node]` derives from the declaration. Evaluation reads it to decide
//! whether a node runs on whole images, publishes a deferred image or
//! expands into a sub-graph, at which scale, and how the store keeps its
//! outputs. Node functions never name it.
use crate::{
    Error, Result,
    body::{BinsBody, MapBody, ReduceBody, StencilBody},
    image::{Image, Raster},
    payload::Persist,
};
use std::io::{Read, Write};

/// How evaluation runs a node kind. `Kernel`, `Source` and `Foreign` are
/// opaque and receive and produce whole images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// The node's function runs on whole materialized inputs whenever a
    /// demanded output is not stored, and each output stays where it wrote it.
    /// With `scaled`, `#[node(scaled)]`, the kernel produces its outputs at
    /// the requested scale itself, as demosaic does. Otherwise it sees scale
    /// 1 and processes the samples it receives.
    Kernel { scaled: bool },
    /// A value bound in memory (`node::source`), handed to each run.
    Source,
    /// The node's host function runs on a thread of its own, on host samples
    /// of its inputs, without a scale, and describes its outputs when it
    /// publishes them. The store keeps the outputs, or the failure, under the
    /// node's key, so the function runs once per key while they stay stored.
    Foreign(Cache),
    /// The node's host function returns constants for its body, and its
    /// output is a deferred image. A consumer that needs samples runs every
    /// map since the last raster in one kernel.
    Map(&'static MapBody),
    /// The node's host function returns a [`Warp`](crate::body::Warp), and its output is a
    /// deferred image resampled once, together with neighbouring warps.
    Warp,
    /// The node's host function returns a [`FilterBody`](crate::body::FilterBody), and its output is
    /// its input resampled with that filter onto the output's lattice.
    Resample,
    /// The node's host function returns constants for the body, and its
    /// output is a host value: the body's accumulator over every pixel of its
    /// input, computed in the input's segment without storing it.
    Reduce(&'static ReduceBody),
    /// As `Reduce`, with the output the number of pixels in each bin per
    /// channel.
    Bins(&'static BinsBody),
    /// The node's host function returns a [`Stencil`](crate::body::Stencil): the reach of the body
    /// in full-detail pixels and its constants. The output is a deferred
    /// image whose segment reads the materialized input through a [`Window`](crate::body::Window)
    /// around each sample, so a window of the output reads that window grown
    /// by the reach.
    Stencil(&'static StencilBody),
    /// The node's host function wires a [`SubGraph`](crate::expand::SubGraph)
    /// of other kinds to its ports, from its parameters and input
    /// descriptions. Evaluation runs the sub-graph's nodes in its place at
    /// the scale the node runs at, keyed like any node's, and the node's
    /// outputs are the sub-graph outputs mapped to its ports.
    Meta,
}
impl Shape {
    /// Whether the kind's contract reads the requested scale to choose its
    /// output lattice: a `scaled` kernel, a warp or a resample. Evaluation
    /// keys such a node by the scale and hands every other node scale 1.
    pub fn scaled(self) -> bool {
        matches!(self, Self::Kernel { scaled: true } | Self::Warp | Self::Resample)
    }

    /// Whether the kind reduces its input to a host value, `Reduce` or
    /// `Bins`. A frontend consumes such a node's outputs.
    pub fn reduces(self) -> bool {
        matches!(self, Self::Reduce(_) | Self::Bins(_))
    }
}

/// Where the store keeps a foreign node's outputs beside memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cache {
    /// In the evaluator's memory only, like computed outputs.
    Memory,
    /// Also on disk, one [`Codec`] per output in port order, so another
    /// evaluator or process reads them instead of running the node.
    Disk(&'static [Codec]),
}

/// How the store writes and reads one output of a [`Persist`] payload.
/// `#[node(foreign = disk)]` generates one per output.
#[derive(Clone, Copy)]
pub struct Codec {
    /// The payload's name. Codecs compare by it.
    pub payload: &'static str,
    pub(crate) write: fn(&Image, &mut dyn Write) -> Result<()>,
    pub(crate) read: fn(&mut dyn Read) -> Result<Image>,
}
impl Codec {
    pub const fn of<P: Persist>() -> Self {
        Self { payload: P::NAME, write: write::<P>, read: read::<P> }
    }
}
fn write<P: Persist>(image: &Image, out: &mut dyn Write) -> Result<()> {
    let raster = image
        .downcast::<P>()
        .ok_or_else(|| Error::Contract(format!("image is not {}", P::NAME)))?;
    P::write(raster.desc(), raster.host()?, out).map_err(|e| Error::Runtime(e.to_string()))
}
/// Fails unless the stream holds a valid description and samples that agree
/// with it.
fn read<P: Persist>(input: &mut dyn Read) -> Result<Image> {
    let (desc, host) = P::read(input).map_err(|e| Error::Runtime(e.to_string()))?;
    P::validate(&desc)?;
    Ok(Raster::<P>::from_host(desc, host)?.into())
}
impl PartialEq for Codec {
    fn eq(&self, other: &Self) -> bool {
        self.payload == other.payload
    }
}
impl Eq for Codec {}
impl std::fmt::Debug for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.payload)
    }
}
