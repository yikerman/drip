mod common;

use common::*;
use drip::eval::{Evaluator, NodeError, run_action};
use drip::image::Rec2020Mat;
use drip::node::{NodeDeclaration, NodeKind, TypedAction};

use drip::ports::{Optional, Read};
use drip::project::Project;
use serde_json::json;

/// c1 ─┐
///     add ── view        c3 (unrelated)
/// c2 ─┘
fn diamond() -> (Project, [drip::graph::NodeId; 5]) {
    let mut p = Project::default();
    let g = &mut p.graph;
    let ids = [
        g.add_node(&CONST),
        g.add_node(&CONST),
        g.add_node(&ADD),
        g.add_node(&VIEW),
        g.add_node(&CONST),
    ];
    let [c1, c2, add, view, _] = ids;
    g.connect(port(c1, "image"), port(add, "a")).unwrap();
    g.connect(port(c2, "image"), port(add, "b")).unwrap();
    g.connect(port(add, "sum"), port(view, "image")).unwrap();
    g.set_param(c2, "value", json!(2.0)).unwrap();
    (p, ids)
}

#[test]
fn evaluates_only_ancestors_of_targets() {
    let (p, [c1, c2, add, _, c3]) = diamond();
    let mut ev = Evaluator::default();
    let computed = ev.evaluate(&p.graph, PREVIEW, &[add]);
    assert_eq!(computed, [c1, c2, add]);
    assert_eq!(output(ev.result(add).unwrap()), [3.0, 8.0, 0.0]);
    assert!(ev.result(c3).is_none());
}

#[test]
fn recomputes_only_what_changed() {
    let (mut p, [c1, c2, add, view, _]) = diamond();
    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, PREVIEW, &[view]);
    assert!(ev.evaluate(&p.graph, PREVIEW, &[view]).is_empty());

    p.graph.set_param(c1, "value", json!(5.0)).unwrap();
    assert_eq!(ev.evaluate(&p.graph, PREVIEW, &[view]), [c1, add, view]);
    assert_eq!(output(ev.result(add).unwrap()), [7.0, 8.0, 0.0]);

    // Rewiring changes the stamp downstream even though no params changed.
    p.graph.connect(port(c2, "image"), port(add, "a")).unwrap();
    assert_eq!(ev.evaluate(&p.graph, PREVIEW, &[view]), [add, view]);
    assert_eq!(output(ev.result(add).unwrap()), [4.0, 8.0, 0.0]);
}

#[test]
fn scale_change_recomputes_everything() {
    let (p, [c1, c2, add, _, _]) = diamond();
    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, PREVIEW, &[add]);
    assert_eq!(ev.evaluate(&p.graph, 0, &[add]), [c1, c2, add]);
    assert_eq!(output(ev.result(add).unwrap()), [3.0, 2.0, 0.0]);
}

#[test]
fn failures_block_descendants_only() {
    let mut p = Project::default();
    let g = &mut p.graph;
    let (c, fail, after, ok) =
        (g.add_node(&CONST), g.add_node(&FAIL), g.add_node(&VIEW), g.add_node(&VIEW));
    g.connect(port(c, "image"), port(fail, "image")).unwrap();
    g.connect(port(fail, "image"), port(after, "image")).unwrap();
    g.connect(port(c, "image"), port(ok, "image")).unwrap();
    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, PREVIEW, &[after, ok]);
    assert_eq!(ev.result(fail).unwrap().as_ref().unwrap_err(), &NodeError::Failed("boom".into()));
    assert_eq!(ev.result(after).unwrap().as_ref().unwrap_err(), &NodeError::Upstream(fail));
    assert!(ev.result(ok).unwrap().is_ok());
}

#[test]
fn unconnected_input_is_an_error() {
    let mut p = Project::default();
    let add = p.graph.add_node(&ADD);
    assert_eq!(
        eval(&mut Evaluator::default(), &p, add).as_ref().unwrap_err(),
        &NodeError::MissingInput("a")
    );
}

