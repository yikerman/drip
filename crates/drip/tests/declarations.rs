//! Local concrete payloads and nodes need no central registration edits.
use drip::eval::{Evaluator, NodeError};
use drip::graph::{Graph, Port};
use drip::image::{Interpretation, RawMat, RealMat};
use drip::node::{EvalContext, KernelError};
use drip::param::{ParamKind, Parameters as _};
use drip::{Parameters, node};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug, Clone)]
pub struct Measurement {
    unit: u32,
}
impl Interpretation for Measurement {
    const CHANNELS: usize = 1;
    const NAME: &'static str = "Measurement";
}
type Samples = RealMat<1, Measurement>;

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
fn source(#[params] p: Source) -> Result<(Arc<RealMat<1, Measurement>>,), KernelError> {
    let width = p.geometry.width as usize;
    Ok((Arc::new(RealMat::new(
        Arc::new(RawMat { width, height: 1, scale: 1, pixels: vec![[1.0]; width] }),
        Measurement { unit: p.unit as u32 },
    )),))
}

fn compatible(a: &Samples, b: &Samples) -> Result<(), String> {
    if (a.buffer().width, a.buffer().height, a.buffer().scale)
        != (b.buffer().width, b.buffer().height, b.buffer().scale)
    {
        return Err("sample geometries differ".into());
    }
    if a.interpretation().unit != b.interpretation().unit {
        return Err("measurement units differ".into());
    }
    Ok(())
}
static EVALUATIONS: AtomicUsize = AtomicUsize::new(0);
static ACTIONS: AtomicUsize = AtomicUsize::new(0);
fn action(_: (), (a, b): (&Samples, &Samples), _: &EvalContext<'_>) -> Result<(), KernelError> {
    compatible(a, b)?;
    ACTIONS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}
#[node(kind = SUM, id = "contract.sum", category = "test", name = "Sum", outputs = ["samples"],
    actions = [("record", action)])]
