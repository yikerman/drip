//! Immutable, already loaded source values.
use crate::{
    Result,
    definition::{Kind, Node},
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
impl<P: Payload> Source<P> {
    const KIND: Kind = Kind {
        id: "bound-source",
        name: "Bound source",
        category: "Input",
        help: "Immutable, validated source value.",
        references: &[],
        parameters: &[],
        defaults: || serde_json::Value::Null,
        loads_assets: false,
        build: |_| Err(crate::Error::Graph("bound sources have no persistent parameters".into())),
        inputs: &[],
        outputs: &[PortSpec::of::<Cpu<P>, Any>("value")],
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
        Ok(vec![Arc::new(self.desc.clone())])
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
