mod common;

use common::*;
use drip::eval::{Evaluator, NodeError, run_action};
use drip::project::Project;
use drip::value::View;
use serde_json::json;

/// c1 ─┐
///     add ── view        c3 (unrelated)
/// c2 ─┘
fn diamond() -> (Project, [drip::graph::NodeId; 5]) {
    let (reg, mut p) = (registry(), Project::default());
    let g = &mut p.graph;
    let ids = [
        g.add_node(&CONST),
        g.add_node(&CONST),
        g.add_node(&ADD),
        g.add_node(&VIEW),
        g.add_node(&CONST),
    ];
    let [c1, c2, add, view, _] = ids;
    g.connect(&reg, port(c1, "image"), port(add, "a")).unwrap();
    g.connect(&reg, port(c2, "image"), port(add, "b")).unwrap();
    g.connect(&reg, port(add, "sum"), port(view, "image")).unwrap();
    g.set_param(&reg, c2, "value", json!(2.0)).unwrap();
    (p, ids)
}

#[test]
fn evaluates_only_ancestors_of_targets() {
    let (p, [c1, c2, add, _, c3]) = diamond();
    let mut ev = Evaluator::default();
    let computed = ev.evaluate(&p, &registry(), PREVIEW, &[add]);
    assert_eq!(computed, [c1, c2, add]);
    assert_eq!(output(ev.result(add).unwrap()), [3.0, 8.0, 0.0]);
    assert!(ev.result(c3).is_none());
}

#[test]
fn recomputes_only_what_changed() {
    let (mut p, [c1, c2, add, view, _]) = diamond();
    let mut ev = Evaluator::default();
    ev.evaluate(&p, &registry(), PREVIEW, &[view]);
    assert!(ev.evaluate(&p, &registry(), PREVIEW, &[view]).is_empty());

    p.graph.set_param(&registry(), c1, "value", json!(5.0)).unwrap();
    assert_eq!(ev.evaluate(&p, &registry(), PREVIEW, &[view]), [c1, add, view]);
    assert_eq!(output(ev.result(add).unwrap()), [7.0, 8.0, 0.0]);

    // Rewiring changes the stamp downstream even though no params changed.
    p.graph.connect(&registry(), port(c2, "image"), port(add, "a")).unwrap();
    assert_eq!(ev.evaluate(&p, &registry(), PREVIEW, &[view]), [add, view]);
    assert_eq!(output(ev.result(add).unwrap()), [4.0, 8.0, 0.0]);
}

#[test]
fn scale_change_recomputes_everything() {
    let (p, [c1, c2, add, _, _]) = diamond();
    let mut ev = Evaluator::default();
    ev.evaluate(&p, &registry(), PREVIEW, &[add]);
    assert_eq!(ev.evaluate(&p, &registry(), 0, &[add]), [c1, c2, add]);
    assert_eq!(output(ev.result(add).unwrap()), [3.0, 2.0, 0.0]);
}

#[test]
fn failures_block_descendants_only() {
    let (reg, mut p) = (registry(), Project::default());
    let g = &mut p.graph;
    let (c, fail, after, ok) =
        (g.add_node(&CONST), g.add_node(&FAIL), g.add_node(&VIEW), g.add_node(&VIEW));
    g.connect(&reg, port(c, "image"), port(fail, "image")).unwrap();
    g.connect(&reg, port(fail, "image"), port(after, "image")).unwrap();
    g.connect(&reg, port(c, "image"), port(ok, "image")).unwrap();
    let mut ev = Evaluator::default();
    ev.evaluate(&p, &reg, PREVIEW, &[after, ok]);
    assert_eq!(ev.result(fail), Some(&Err(NodeError::Failed("boom".into()))));
    assert_eq!(ev.result(after), Some(&Err(NodeError::Upstream(fail))));
    assert!(ev.result(ok).unwrap().is_ok());
}

