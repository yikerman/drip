mod common;

use common::*;
use drip::eval::{Evaluator, NodeError};
use drip::graph::{GraphError, NodeId};
use drip::project::{LoadError, Project};
use serde_json::{Value as Json, json};

fn load(file: Json) -> Result<(Project, Vec<String>), LoadError> {
    Project::from_json(&file.to_string(), &registry())
}

fn file(nodes: Json, edges: Json) -> Json {
    json!({ "format": "drip", "version": 1, "nodes": nodes, "edges": edges })
}

fn node(id: u64, label: &str, kind: &str, version: u32, params: Json) -> Json {
    json!({ "id": id, "label": label, "kind": kind, "kind_version": version, "params": params })
}

fn edge(from: u64, output: &str, to: u64, input: &str) -> Json {
    json!({ "from": [from, output], "to": [to, input] })
}

fn saved(p: &Project) -> Json {
    serde_json::from_str(&p.to_json()).unwrap()
}

#[test]
fn round_trips() {
    let reg = registry();
    let mut p = Project::default();
    let (c, t, w) =
        (p.graph.add_node(&CONST), p.graph.add_node(&TONEMAP), p.graph.add_node(&WRITE));
    p.graph.connect(&reg, port(c, "image"), port(t, "scene")).unwrap();
    p.graph.connect(&reg, port(t, "display"), port(w, "image")).unwrap();
    p.graph.set_ui(c, json!({ "pos": [10, 20] })).unwrap();
    p.bind(&reg, w, "path", "out").unwrap();
    p.set_argument(&reg, "out", json!("a.tif")).unwrap();
    p.ui = json!({ "zoom": 2 });
    let (loaded, warnings) = Project::from_json(&p.to_json(), &reg).unwrap();
    assert_eq!(loaded, p);
    assert!(warnings.is_empty());
}

#[test]
fn unknown_kinds_survive_and_block_evaluation() {
    let original = file(
        json!([
            node(0, "future", "test.future", 3, json!({ "knob": [1, 2] })),
            node(1, "view", "test.view", 1, json!({}))
        ]),
        json!([edge(0, "out", 1, "image")]),
    );
    let (mut p, warnings) = load(original.clone()).unwrap();
    assert_eq!(warnings, ["`future`: unknown node kind `test.future`"]);
    assert_eq!(saved(&p), original);

    let mut ev = Evaluator::default();
    ev.evaluate(&p, &registry(), PREVIEW, &[NodeId(1)]);
    assert_eq!(ev.result(NodeId(0)), Some(&Err(NodeError::UnknownKind("test.future".into()))));
    assert_eq!(ev.result(NodeId(1)), Some(&Err(NodeError::Upstream(NodeId(0)))));

    let err = p.graph.set_param(&registry(), NodeId(0), "knob", json!(1));
    assert_eq!(err, Err(GraphError::UnknownKind("test.future".into())));
}

#[test]
fn newer_kind_versions_are_kept_but_not_evaluated() {
    let original = file(json!([node(0, "c", "test.const", 7, json!({ "value": 1 }))]), json!([]));
    let (p, warnings) = load(original.clone()).unwrap();
    assert_eq!(warnings, ["`c`: saved by a newer version of `test.const`"]);
    assert_eq!(saved(&p), original);
    let expected = NodeError::NewerVersion { kind: "test.const".into(), saved: 7, known: 1 };
    assert_eq!(eval(&mut Evaluator::default(), &p, NodeId(0)), &Err(expected));
}

#[test]
fn missing_params_default_and_unknown_params_are_kept() {
    let (p, warnings) =
        load(file(json!([node(0, "c", "test.const", 1, json!({ "old": true }))]), json!([])))
            .unwrap();
    assert_eq!(warnings, ["`c`: unknown parameter `old` kept"]);
    let params = &p.graph.node(NodeId(0)).unwrap().params;
    assert_eq!(params["value"], json!(1.0));
    assert_eq!(params["old"], json!(true));
    assert_eq!(output(eval(&mut Evaluator::default(), &p, NodeId(0))), [1.0, 4.0, 0.0]);
}

#[test]
fn old_kind_versions_migrate_params_and_ports() {
    let (p, warnings) = load(file(
        json!([
            node(0, "c", "test.const", 1, json!({ "value": 2 })),
            node(1, "g", "test.gain", 1, json!({ "factor": 3 }))
        ]),
        json!([edge(0, "image", 1, "in")]),
    ))
    .unwrap();
    assert!(warnings.is_empty());
    let gain = p.graph.node(NodeId(1)).unwrap();
    assert_eq!((gain.kind_version, &gain.params["gain"]), (2, &json!(3)));
    assert_eq!(p.graph.source(&port(NodeId(1), "image")), Some(&port(NodeId(0), "image")));
    assert_eq!(output(eval(&mut Evaluator::default(), &p, NodeId(1))), [6.0, 12.0, 0.0]);
}

#[test]
fn migration_follows_bound_params() {
    let mut gain = node(1, "g", "test.gain", 1, json!({}));
    gain["bindings"] = json!({ "factor": "x" });
    let mut f = file(
        json!([node(0, "c", "test.const", 1, json!({})), gain]),
        json!([edge(0, "image", 1, "in")]),
    );
    f["arguments"] = json!({ "x": 3 });
    let (p, warnings) = load(f).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(p.graph.node(NodeId(1)).unwrap().bindings["gain"], "x");
    assert_eq!(output(eval(&mut Evaluator::default(), &p, NodeId(1))), [3.0, 12.0, 0.0]);
}

