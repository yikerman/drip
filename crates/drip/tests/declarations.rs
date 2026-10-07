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
fn source(
    p: Source,
    (): (),
    _: &EvalContext<'_>,
) -> Result<(Arc<RealMat<1, Measurement>>,), KernelError> {
    let width = p.geometry.width as usize;
    Ok((Arc::new(RealMat::new(
        Arc::new(RawMat { width, height: 1, scale: 1, pixels: vec![[1.0]; width] }),
        Measurement { unit: p.unit as u32 },
    )),))
}

fn compatible(_: (), (a, b): (&Samples, &Samples), _: &EvalContext<'_>) -> Result<(), String> {
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
fn action(_: (), _: (&Samples, &Samples), _: &EvalContext<'_>) -> Result<(), KernelError> {
    ACTIONS.fetch_add(1, Ordering::Relaxed);
    Ok(())
}
#[node(kind = SUM, id = "contract.sum", category = "test", name = "Sum", outputs = ["samples"],
    checks = [("matching geometry and units", compatible)], actions = [("record", action)])]
fn sum(
    _: (),
    (a, b): (&Samples, &Samples),
    _: &EvalContext<'_>,
) -> Result<(Arc<Samples>,), KernelError> {
    EVALUATIONS.fetch_add(1, Ordering::Relaxed);
    let pixels =
        a.buffer().pixels.iter().zip(&b.buffer().pixels).map(|(a, b)| [a[0] + b[0]]).collect();
    Ok((Arc::new(a.with_buffer(Arc::new(RawMat { pixels, ..**a.buffer() }))),))
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
fn observer(_: (), (samples,): (&Samples,), _: &EvalContext<'_>) -> Result<(), KernelError>;

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
fn consumer_failures_leave_computational_results_reusable() {
    let mut graph = Graph::default();
    let source = graph.add_node(&SOURCE);
    let observer = graph.add_node(&OBSERVER);
    graph.connect(Port(source, "samples".into()), Port(observer, "samples".into())).unwrap();
    let mut evaluator = Evaluator::default();
    let result = evaluator.with_inputs(&graph, observer, 0, &OBSERVER, |_, _, _| {
        Err::<(), _>(KernelError::Failed("consumer failed".into()))
    });
    assert_eq!(result, Err(NodeError::Failed("consumer failed".into())));
    assert!(evaluator.result(source).unwrap().is_ok());
    assert!(evaluator.result(observer).is_none());
    let revision = evaluator.revision(source);
    evaluator.with_inputs(&graph, observer, 0, &OBSERVER, |_, _, _| Ok(())).unwrap();
    assert_eq!(evaluator.revision(source), revision);
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

mod observed_kernel {
    use super::*;

    static CHECKS: AtomicUsize = AtomicUsize::new(0);
    static KERNEL_CALLS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Parameters)]
    pub struct Settings {
        #[param(ParamKind::Bool { default: true })]
        fail: bool,
    }

    fn check(
        _: Settings,
        inputs: (&Samples, &Samples),
        ctx: &EvalContext<'_>,
    ) -> Result<(), String> {
        CHECKS.fetch_add(1, Ordering::Relaxed);
        compatible((), inputs, ctx)
    }

    #[node(kind = PROBE, id = "contract.observed_kernel", category = "test", name = "Observed kernel", outputs = ["samples"], checks = [("matching geometry and units", check)])]
    fn probe(
        settings: Settings,
        (a, b): (&Samples, &Samples),
        _: &EvalContext<'_>,
    ) -> Result<(Arc<Samples>,), KernelError> {
        KERNEL_CALLS.fetch_add(1, Ordering::Relaxed);
        let _ = b;
        if settings.fail {
            return Err("kernel failed".into());
        }
        Ok((Arc::new(a.clone()),))
    }

    #[test]
    fn input_observation_skips_the_kernel_and_reuses_successful_contract_checks() {
        let mut graph = Graph::default();
        let a = graph.add_node(&SOURCE);
        let b = graph.add_node(&SOURCE);
        let probe = graph.add_node(&PROBE);
        graph.connect(Port(a, "samples".into()), Port(probe, "a".into())).unwrap();
        graph.connect(Port(b, "samples".into()), Port(probe, "b".into())).unwrap();
        let mut evaluator = Evaluator::default();
        evaluator
            .with_inputs(&graph, probe, 0, &PROBE, |_, (a, _), _| {
                assert_eq!(a.buffer().pixels, vec![[1.0], [1.0]]);
                Ok(())
            })
            .unwrap();
        assert_eq!(CHECKS.load(Ordering::Relaxed), 1);
        assert_eq!(KERNEL_CALLS.load(Ordering::Relaxed), 0);
        assert!(evaluator.result(probe).is_none());

        evaluator.evaluate(&graph, 0, &[probe]);
        assert!(
            matches!(evaluator.result(probe), Some(Err(NodeError::Failed(error))) if error == "kernel failed")
        );
        evaluator.with_inputs(&graph, probe, 0, &PROBE, |_, _, _| Ok(())).unwrap();
        assert_eq!(
            KERNEL_CALLS.load(Ordering::Relaxed),
            1,
            "a cached kernel failure neither blocks input observation nor reruns the kernel"
        );
        assert!(evaluator.result(probe).unwrap().is_err());

        graph.set_param(probe, "fail", false.into()).unwrap();
        evaluator.evaluate(&graph, 0, &[probe]);
        let checks = CHECKS.load(Ordering::Relaxed);
        assert!(evaluator.result(probe).unwrap().is_ok());
        evaluator.with_inputs(&graph, probe, 0, &PROBE, |_, _, _| Ok(())).unwrap();
        assert_eq!(
            CHECKS.load(Ordering::Relaxed),
            checks,
            "the current successful evaluation already checked this input contract"
        );
        assert_eq!(KERNEL_CALLS.load(Ordering::Relaxed), 2);

        graph.set_param(b, "unit", 1.into()).unwrap();
        let result =
            evaluator.with_inputs(&graph, probe, 0, &PROBE, |_, _, _| -> Result<(), KernelError> {
                panic!("incompatible inputs must not reach the consumer")
            });
        assert!(
            matches!(result, Err(NodeError::Constraint(error)) if error.detail == "measurement units differ")
        );
        assert_eq!(CHECKS.load(Ordering::Relaxed), checks + 1);
        assert_eq!(KERNEL_CALLS.load(Ordering::Relaxed), 2);
    }
}