#[test]
fn unconnected_input_is_an_error() {
    let mut p = Project::default();
    let add = p.graph.add_node(&ADD);
    assert_eq!(eval(&mut Evaluator::default(), &p, add), &Err(NodeError::MissingInput("a")));
}

#[test]
fn ui_only_nodes_present_views() {
    let (p, [_, _, _, view, _]) = diamond();
    let mut ev = Evaluator::default();
    let result = eval(&mut ev, &p, view).as_ref().unwrap();
    assert!(result.outputs.is_empty());
    let Some(View::Image(image)) = &result.view else { panic!("no view") };
    assert_eq!(pixel(image), [3.0, 8.0, 0.0]);
}

#[test]
fn actions_run_at_full_resolution_on_request() {
    let (reg, mut p) = (registry(), Project::default());
    let g = &mut p.graph;
    let (c, t, w) = (g.add_node(&CONST), g.add_node(&TONEMAP), g.add_node(&WRITE));
    g.connect(&reg, port(c, "image"), port(t, "scene")).unwrap();
    g.connect(&reg, port(t, "display"), port(w, "image")).unwrap();
    let path = std::env::temp_dir().join(format!("drip-action-{}", std::process::id()));
    g.set_param(&reg, w, "path", json!(path)).unwrap();

    let mut ev = Evaluator::default();
    ev.evaluate(&p, &reg, PREVIEW, &[w]);
    assert!(!path.exists(), "evaluation has no side effects");

    run_action(&p, &reg, w, "write").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1.0, 1.0, 0.0]");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(run_action(&p, &reg, w, "print"), Err(NodeError::UnknownAction("print".into())));
}

#[test]
fn action_errors_are_reported() {
    let (reg, mut p) = (registry(), Project::default());
    let g = &mut p.graph;
    let (c, t, w) = (g.add_node(&CONST), g.add_node(&TONEMAP), g.add_node(&WRITE));
    g.connect(&reg, port(c, "image"), port(t, "scene")).unwrap();
    assert_eq!(run_action(&p, &reg, w, "write"), Err(NodeError::MissingInput("image")));
    p.graph.connect(&reg, port(t, "display"), port(w, "image")).unwrap();
    assert_eq!(run_action(&p, &reg, w, "write"), Err(NodeError::Failed("no path set".into())));
}

#[test]
fn editing_an_unrelated_node_recomputes_nothing() {
    let (mut p, [_, _, _, view, c3]) = diamond();
    let mut ev = Evaluator::default();
    ev.evaluate(&p, &registry(), PREVIEW, &[view]);
    p.graph.set_param(&registry(), c3, "value", json!(9.0)).unwrap();
    assert!(ev.evaluate(&p, &registry(), PREVIEW, &[view]).is_empty());
}

#[test]
fn registry_changes_invalidate_results() {
    let mut p = Project::default();
    let c = p.graph.add_node(&CONST);
    let mut ev = Evaluator::default();
    ev.evaluate(&p, &registry(), PREVIEW, &[c]);
    ev.evaluate(&p, &drip::node::Registry::default(), PREVIEW, &[c]);
    assert_eq!(ev.result(c), Some(&Err(NodeError::UnknownKind("test.const".into()))));
}

#[test]
fn errors_name_the_current_upstream() {
    let (reg, mut p) = (registry(), Project::default());
    let g = &mut p.graph;
    let (c, f1, f2, v) =
        (g.add_node(&CONST), g.add_node(&FAIL), g.add_node(&FAIL), g.add_node(&VIEW));
    g.connect(&reg, port(c, "image"), port(f1, "image")).unwrap();
    g.connect(&reg, port(c, "image"), port(f2, "image")).unwrap();
    g.connect(&reg, port(f1, "image"), port(v, "image")).unwrap();
    let mut ev = Evaluator::default();
    assert_eq!(eval(&mut ev, &p, v), &Err(NodeError::Upstream(f1)));
    p.graph.connect(&reg, port(f2, "image"), port(v, "image")).unwrap();
    assert_eq!(eval(&mut ev, &p, v), &Err(NodeError::Upstream(f2)));
}