#[test]
fn malformed_files_are_rejected() {
    let c = node(0, "c", "test.const", 1, json!({}));
    let a = node(1, "a", "test.add", 1, json!({}));
    let fails = |f: Json| load(f).unwrap_err();

    assert!(matches!(Project::from_json("{", &registry()), Err(LoadError::Json(_))));
    assert!(matches!(
        fails(json!({ "format": "png", "version": 1, "nodes": [] })),
        LoadError::NotDrip
    ));
    assert!(matches!(
        fails(json!({ "format": "drip", "version": 2, "nodes": [] })),
        LoadError::NewerFormat(2)
    ));
    let dangling = fails(file(json!([c]), json!([edge(0, "image", 5, "a")])));
    assert!(matches!(dangling, LoadError::Graph(GraphError::UnknownNode(NodeId(5)))));
    let cycle = fails(file(
        json!([a, node(2, "b", "test.add", 1, json!({}))]),
        json!([edge(1, "sum", 2, "a"), edge(2, "sum", 1, "a")]),
    ));
    assert!(matches!(cycle, LoadError::Graph(GraphError::Cycle)));
    let opaque_cycle = fails(file(
        json!([a, node(2, "x", "test.future", 1, json!({}))]),
        json!([edge(1, "sum", 2, "x"), edge(2, "y", 1, "a")]),
    ));
    assert!(matches!(opaque_cycle, LoadError::Graph(GraphError::Cycle)));
    let opaque_to_missing_port = fails(file(
        json!([c, node(2, "x", "test.future", 1, json!({}))]),
        json!([edge(2, "y", 0, "nope")]),
    ));
    assert!(matches!(
        opaque_to_missing_port,
        LoadError::Graph(GraphError::UnknownPort(NodeId(0), "input", _))
    ));
    let twice =
        fails(file(json!([c, a]), json!([edge(0, "image", 1, "a"), edge(0, "image", 1, "a")])));
    assert!(matches!(twice, LoadError::DuplicateInput(_)));
    let mistyped = fails(file(
        json!([c, node(1, "w", "test.write", 1, json!({}))]),
        json!([edge(0, "image", 1, "image")]),
    ));
    assert!(matches!(mistyped, LoadError::Graph(GraphError::TypeMismatch { .. })));
    let bad_value =
        fails(file(json!([node(0, "c", "test.const", 1, json!({ "value": 99 }))]), json!([])));
    assert!(matches!(bad_value, LoadError::Graph(GraphError::InvalidParam { .. })));
    let same_label = fails(file(json!([c, node(1, "c", "test.const", 1, json!({}))]), json!([])));
    assert!(matches!(same_label, LoadError::Graph(GraphError::InvalidLabel(_))));
    let same_id = fails(file(json!([c, node(0, "d", "test.const", 1, json!({}))]), json!([])));
    assert!(matches!(same_id, LoadError::InvalidId(NodeId(0))));
    let max_id = fails(file(json!([node(u64::MAX, "c", "test.const", 1, json!({}))]), json!([])));
    assert!(matches!(max_id, LoadError::InvalidId(NodeId(u64::MAX))));
}

#[test]
fn loaded_graphs_continue_ids_after_the_largest() {
    let (mut p, _) =
        load(file(json!([node(41, "c", "test.const", 1, json!({}))]), json!([]))).unwrap();
    assert_eq!(p.graph.add_node(&CONST), NodeId(42));
}

#[test]
fn templates_are_functions_of_their_inputs() {
    let reg = registry();
    let mut p = Project::default();
    let (c1, c2, add) =
        (p.graph.add_node(&CONST), p.graph.add_node(&CONST), p.graph.add_node(&ADD));
    p.graph.connect(&reg, port(c1, "image"), port(add, "a")).unwrap();
    p.graph.connect(&reg, port(c2, "image"), port(add, "b")).unwrap();
    p.bind(&reg, c1, "value", "x").unwrap();
    p.bind(&reg, c2, "value", "x").unwrap();
    assert_eq!(p.graph.inputs().into_iter().collect::<Vec<_>>(), ["x"]);

    let mut ev = Evaluator::default();
    assert_eq!(eval(&mut ev, &p, add), &Err(NodeError::Upstream(c1)));
    assert_eq!(ev.result(c1), Some(&Err(NodeError::MissingArgument("x".into()))));

    p.set_argument(&reg, "x", json!(1.5)).unwrap();
    assert_eq!(output(eval(&mut ev, &p, add)), [3.0, 8.0, 0.0]);
    p.set_argument(&reg, "x", json!(2.0)).unwrap();
    assert_eq!(output(eval(&mut ev, &p, add)), [4.0, 8.0, 0.0]);

    assert!(matches!(p.set_argument(&reg, "x", json!(50.0)), Err(GraphError::InvalidParam { .. })));
    assert_eq!(p.set_argument(&reg, "y", json!(1.0)), Err(GraphError::UnknownInput("y".into())));
    let w = p.graph.add_node(&WRITE);
    assert!(
        matches!(p.bind(&reg, w, "path", "x"), Err(GraphError::InvalidParam { .. })),
        "a number is not a path"
    );

    let template = p.template();
    assert!(template.arguments().is_empty());
    assert_eq!(template.graph, p.graph);
}

#[test]
fn invalid_arguments_in_files_are_rejected() {
    let mut c = node(0, "c", "test.const", 1, json!({}));
    c["bindings"] = json!({ "value": "x" });
    let mut f = file(json!([c]), json!([]));
    f["arguments"] = json!({ "x": "bright" });
    assert!(matches!(load(f.clone()), Err(LoadError::Graph(GraphError::InvalidParam { .. }))));
    f["arguments"] = json!({ "x": 2, "unused": 1 });
    let (_, warnings) = load(f).unwrap();
    assert_eq!(warnings, ["argument `unused` is not used by any node"]);
}