/// `base`, plus `offset` when connected.
static OFFSET: NodeKind =
    NodeKind::new::<OffsetKernel>("test.offset", "test", "offset", &["base", "offset"], &["image"]);
struct OffsetKernel;
impl NodeDeclaration for OffsetKernel {
    type Parameters = ();
    type Inputs = (Read<Rec2020Mat>, Optional<Read<Rec2020Mat>>);
    type Outputs = (std::sync::Arc<Rec2020Mat>,);

    const KERNEL: Option<drip::node::Kernel<Self>> = Some(|_, (base, offset), _| {
        let (a, b) = (base.rgb().pixels[0], offset.map_or([0.0; 3], |o| o.rgb().pixels[0]));
        Ok((scene([a[0] + b[0], a[1] + b[1], a[2] + b[2]]),))
    });
}

#[test]
fn optional_inputs_may_stay_unconnected_but_not_fail() {
    let mut p = Project::default();
    let g = &mut p.graph;
    let (c, fail, offset) = (g.add_node(&CONST), g.add_node(&FAIL), g.add_node(&OFFSET));
    g.connect(port(c, "image"), port(offset, "base")).unwrap();
    let mut ev = Evaluator::default();
    assert_eq!(output(eval(&mut ev, &p, offset)), [1.0, 4.0, 0.0]);

    p.graph.connect(port(c, "image"), port(offset, "offset")).unwrap();
    assert_eq!(output(eval(&mut ev, &p, offset)), [2.0, 8.0, 0.0]);

    p.graph.connect(port(c, "image"), port(fail, "image")).unwrap();
    p.graph.connect(port(fail, "image"), port(offset, "offset")).unwrap();
    assert_eq!(eval(&mut ev, &p, offset).as_ref().unwrap_err(), &NodeError::Upstream(fail));

    p.graph.disconnect(&port(offset, "base"));
    assert_eq!(eval(&mut ev, &p, offset).as_ref().unwrap_err(), &NodeError::MissingInput("base"));
}

#[test]
fn input_only_consumers_borrow_validated_upstream_values() {
    let (p, [_, _, _, view, _]) = diamond();
    let mut ev = Evaluator::default();
    let pixel = ev
        .with_inputs(&p.graph, view, PREVIEW, &VIEW, |_, (image,), _| Ok(image.rgb().pixels[0]))
        .unwrap();
    assert_eq!(pixel, [3.0, 8.0, 0.0]);
    assert!(ev.result(view).is_none());
}

#[test]
fn actions_run_at_full_resolution_on_request() {
    let mut p = Project::default();
    let g = &mut p.graph;
    let (c, t, w) = (g.add_node(&CONST), g.add_node(&TONEMAP), g.add_node(&WRITE));
    g.connect(port(c, "image"), port(t, "scene")).unwrap();
    g.connect(port(t, "display"), port(w, "image")).unwrap();
    let path = std::env::temp_dir().join(format!("drip-action-{}", std::process::id()));
    g.set_param(w, "path", json!(path)).unwrap();

    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, PREVIEW, &[w]);
    assert!(!path.exists(), "evaluation has no side effects");

    run_action(&p.graph, w, "write").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1.0, 1.0, 0.0]");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(run_action(&p.graph, w, "print"), Err(NodeError::UnknownAction("print".into())));
}

#[test]
fn action_errors_are_reported() {
    let mut p = Project::default();
    let g = &mut p.graph;
    let (c, t, w) = (g.add_node(&CONST), g.add_node(&TONEMAP), g.add_node(&WRITE));
    g.connect(port(c, "image"), port(t, "scene")).unwrap();
    assert_eq!(run_action(&p.graph, w, "write"), Err(NodeError::MissingInput("image")));
    p.graph.connect(port(t, "display"), port(w, "image")).unwrap();
    assert_eq!(run_action(&p.graph, w, "write"), Err(NodeError::Failed("no path set".into())));
}

