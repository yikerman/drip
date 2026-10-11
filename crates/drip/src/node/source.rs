//! Immutable, already loaded source values.
use crate::{
    Result,
    definition::{Metadata, Node, Outcome},
    graph::Dag,
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
    let data = data.into();
    P::validate(&desc)?;
    P::check(&desc, &data)?;
    let id = dag.add(Arc::new(Source::<P> { desc, data: Arc::new(data) }))?;
    Ok(Output::new(id, 0))
}
struct Source<P: Payload> {
    desc: P::Desc,
    data: Arc<P::Host>,
}
impl<P: Payload> Node for Source<P> {
    fn metadata(&self) -> Metadata {
        Metadata {
            id: "bound-source",
            name: "Bound source",
            category: "Input",
            help: "Immutable, validated source value.",
            references: &[],
            parameters: &[],
        }
    }
    fn inputs(&self) -> Vec<PortSpec> {
        vec![]
    }
    fn outputs(&self) -> Vec<PortSpec> {
        vec![PortSpec::of::<Cpu<P>, Any>("value")]
    }
    fn contract(&self, _: &GlobalContext, _: &[Option<Description>]) -> Result<Outcome> {
        Ok(Outcome::Known(vec![Arc::new(self.desc.clone())]))
    }
    fn run(
        &self,
        _: &KernelContext<'_>,
        _: &[Option<(Description, Data)>],
        _: &[Description],
    ) -> Result<Vec<Data>> {
        Ok(vec![self.data.clone()])
    }
}
