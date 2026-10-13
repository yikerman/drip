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

/// A target's evaluated inputs, in its input order, or a reduce or bins
/// target's outputs in their place. An input on a plane larger than the
/// device budget stays deferred until it is bound or cut, so an action can
/// compute it in bands.
pub struct InputValues {
    pub(super) kind: &'static Kind,
    /// `None` throughout for a reduction, whose inputs only its body reads.
    pub(super) values: Vec<Option<Image>>,
    /// A reduction's outputs, otherwise empty.
    pub(super) outputs: Vec<Image>,
    /// The runtime that materializes deferred inputs.
    pub(super) runtime: RuntimeContext,
}
impl InputValues {
    /// Typed views as declared by node type `N`; these must be `N`'s inputs.
    /// Deferred inputs are materialized whole first.
    pub fn bind<N: Typed>(&self) -> Result<N::Inputs<'_>> {
        self.check::<N>()?;
        let kernels = KernelContext::new(&FULL, &self.runtime);
        for image in self.values.iter().flatten() {
            image.materialize(&kernels)?;
        }
        N::bind(&self.values)
    }

    /// Typed views of a reduce or bins target's outputs as declared by node
    /// type `N`. Fails for any other target.
    pub fn bind_outputs<N: Typed>(&self) -> Result<N::Outputs<'_>> {
        self.check::<N>()?;
        N::bind_outputs(&self.outputs)
    }

    fn check<N: Typed>(&self) -> Result<()> {
        if !std::ptr::eq(self.kind, N::KIND) {
            return Err(Error::Contract(format!(
                "values of {} cannot bind as {}",
                self.kind.id,
                N::KIND.id
            )));
        }
        Ok(())
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
impl Clone for InputValues {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            values: self.values.clone(),
            outputs: self.outputs.clone(),
            runtime: self.runtime.clone(),
        }
    }
}