#[test]
fn actions_preserve_root_failures_after_releasing_intermediates() {
    let mut p = Project::default();
    let g = &mut p.graph;
    let (c, f, t, w) =
        (g.add_node(&CONST), g.add_node(&FAIL), g.add_node(&TONEMAP), g.add_node(&WRITE));
    g.connect(port(c, "image"), port(f, "image")).unwrap();
    g.connect(port(f, "image"), port(t, "scene")).unwrap();
    g.connect(port(t, "display"), port(w, "image")).unwrap();
    assert_eq!(
        run_action(&p.graph, w, "write"),
        Err(NodeError::AtNode {
            node: f,
            kind: "test.fail",
            source: Box::new(NodeError::Failed("boom".into())),
        })
    );
}

#[test]
fn an_unconfigured_raw_is_incomplete_instead_of_a_processing_failure() {
    let mut graph = drip::graph::Graph::default();
    let raw = graph.add_node(&drip::nodes::READ);
    let mut evaluator = Evaluator::default();
    evaluator.evaluate(&graph, 1, &[raw]);
    assert_eq!(
        evaluator.result(raw).unwrap().as_ref().unwrap_err(),
        &NodeError::Incomplete("no raw file chosen")
    );
}

#[test]
fn editing_an_unrelated_node_recomputes_nothing() {
    let (mut p, [_, _, _, view, c3]) = diamond();
    let mut ev = Evaluator::default();
    ev.evaluate(&p.graph, PREVIEW, &[view]);
    p.graph.set_param(c3, "value", json!(9.0)).unwrap();
    assert!(ev.evaluate(&p.graph, PREVIEW, &[view]).is_empty());
}

#[test]
fn errors_name_the_current_upstream() {
    let mut p = Project::default();
    let g = &mut p.graph;
    let (c, f1, f2, v) =
        (g.add_node(&CONST), g.add_node(&FAIL), g.add_node(&FAIL), g.add_node(&VIEW));
    g.connect(port(c, "image"), port(f1, "image")).unwrap();
    g.connect(port(c, "image"), port(f2, "image")).unwrap();
    g.connect(port(f1, "image"), port(v, "image")).unwrap();
    let mut ev = Evaluator::default();
    assert_eq!(eval(&mut ev, &p, v).as_ref().unwrap_err(), &NodeError::Upstream(f1));
    p.graph.connect(port(f2, "image"), port(v, "image")).unwrap();
    assert_eq!(eval(&mut ev, &p, v).as_ref().unwrap_err(), &NodeError::Upstream(f2));
}

#[test]
fn files_are_reread_after_cache_invalidation() {
    let path = std::env::temp_dir().join(format!("drip-resource-{}", std::process::id()));
    std::fs::write(&path, "abc").unwrap();
    let mut p = Project::default();
    let f = p.graph.add_node(&FILE);
    p.graph.set_param(f, "path", json!(path)).unwrap();
    let mut ev = Evaluator::default();
    assert_eq!(output(eval(&mut ev, &p, f))[0], 3.0);

    std::fs::write(&path, "abcdef").unwrap();
    assert!(ev.evaluate(&p.graph, PREVIEW, &[f]).is_empty(), "changes on disk are not watched");
    assert_eq!(ev.evaluate(&p.graph, 0, &[f]), [f]);
    assert_eq!(output(ev.result(f).unwrap())[0], 3.0, "a new scale reuses what was loaded");

    ev = Evaluator::default();
    assert_eq!(output(eval(&mut ev, &p, f))[0], 6.0);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn actions_share_resources_with_preview_and_survive_invalidation() {
    let path = std::env::temp_dir().join(format!("drip-shared-input-{}", std::process::id()));
    let out = path.with_extension("out");
    std::fs::write(&path, "abc").unwrap();
    let mut p = Project::default();
    let (f, t, w) = (p.graph.add_node(&FILE), p.graph.add_node(&TONEMAP), p.graph.add_node(&WRITE));
    p.graph.set_param(f, "path", json!(path)).unwrap();
    p.graph.set_param(w, "path", json!(out)).unwrap();
    p.graph.connect(port(f, "image"), port(t, "scene")).unwrap();
    p.graph.connect(port(t, "display"), port(w, "image")).unwrap();

    let mut ev = Evaluator::default();
    // An export can be the first consumer of a resource, before any preview.
    let action = ev.fork();
    action.fork().run_action(&p.graph, w, "write").unwrap();
    std::fs::write(&path, "abcdef").unwrap();
    assert_eq!(output(eval(&mut ev, &p, f))[0], 3.0);
    ev = Evaluator::default();
    assert_eq!(output(eval(&mut ev, &p, f))[0], 6.0);
    action.run_action(&p.graph, w, "write").unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "[3.0, 0.0, 0.0]");
    ev.fork().run_action(&p.graph, w, "write").unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "[6.0, 0.0, 0.0]");
    assert!(ev.evaluate(&p.graph, PREVIEW, &[f]).is_empty(), "actions keep the preview cache");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(out).unwrap();
}