fn sum(a: &Samples, b: &Samples) -> Result<(Arc<Samples>,), KernelError> {
    compatible(a, b)?;
    EVALUATIONS.fetch_add(1, Ordering::Relaxed);
    let pixels =
        a.buffer().pixels.iter().zip(&b.buffer().pixels).map(|(a, b)| [a[0] + b[0]]).collect();
    Ok((Arc::new(Samples::try_new(
        Arc::new(RawMat { pixels, ..**a.buffer() }),
        a.interpretation().clone(),
    )?),))
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
    let mut eval = Evaluator::default();
    eval.evaluate(&graph, 0, &[sum]);
    assert!(
        matches!(eval.result(sum).unwrap(), Err(NodeError::Failed(error)) if error == "measurement units differ")
    );
    assert!(matches!(eval.fork().run_action(&graph, sum, "record"), Err(NodeError::Failed(_))));
    assert_eq!(EVALUATIONS.load(Ordering::Relaxed), 0);
    assert_eq!(ACTIONS.load(Ordering::Relaxed), 0);
    graph.set_param(b, "unit", 0.into()).unwrap();
    graph.set_param(b, "width", 3.into()).unwrap();
    eval.evaluate(&graph, 0, &[sum]);
    assert!(
        matches!(eval.result(sum).unwrap(), Err(NodeError::Failed(error)) if error == "sample geometries differ")
    );
    graph.set_param(b, "width", 2.into()).unwrap();
    eval.evaluate(&graph, 0, &[sum]);
    let result = eval.result(sum).unwrap().as_ref().unwrap();
    assert_eq!(result[0].downcast_ref::<RealMat<1, Measurement>>().unwrap().samples(), &[2.0, 2.0]);
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

#[node(kind = OBSERVER, id = "contract.observer", category = "test", name = "Observer", outputs = [])]
fn observer(samples: &Samples) -> Result<(), KernelError>;

#[test]
fn declaration_only_nodes_provide_validated_inputs_without_computation() {
    let mut graph = Graph::default();
    let source = graph.add_node(&SOURCE);
    let observer = graph.add_node(&OBSERVER);
    graph.connect(Port(source, "samples".into()), Port(observer, "samples".into())).unwrap();
    let mut evaluator = Evaluator::default();
    let samples = evaluator
        .with_inputs(&graph, observer, 0, &OBSERVER, |_, (samples,), _| {
            Ok(samples.buffer().pixels.clone())
        })
        .unwrap();
    assert_eq!(samples, vec![[1.0], [1.0]]);
    assert!(evaluator.result(observer).is_none());
}

#[test]
fn input_consumers_reject_a_different_declaration() {
    let mut graph = Graph::default();
    let source = graph.add_node(&SOURCE);
    let mut evaluator = Evaluator::default();
    let error = evaluator.with_inputs(&graph, source, 0, &OBSERVER, |_, _, _| Ok(())).unwrap_err();
    assert!(
        matches!(error, NodeError::Failed(message) if message.contains("different node declaration"))
    );
}

#[test]
fn consumer_failure_does_not_poison_the_next_request() {
    let mut graph = Graph::default();
    let source = graph.add_node(&SOURCE);
    let observer = graph.add_node(&OBSERVER);
    graph.connect(Port(source, "samples".into()), Port(observer, "samples".into())).unwrap();
    let mut evaluator = Evaluator::default();
    let result = evaluator.with_inputs(&graph, observer, 0, &OBSERVER, |_, _, _| {
        Err::<(), _>(KernelError::Failed("consumer failed".into()))
    });
    assert_eq!(result, Err(NodeError::Failed("consumer failed".into())));
    let length = evaluator
        .with_inputs(&graph, observer, 0, &OBSERVER, |_, (samples,), _| Ok(samples.samples().len()))
        .unwrap();
    assert_eq!(length, 2);
}

#[test]
#[should_panic(expected = "outputs require a kernel")]
fn declarations_cannot_advertise_outputs_without_a_kernel() {
    struct NoKernel;
    impl drip::node::NodeDeclaration for NoKernel {
        type Parameters = ();
        type Inputs = ();
        type Outputs = (Arc<RealMat<1, Measurement>>,);
    }
    let _ =
        drip::node::NodeKind::new::<NoKernel>("test.invalid", "test", "Invalid", &[], &["samples"]);
}

/// Inline scalar parameters require no separately maintained parameter record.
#[node(kind = SCALE, id = "contract.scale", category = "test", name = "Scale", outputs = ["samples"])]
fn scale(
    samples: &Samples,
    #[param(ParamKind::Float { min: 0.0, max: 4.0, default: 2.0 })] gain: f32,
) -> Result<Arc<Samples>, KernelError> {
    Ok(Arc::new(Samples::try_new(
        Arc::new(RawMat {
            pixels: samples.pixels.iter().map(|p| [p[0] * gain]).collect(),
            ..**samples.buffer()
        }),
        samples.interpretation().clone(),
    )?))
}

#[test]
fn inline_parameter_and_single_output_have_generated_bindings() {
    let mut graph = Graph::default();
    let source = graph.add_node(&SOURCE);
    let scale = graph.add_node(&SCALE);
    graph.connect(Port(source, "samples".into()), Port(scale, "samples".into())).unwrap();
    assert_eq!(ScaleParameters::SPECS.len(), 1);
    assert_eq!(SCALE.inputs().next().unwrap().name, "samples");
    let mut evaluator = Evaluator::default();
    evaluator.evaluate(&graph, 0, &[scale]);
    assert_eq!(
        evaluator.result(scale).unwrap().as_ref().unwrap()[0]
            .downcast_ref::<Samples>()
            .unwrap()
            .samples(),
        &[2.0, 2.0]
    );
    graph.set_param(scale, "gain", 3.0.into()).unwrap();
    evaluator.evaluate(&graph, 0, &[scale]);
    assert_eq!(
        evaluator.result(scale).unwrap().as_ref().unwrap()[0]
            .downcast_ref::<Samples>()
            .unwrap()
            .samples(),
        &[3.0, 3.0]
    );
}
