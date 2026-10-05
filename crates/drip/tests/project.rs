mod common;

use common::*;
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
    json!({ "id": id, "label": label, "kind": kind, "params": params, "external": [] })
}

fn edge(from: u64, output: &str, to: u64, input: &str) -> Json {
    json!({ "from": [from, output], "to": [to, input] })
}

/// const → tonemap → write, with the write path filled in.
fn project() -> (Project, [NodeId; 3]) {
    let mut p = Project::default();
    let g = &mut p.graph;
    let ids = [g.add_node(&CONST), g.add_node(&TONEMAP), g.add_node(&WRITE)];
    g.connect(port(ids[0], "image"), port(ids[1], "scene")).unwrap();
    g.connect(port(ids[1], "display"), port(ids[2], "image")).unwrap();
    g.set_param(ids[0], "value", json!(2.5)).unwrap();
    g.set_param(ids[2], "path", json!("a.tif")).unwrap();
    g.set_ui(ids[0], json!({ "pos": [10, 20] })).unwrap();
    p.ui = json!({ "zoom": 2 });
    (p, ids)
}

#[test]
fn round_trips() {
    let (mut p, [c, ..]) = project();
    p.graph.set_external(c, "value", true).unwrap();
    let saved: Json = serde_json::from_str(&p.to_json()).unwrap();
    assert_eq!(saved["version"], json!(MAJOR.parse::<u32>().unwrap()), "the crate's major version");
    assert_eq!(Project::from_json(&p.to_json(), &registry()).unwrap(), p);
}

#[test]
fn templates_clear_external_params_only() {
    let (mut p, [c, _, w]) = project();
    p.graph.set_external(c, "value", true).unwrap();
    let template = p.template();
    assert_eq!(template.graph.node(c).unwrap().params["value"], json!(1.0), "back to its default");
    assert_eq!(template.graph.node(w).unwrap().params["path"], json!(null));
    assert_eq!(template.graph.inputs().collect::<Vec<_>>(), p.graph.inputs().collect::<Vec<_>>());
    let (_, [_, t, _]) = project();
    assert_eq!(template.graph.node(t), p.graph.node(t), "internal params untouched");
}

#[test]
fn other_major_versions_are_rejected() {
    let mut f = file(json!([]), json!([]));
    f["version"] = json!(MAJOR.parse::<u32>().unwrap() + 1);
    assert!(matches!(load(f), Err(LoadError::Version(_))));
}

#[test]
fn files_must_match_this_build_exactly() {
    let missing = load(file(json!([node(0, "c", "test.const", json!({}))]), json!([])));
    assert!(matches!(missing, Err(LoadError::MissingParam(NodeId(0), "value"))));
    let mut extra = file(json!([]), json!([]));
    extra["arguments"] = json!({ "raw": "a.arw" });
    assert!(matches!(load(extra), Err(LoadError::Json(_))), "fields of older formats are refused");
    let mut bindings = node(0, "c", "test.const", json!({ "value": 1.0 }));
    bindings["bindings"] = json!({ "value": "x" });
    assert!(matches!(load(file(json!([bindings]), json!([]))), Err(LoadError::Json(_))));
}

#[test]
fn external_sets_load_as_saved() {
    let mut w = node(0, "w", "test.write", json!({ "path": "a.tif" }));
    w["external"] = json!([]);
    let mut c = node(1, "c", "test.const", json!({ "value": 2.0 }));
    c["external"] = json!(["value"]);
    let p = load(file(json!([w, c]), json!([]))).unwrap();
    assert_eq!(p.graph.inputs().collect::<Vec<_>>(), [(NodeId(1), "value")]);
}

#[test]
fn templates_are_idempotent() {
    let (p, _) = project();
    assert_eq!(p.template().template(), p.template());
}

#[test]
fn malformed_files_are_rejected() {
    let c = node(0, "c", "test.const", json!({ "value": 1.0 }));
    let a = node(1, "a", "test.add", json!({}));
    let fails = |f: Json| load(f).unwrap_err();
    let graph = |e: LoadError| match e {
        LoadError::Graph(e) => e,
        e => panic!("{e}"),
    };

    assert!(matches!(Project::from_json("{", &registry()), Err(LoadError::Json(_))));
    assert!(matches!(
        fails(json!({ "format": "png", "version": 0, "nodes": [], "edges": [] })),
        LoadError::NotDrip
    ));
    let unknown = fails(file(json!([node(0, "x", "test.future", json!({}))]), json!([])));
    assert!(matches!(unknown, LoadError::UnknownKind(k) if k == "test.future"));
    let param =
        graph(fails(file(json!([node(0, "c", "test.const", json!({ "old": 1 }))]), json!([]))));
    assert_eq!(param, GraphError::UnknownParam(NodeId(0), "old".into()));
    let mut external = c.clone();
    external["external"] = json!(["old"]);
    let external = graph(fails(file(json!([external]), json!([]))));
    assert_eq!(external, GraphError::UnknownParam(NodeId(0), "old".into()));
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
        json!([c, node(1, "w", "test.write", json!({ "path": null }))]),
        json!([edge(0, "image", 1, "image")]),
    ));
    assert!(matches!(graph(mistyped), GraphError::TypeMismatch { .. }));
    let bad_value =
        fails(file(json!([node(0, "c", "test.const", json!({ "value": 99 }))]), json!([])));
    assert!(matches!(graph(bad_value), GraphError::InvalidParam { .. }));
    let same_label =
        fails(file(json!([c, node(1, "c", "test.const", json!({ "value": 1.0 }))]), json!([])));
    assert!(matches!(graph(same_label), GraphError::InvalidLabel(_)));
    let same_id =
        fails(file(json!([c, node(0, "d", "test.const", json!({ "value": 1.0 }))]), json!([])));
    assert!(matches!(same_id, LoadError::InvalidId(0)));
    let max_id =
        fails(file(json!([node(u64::MAX, "c", "test.const", json!({ "value": 1.0 }))]), json!([])));
    assert!(matches!(max_id, LoadError::InvalidId(u64::MAX)));
}

#[test]
fn loaded_graphs_continue_ids_after_the_largest() {
    let mut p =
        load(file(json!([node(41, "c", "test.const", json!({ "value": 1.0 }))]), json!([])))
            .unwrap();
    assert_eq!(p.graph.add_node(&CONST), NodeId(42));
}
