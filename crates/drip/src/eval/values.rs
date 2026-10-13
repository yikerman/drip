//! What a target hands its frontend: its evaluated inputs, or a reduction's
//! outputs, bound to the node's typed views or cut into regions and bands.
use super::run::FULL;
use crate::{
    Error, Result,
    definition::{Kind, Typed},
    image::Image,
    lattice::Rect,
    runtime::{KernelContext, RuntimeContext},
};

/// The samples of `image`, which is on a plane, whose cells meet `region`,
/// materialized. A region that covers the image yields the image itself.
pub(super) fn cut(image: &Image, region: Rect, kernels: &KernelContext<'_>) -> Result<Image> {
    let lattice = image.lattice().expect("regions cut images on a plane");
    let window = lattice
        .cover(region)
        .ok_or_else(|| Error::Contract("the region lies outside the image".into()))?;
    if lattice.with(window).is_whole() {
        image.materialize(kernels)?;
        return Ok(image.clone());
    }
    image.crop(window, kernels)
}

/// What a target hands its frontend: a reduce or bins node's outputs, any
/// other node's inputs.
pub enum TargetValues {
    Inputs(InputValues),
    Outputs(OutputValues),
}
impl TargetValues {
    /// The inputs. Fails for a reduce or bins target, which hands its
    /// outputs.
    pub fn inputs(&self) -> Result<&InputValues> {
        match self {
            Self::Inputs(inputs) => Ok(inputs),
            Self::Outputs(outputs) => Err(Error::Contract(format!(
                "{} hands its outputs, not its inputs",
                outputs.kind.id
            ))),
        }
    }

    /// [`InputValues::bind`]. Fails for a reduce or bins target.
    pub fn bind<N: Typed>(&self) -> Result<N::Inputs<'_>> {
        self.inputs()?.bind::<N>()
    }

    /// [`OutputValues::bind`]. Fails for any target but a reduce or bins
    /// node.
    pub fn bind_outputs<N: Typed>(&self) -> Result<N::Outputs<'_>> {
        match self {
            Self::Outputs(outputs) => outputs.bind::<N>(),
            Self::Inputs(inputs) => Err(Error::Contract(format!(
                "{} hands its inputs, not its outputs",
                inputs.kind.id
            ))),
        }
    }
}

/// A target's evaluated inputs, in its input order. An input on a plane
/// larger than the device budget stays deferred until it is bound or cut, so
/// an action can compute it in bands.
#[derive(Clone)]
pub struct InputValues {
    pub(super) kind: &'static Kind,
    pub(super) values: Vec<Option<Image>>,
    /// The runtime that materializes deferred inputs.
    pub(super) runtime: RuntimeContext,
}

/// A reduce or bins target's outputs, host values, in its output order.
pub struct OutputValues {
    pub(super) kind: &'static Kind,
    pub(super) outputs: Vec<Image>,
}
impl OutputValues {
    /// Typed views as declared by node type `N`, which must be the target's.
    pub fn bind<N: Typed>(&self) -> Result<N::Outputs<'_>> {
        check::<N>(self.kind)?;
        N::bind_outputs(&self.outputs)
    }
}

/// Fails unless `kind` is `N`'s.
fn check<N: Typed>(kind: &'static Kind) -> Result<()> {
    if !std::ptr::eq(kind, N::KIND) {
        return Err(Error::Contract(format!(
            "values of {} cannot bind as {}",
            kind.id,
            N::KIND.id
        )));
    }
    Ok(())
}

impl InputValues {
    /// Typed views as declared by node type `N`, which must be the target's.
    /// Deferred inputs are materialized whole first.
    pub fn bind<N: Typed>(&self) -> Result<N::Inputs<'_>> {
        check::<N>(self.kind)?;
        let kernels = KernelContext::new(&FULL, &self.runtime);
        for image in self.values.iter().flatten() {
            image.materialize(&kernels)?;
        }
        N::bind(&self.values)
    }

    /// The device budget of the evaluator's store, which bounds what
    /// [`bands`](Self::bands) holds.
    pub fn budget(&self) -> u64 {
        self.runtime.budget().device
    }

    /// The same inputs with each input on a plane holding only its samples
    /// whose cells meet `region` of its plane, in full-detail pixels, as
    /// [`Target::region`](super::Target::region) does. Fails where none do.
    pub fn region(&self, region: Rect) -> Result<InputValues> {
        let kernels = KernelContext::new(&FULL, &self.runtime);
        let cut = |image: &Image| match image.lattice() {
            Some(_) => cut(image, region, &kernels),
            None => Ok(image.clone()),
        };
        let values =
            self.values.iter().map(|v| v.as_ref().map(cut).transpose()).collect::<Result<_>>()?;
        Ok(InputValues { values, ..self.clone() })
    }

    /// Regions of whole rows of the inputs' plane, top to bottom, that cut
    /// each input on a plane into parts of at most `bytes`. One region covers
    /// the plane when every input fits, or when one stores other than one
    /// sample per full-detail pixel. Inputs must share their plane.
    pub fn bands(&self, bytes: u64) -> Vec<Rect> {
        let planes: Vec<_> = self
            .values
            .iter()
            .flatten()
            .filter_map(|image| Some((image.lattice()?, image.byte_len()? as u64)))
            .collect();
        let Some((lattice, _)) = planes.first() else { return Vec::new() };
        let [width, height] = lattice.full;
        let full = planes.iter().all(|(l, _)| l.extent == l.full && l.is_whole());
        let largest = planes.iter().map(|(_, bytes)| *bytes).max().unwrap_or(0);
        let rows = match full && largest > bytes {
            true => (bytes / largest.div_ceil(u64::from(height))).clamp(1, height.into()) as u32,
            false => height,
        };
        (0..height)
            .step_by(rows as usize)
            .map(|y| Rect { origin: [0, y], extent: [width, rows.min(height - y)] })
            .collect()
    }
}
