mod common;

use common::*;
use drip::eval::{Evaluator, NodeError};
use drip::graph::{GraphError, NodeId};
use drip::project::{LoadError, Project};
use serde_json::{Value as Json, json};

const MAJOR: &str = env!("CARGO_PKG_VERSION_MAJOR");

fn load(file: Json) -> Result<Project, LoadError> {
    Project::from_json(&file.to_string(), &registry())
}

fn file(nodes: Json, edges: Json) -> Json {
    json!({ "format": "drip", "version": MAJOR.parse::<u32>().unwrap(), "nodes": nodes, "edges": edges })
}

fn node(id: u64, label: &str, kind: &str, params: Json) -> Json {
    json!({ "id": id, "label": label, "kind": kind, "params": params })
}

fn edge(from: u64, output: &str, to: u64, input: &str) -> Json {
    json!({ "from": [from, output], "to": [to, input] })
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
    let saved: Json = serde_json::from_str(&p.to_json()).unwrap();
    assert_eq!(saved["version"], json!(MAJOR.parse::<u32>().unwrap()), "the crate's major version");
    assert_eq!(Project::from_json(&p.to_json(), &reg).unwrap(), p);
}

#[test]
fn other_major_versions_are_rejected() {
    let mut f = file(json!([]), json!([]));
    f["version"] = json!(MAJOR.parse::<u32>().unwrap() + 1);
    assert!(matches!(load(f), Err(LoadError::Version(_))));
}

#[test]
fn missing_params_take_defaults() {
    let p = load(file(json!([node(0, "c", "test.const", json!({}))]), json!([]))).unwrap();
    assert_eq!(p.graph.node(NodeId(0)).unwrap().params["value"], json!(1.0));
    assert_eq!(output(eval(&mut Evaluator::default(), &p, NodeId(0))), [1.0, 4.0, 0.0]);
}

#[test]
fn malformed_files_are_rejected() {
    let c = node(0, "c", "test.const", json!({}));
    let a = node(1, "a", "test.add", json!({}));
    let fails = |f: Json| load(f).unwrap_err();
    let graph = |e: LoadError| match e {
        LoadError::Graph(e) => e,
        e => panic!("{e}"),
    };

    assert!(matches!(Project::from_json("{", &registry()), Err(LoadError::Json(_))));
    assert!(matches!(
        fails(json!({ "format": "png", "version": 0, "nodes": [] })),
        LoadError::NotDrip
    ));
    let unknown = graph(fails(file(json!([node(0, "x", "test.future", json!({}))]), json!([]))));
    assert_eq!(unknown, GraphError::UnknownKind("test.future".into()));
    let param =
        graph(fails(file(json!([node(0, "c", "test.const", json!({ "old": 1 }))]), json!([]))));
    assert_eq!(param, GraphError::UnknownParam(NodeId(0), "old".into()));
    let dangling = graph(fails(file(json!([c]), json!([edge(0, "image", 5, "a")]))));
    assert_eq!(dangling, GraphError::UnknownNode(NodeId(5)));
    let port = graph(fails(file(json!([c, a]), json!([edge(0, "image", 1, "nope")]))));
    assert_eq!(port, GraphError::UnknownPort(NodeId(1), "input", "nope".into()));
    let cycle = graph(fails(file(
        json!([a, node(2, "b", "test.add", json!({}))]),
        json!([edge(1, "sum", 2, "a"), edge(2, "sum", 1, "a")]),
    )));
    assert_eq!(cycle, GraphError::Cycle);
    let twice =
        fails(file(json!([c, a]), json!([edge(0, "image", 1, "a"), edge(0, "image", 1, "a")])));
    assert!(matches!(twice, LoadError::DuplicateInput(_)));
    let mistyped = fails(file(
        json!([c, node(1, "w", "test.write", json!({}))]),
        json!([edge(0, "image", 1, "image")]),
    ));
    assert!(matches!(graph(mistyped), GraphError::TypeMismatch { .. }));
    let bad_value =
        fails(file(json!([node(0, "c", "test.const", json!({ "value": 99 }))]), json!([])));
    assert!(matches!(graph(bad_value), GraphError::InvalidParam { .. }));
    let same_label = fails(file(json!([c, node(1, "c", "test.const", json!({}))]), json!([])));
    assert!(matches!(graph(same_label), GraphError::InvalidLabel(_)));
    let same_id = fails(file(json!([c, node(0, "d", "test.const", json!({}))]), json!([])));
    assert!(matches!(same_id, LoadError::InvalidId(NodeId(0))));
    let max_id = fails(file(json!([node(u64::MAX, "c", "test.const", json!({}))]), json!([])));
    assert!(matches!(max_id, LoadError::InvalidId(NodeId(u64::MAX))));
}

#[test]
fn loaded_graphs_continue_ids_after_the_largest() {
    let mut p = load(file(json!([node(41, "c", "test.const", json!({}))]), json!([]))).unwrap();
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

    let template = p.template();
    assert!(template.arguments().is_empty());
    assert_eq!(template.graph, p.graph);
}

#[test]
fn an_input_takes_one_kind_of_value() {
    let reg = registry();
    let mut p = Project::default();
    let (c, w) = (p.graph.add_node(&CONST), p.graph.add_node(&WRITE));
    p.bind(&reg, c, "value", "x").unwrap();
    let refused = p.bind(&reg, w, "path", "x");
    assert_eq!(
        refused,
        Err(GraphError::IncompatibleInput { input: "x".into(), param: "path".into() })
    );
    assert!(
        p.input_kind(&reg, "x").is_some_and(|k| matches!(k, drip::param::ParamKind::Float { .. }))
    );

    let mut f: Json = serde_json::from_str(&p.to_json()).unwrap();
    f["nodes"][1]["bindings"] = json!({ "path": "x" });
    let graph = load(f).unwrap_err();
    assert!(matches!(graph, LoadError::Graph(GraphError::IncompatibleInput { .. })));
}

#[test]
fn invalid_arguments_in_files_are_rejected() {
    let mut c = node(0, "c", "test.const", json!({}));
    c["bindings"] = json!({ "value": "x" });
    let mut f = file(json!([c]), json!([]));
    f["arguments"] = json!({ "x": "bright" });
    assert!(matches!(load(f.clone()), Err(LoadError::Graph(GraphError::InvalidParam { .. }))));
    f["arguments"] = json!({ "unused": 1 });
    assert!(matches!(load(f), Err(LoadError::Graph(GraphError::UnknownInput(_)))));
}