mod release {
    use std::sync::{Arc, Mutex, Weak};

    use super::*;
    use drip::image::Rgb;
    use drip::node::NodeKind;

    static PROBED: Mutex<Option<Weak<Rgb>>> = Mutex::new(None);
    static SEEN: Mutex<Option<(bool, [f32; 3])>> = Mutex::new(None);

    /// Remembers its output allocation so tests can see when it is freed.
    static PROBE: NodeKind =
        NodeKind::new::<ProbeKernel>("test.probe", "test", "probe", &[], &["image"]);
    struct ProbeKernel;
    impl NodeDeclaration for ProbeKernel {
        type Parameters = ();
        type Inputs = ();
        type Outputs = (Arc<Rec2020Mat>,);

        const KERNEL: Option<drip::node::Kernel<Self>> = Some(|_, (), _| {
            let image =
                Arc::new(Rgb { width: 1, height: 1, scale: 1, pixels: vec![[1.0, 2.0, 3.0]] });
            *PROBED.lock().unwrap() = Some(Arc::downgrade(&image));
            Ok((Arc::new(Rec2020Mat::from(image)),))
        });
    }

    /// Records, while its action runs, whether the probe's output is alive.
    static CHECK: NodeKind =
        NodeKind::new::<CheckKernel>("test.check", "test", "check", &["image"], &[]);
    struct CheckKernel;
    impl NodeDeclaration for CheckKernel {
        type Parameters = ();
        type Inputs = (Read<TaggedMat>,);
        type Outputs = ();
        const ACTIONS: &'static [TypedAction<Self>] = &[TypedAction {
            name: "check",
            run: |_, (input0,), _| {
                let alive = PROBED.lock().unwrap().as_ref().unwrap().upgrade().is_some();
                *SEEN.lock().unwrap() = Some((alive, input0.rgb().pixels[0]));
                Ok(())
            },
        }];
    }

    /// Runs `check` on `probe → middle… → tonemap → check`.
    fn run(middle: &[&'static NodeKind]) -> (bool, [f32; 3]) {
        let mut p = Project::default();
        let mut last = port(p.graph.add_node(&PROBE), "image");
        for kind in middle {
            let id = p.graph.add_node(kind);
            for input in kind.inputs() {
                p.graph.connect(last.clone(), port(id, input.name)).unwrap();
            }
            last = port(id, kind.outputs().next().unwrap().name);
        }
        let (t, check) = (p.graph.add_node(&TONEMAP), p.graph.add_node(&CHECK));
        p.graph.connect(last, port(t, "scene")).unwrap();
        p.graph.connect(port(t, "display"), port(check, "image")).unwrap();
        run_action(&p.graph, check, "check").unwrap();
        SEEN.lock().unwrap().take().unwrap()
    }

    // One test, so the shared statics are never used concurrently.
    #[test]
    fn intermediates_are_freed_after_their_last_consumer() {
        assert_eq!(run(&[]), (true, [1.0, 2.0, 3.0]), "direct input of the action stays alive");
        assert_eq!(run(&[&GAIN]), (false, [1.0, 2.0, 3.0]), "consumed by gain, then freed");
        assert_eq!(run(&[&ADD]), (false, [2.0, 4.0, 6.0]), "one source feeding both inputs");
    }
}
