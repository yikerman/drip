//! Immutable, already loaded source values.
use crate::{
    Result,
    definition::{Kind, Node, Shape},
    graph::Dag,
    image::{Description, Image, Raster},
    payload::Payload,
    ports::*,
    runtime::{GlobalContext, KernelContext},
};
use std::sync::Arc;
/// Bind immutable, already loaded data. Loading and decode remain outside pure nodes.
pub fn source<P: Payload>(
    dag: &mut Dag,
    desc: P::Desc,
    data: impl Into<P::Host>,
) -> Result<Output<P>> {
    P::validate(&desc)?;
    let raster = Raster::<P>::from_host(desc, data.into())?;
    let id = dag.add(Arc::new(Source { raster }))?;
    Ok(Output::new(id, 0))
}
struct Source<P: Payload> {
    raster: Raster<P>,
}
impl<P: Payload> Source<P> {
    const KIND: Kind = Kind {
        id: "bound-source",
        name: "Bound source",
        tags: &[],
        help: "Immutable, validated source value.",
        references: &[],
        parameters: &[],
        defaults: || serde_json::Value::Null,
        build: |_| Err(crate::Error::Graph("bound sources have no persistent parameters".into())),
        inputs: &[],
        outputs: &[PortSpec::of::<P, Any>("value")],
        shape: Shape::Source,
        action: None,
    };
}
impl<P: Payload> Node for Source<P> {
    fn kind(&self) -> &'static Kind {
        &Self::KIND
    }
    fn parameters(&self) -> Result<serde_json::Value> {
        Err(crate::Error::Graph("this bound value has no persistent source definition".into()))
    }
    fn contract(&self, _: &GlobalContext, _: &[Option<Description>]) -> Result<Vec<Description>> {
        Ok(vec![self.raster.description()])
    }
    fn run(
        &self,
        _: &KernelContext<'_>,
        _: &[Option<Image>],
        _: &[Description],
    ) -> Result<Vec<Image>> {
        Ok(vec![self.raster.share_host()?.into()])
    }
}
