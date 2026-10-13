//! Bound sources: values loaded outside the graph, bound to a node with
//! [`source`] and handed unchanged to each run.
//!
//! A bound source is infrastructure, a hand-written [`Node`] with
//! [`Shape::Source`]. Its public path is `node::source`, beside the nodes
//! that consume it.
use crate::{
    Result,
    definition::{Identity, Kind, Node},
    graph::Dag,
    image::{Description, Image, Raster},
    payload::Payload,
    ports::*,
    runtime::{GlobalContext, KernelContext},
    shape::Shape,
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
    let id = dag.add(Arc::new(Source { raster, identity: Identity::bound() }))?;
    Ok(Output::new(id, 0))
}
struct Source<P: Payload> {
    raster: Raster<P>,
    /// Bound data has no persistent definition to key it by.
    identity: Identity,
}
impl<P: Payload> Source<P> {
    const KIND: Kind = Kind {
        id: "bound-source",
        version: 1,
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
    fn identity(&self) -> Result<Identity> {
        Ok(self.identity.clone())
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
