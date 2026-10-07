//! A new interpretation, capability and node need no central registration edits.
use bevy_reflect::Reflect;
use drip::eval::{Evaluator, NodeError};
use drip::graph::{Graph, Port};
use drip::image::{Linearity, RawMat, RealMat};
use drip::node::{EvalContext, Evaluated, KernelError};
use drip::param::{ParamKind, Parameters as _};
use drip::ports::{MatRef, Preserved};
use drip::{Parameters, capability, interpretation, node};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[capability]
trait Measured: Linearity {
    fn unit(&self) -> u32;
}
#[interpretation(Measured)]
#[derive(Debug, Reflect)]
struct Measurement {
    unit: u32,
}
impl Linearity for Measurement {}
impl Measured for Measurement {
    fn unit(&self) -> u32 {
        self.unit
    }
}

#[derive(Parameters)]
struct Geometry {
    /// Number of samples in a row.
    #[param(ParamKind::Int { min: 1, max: 16, default: 2 })]
    width: i64,
}
#[derive(Parameters)]
pub struct Source {
    #[param(flatten)]
    geometry: Geometry,
    #[param(ParamKind::Int { min: 0, max: 10, default: 0 })]
    unit: i64,
}

/// Provides a measurement with an explicitly declared unit.
#[node(kind = SOURCE, id = "contract.source", category = "test", name = "Source", outputs = ["samples"], references = [("Units", "https://example.org/units")])]
fn source(
    p: Source,
    (): (),
    _: &EvalContext<'_>,
) -> Result<Evaluated<(Arc<RealMat<1, Measurement>>,)>, KernelError> {
    let width = p.geometry.width as usize;
    Ok(Evaluated::new((Arc::new(RealMat::new(
        Arc::new(RawMat { width, height: 1, scale: 1, pixels: vec![[1.0]; width] }),
        Measurement { unit: p.unit as u32 },
    )),)))
}

fn compatible(
    _: (),
    (a, b): (MatRef<'_, 1, dyn Measured>, MatRef<'_, 1, dyn Measured>),
    _: &EvalContext<'_>,
) -> Result<(), String> {
    if (a.buffer().width, a.buffer().height, a.buffer().scale)
        != (b.buffer().width, b.buffer().height, b.buffer().scale)
    {
        return Err("sample geometries differ".into());
    }
    if a.interpretation.unit() != b.interpretation.unit() {
        return Err("measurement units differ".into());
    }
    Ok(())
}
static EVALUATIONS: AtomicUsize = AtomicUsize::new(0);
static ACTIONS: AtomicUsize = AtomicUsize::new(0);
fn action(
    _: (),
    _: (MatRef<'_, 1, dyn Measured>, MatRef<'_, 1, dyn Measured>),
    _: &EvalContext<'_>,
) -> Result<(), KernelError> {
    ACTIONS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}
#[node(kind = SUM, id = "contract.sum", category = "test", name = "Sum", outputs = ["samples"],
    checks = [("matching geometry and units", compatible)], actions = [("record", action)])]
fn sum(
    _: (),
    (a, b): (MatRef<'_, 1, dyn Measured>, MatRef<'_, 1, dyn Measured>),
    _: &EvalContext<'_>,
) -> Result<Evaluated<(Preserved<0>,)>, KernelError> {
    EVALUATIONS.fetch_add(1, Ordering::Relaxed);
    let pixels =
        a.buffer().pixels.iter().zip(&b.buffer().pixels).map(|(a, b)| [a[0] + b[0]]).collect();
    Ok(Evaluated::new((a.preserve::<0>(pixels),)))
}