#[test]
fn files_are_reread_only_on_reload() {
    let path = std::env::temp_dir().join(format!("drip-resource-{}", std::process::id()));
    std::fs::write(&path, "abc").unwrap();
    let mut p = Project::default();
    let f = p.graph.add_node(&FILE);
    p.graph.set_param(&registry(), f, "path", json!(path)).unwrap();
    let mut ev = Evaluator::default();
    assert_eq!(output(eval(&mut ev, &p, f))[0], 3.0);

    std::fs::write(&path, "abcdef").unwrap();
    assert!(
        ev.evaluate(&p, &registry(), PREVIEW, &[f]).is_empty(),
        "changes on disk are not watched"
    );
    assert_eq!(ev.evaluate(&p, &registry(), 0, &[f]), [f]);
    assert_eq!(output(ev.result(f).unwrap())[0], 3.0, "a new scale reuses what was loaded");

    ev.reload(&path);
    assert_eq!(output(eval(&mut ev, &p, f))[0], 6.0);
    std::fs::remove_file(&path).unwrap();
}

mod release {
    use std::sync::{Arc, Mutex, Weak};

    use super::*;
    use drip::node::{Action, Evaluated, InputSpec, NodeKind, OutputSpec};
    use drip::value::{PortType, Rgb, Value};

    static PROBED: Mutex<Option<Weak<Rgb>>> = Mutex::new(None);
    static SEEN: Mutex<Option<(bool, [f32; 3])>> = Mutex::new(None);

    /// Remembers its output allocation so tests can see when it is freed.
    static PROBE: NodeKind = NodeKind {
        name: "test.probe",
        version: 1,
        params: &[],
        inputs: &[],
        outputs: &[OutputSpec { name: "image", ty: PortType::SceneRec2020 }],
        eval: |_, _, _| {
            let image = Arc::new(Rgb { width: 1, height: 1, pixels: vec![[1.0, 2.0, 3.0]] });
            *PROBED.lock().unwrap() = Some(Arc::downgrade(&image));
            Ok(Evaluated { outputs: vec![Value::SceneRec2020(image)], view: None })
        },
        actions: &[],
        migrate: None,
    };

    /// Records, while its action runs, whether the probe's output is alive.
    static CHECK: NodeKind = NodeKind {
        name: "test.check",
        version: 1,
        params: &[],
        inputs: &[InputSpec { name: "image", accepts: &[PortType::DisplayRec2020] }],
        outputs: &[],
        eval: |_, _, _| Ok(Evaluated::default()),
        actions: &[Action {
            name: "check",
            run: |_, inputs| {
                let alive = PROBED.lock().unwrap().as_ref().unwrap().upgrade().is_some();
                *SEEN.lock().unwrap() = Some((alive, pixel(&inputs[0])));
                Ok(())
            },
        }],
        migrate: None,
    };

    /// Runs `check` on `probe → middle… → tonemap → check`.
    fn run(middle: &[&'static NodeKind]) -> (bool, [f32; 3]) {
        let reg = registry().with(&PROBE).with(&CHECK);
        let mut p = Project::default();
        let mut last = port(p.graph.add_node(&PROBE), "image");
        for kind in middle {
            let id = p.graph.add_node(kind);
            for input in kind.inputs {
                p.graph.connect(&reg, last.clone(), port(id, input.name)).unwrap();
            }
            last = port(id, kind.outputs[0].name);
        }
        let (t, check) = (p.graph.add_node(&TONEMAP), p.graph.add_node(&CHECK));
        p.graph.connect(&reg, last, port(t, "scene")).unwrap();
        p.graph.connect(&reg, port(t, "display"), port(check, "image")).unwrap();
        run_action(&p, &reg, check, "check").unwrap();
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