#[test]
fn runtime_relationships_gate_both_evaluation_and_actions_and_recheck_edits() {
    let mut graph = Graph::default();
    let a = graph.add_node(&SOURCE);
    let b = graph.add_node(&SOURCE);
    let sum = graph.add_node(&SUM);
    graph.set_param(b, "unit", 1.into()).unwrap();
    graph.connect(Port(a, "samples".into()), Port(sum, "a".into())).unwrap();
    graph.connect(Port(b, "samples".into()), Port(sum, "b".into())).unwrap();
    assert_eq!(
        graph.output_type(&Port(a, "samples".into())),
        graph.output_type(&Port(b, "samples".into()))
    );
    assert_eq!(SUM.checks().collect::<Vec<_>>(), ["matching geometry and units"]);
    let mut eval = Evaluator::default();
    eval.evaluate(&graph, 0, &[sum]);
    assert!(
        matches!(eval.result(sum).unwrap(), Err(NodeError::Constraint(error)) if error.detail == "measurement units differ")
    );
    assert!(matches!(eval.fork().run_action(&graph, sum, "record"), Err(NodeError::Constraint(_))));
    assert_eq!(EVALUATIONS.load(Ordering::Relaxed), 0);
    assert_eq!(ACTIONS.load(Ordering::Relaxed), 0);
    graph.set_param(b, "unit", 0.into()).unwrap();
    graph.set_param(b, "width", 3.into()).unwrap();
    eval.evaluate(&graph, 0, &[sum]);
    assert!(
        matches!(eval.result(sum).unwrap(), Err(NodeError::Constraint(error)) if error.detail == "sample geometries differ")
    );
    graph.set_param(b, "width", 2.into()).unwrap();
    eval.evaluate(&graph, 0, &[sum]);
    let result = eval.result(sum).unwrap().as_ref().unwrap();
    assert_eq!(
        result.outputs[0].downcast_ref::<RealMat<1, Measurement>>().unwrap().samples(),
        &[2.0, 2.0]
    );
    assert_eq!(EVALUATIONS.load(Ordering::Relaxed), 1);
    eval.fork().run_action(&graph, sum, "record").unwrap();
    assert_eq!(ACTIONS.load(Ordering::Relaxed), 1);
}

#[test]
fn fields_supply_flat_schema_defaults_and_documentation() {
    let mut graph = Graph::default();
    let id = graph.add_node(&SOURCE);
    assert_eq!(Source::SPECS.iter().map(|s| s.name).collect::<Vec<_>>(), ["width", "unit"]);
    assert_eq!(graph.node(id).unwrap().params["width"], 2);
    assert!(graph.set_param(id, "width", 17.into()).is_err());
    assert_eq!(SOURCE.params[0].documentation, "Number of samples in a row.");
    assert!(SOURCE.documentation.contains("explicitly declared unit"));
    assert_eq!(SOURCE.references, &[("Units", "https://example.org/units")]);
    assert!(drip::nodes::registry().get(SOURCE.id).is_some());
}

#[node(kind = WRONG_SOURCE, id = "contract.wrong_source", category = "test", name = "Wrong source", outputs = ["samples"])]
fn wrong_source(
    _: (),
    (a, b): (MatRef<'_, 1, dyn Measured>, MatRef<'_, 1, dyn Measured>),
    _: &EvalContext<'_>,
) -> Result<Evaluated<(Preserved<0>,)>, KernelError> {
    let _ = a;
    Ok(Evaluated::new((b.preserve::<0>(b.buffer().pixels.clone()),)))
}
#[test]
fn preservation_checks_the_actual_input_witness_even_for_the_same_nominal_type() {
    let mut graph = Graph::default();
    let a = graph.add_node(&SOURCE);
    let b = graph.add_node(&SOURCE);
    graph.set_param(b, "unit", 1.into()).unwrap();
    let wrong = graph.add_node(&WRONG_SOURCE);
    graph.connect(Port(a, "samples".into()), Port(wrong, "a".into())).unwrap();
    graph.connect(Port(b, "samples".into()), Port(wrong, "b".into())).unwrap();
    let mut eval = Evaluator::default();
    eval.evaluate(&graph, 0, &[wrong]);
    assert!(
        matches!(eval.result(wrong).unwrap(), Err(NodeError::Failed(message)) if message.contains("preservation"))
    );
}

#[node(kind = OPTIONAL_VIEW, id = "contract.optional_view", category = "test", name = "Optional view", outputs = [])]
fn optional_view(
    _: (),
    (): (),
    _: &EvalContext<'_>,
) -> Result<Evaluated<(), Option<drip::view::PreviewImage>>, KernelError> {
    Ok(Evaluated::view(None))
}

#[test]
fn presentation_availability_follows_the_return_type_even_without_data() {
    assert!(!SOURCE.has_view());
    assert!(drip::nodes::PREVIEW.has_view());
    assert!(OPTIONAL_VIEW.has_view());
    let mut graph = Graph::default();
    let id = graph.add_node(&OPTIONAL_VIEW);
    let mut evaluator = Evaluator::default();
    evaluator.evaluate(&graph, 0, &[id]);
    assert!(evaluator.result(id).unwrap().as_ref().unwrap().view.is_none());
}
