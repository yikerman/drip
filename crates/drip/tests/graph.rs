mod common;

use common::*;
use drip::graph::{Graph, GraphError};

use serde_json::json;

#[test]
fn connect_rejects_incompatible_types() {
    let mut g = Graph::default();
    let (c, w) = (g.add_node(&CONST), g.add_node(&WRITE));
    let err = g.connect(port(c, "image"), port(w, "image")).unwrap_err();
    assert_eq!(err, GraphError::TypeMismatch { input: "image".into(), found: "SceneRec2020" });
    assert_eq!(g.edges().count(), 0);
}

#[test]
fn input_accepting_several_types_takes_each() {
    let mut g = Graph::default();
    let (c, t, v1, v2) =
        (g.add_node(&CONST), g.add_node(&TONEMAP), g.add_node(&VIEW), g.add_node(&VIEW));
    g.connect(port(c, "image"), port(t, "scene")).unwrap();
    g.connect(port(c, "image"), port(v1, "image")).unwrap();
    g.connect(port(t, "display"), port(v2, "image")).unwrap();
}

#[test]
fn connect_rejects_unknown_ports() {
    let mut g = Graph::default();
    let (c, a) = (g.add_node(&CONST), g.add_node(&ADD));
    assert_eq!(
        g.connect(port(c, "nope"), port(a, "a")),
        Err(GraphError::UnknownPort(c, "output", "nope".into()))
    );
    assert_eq!(
        g.connect(port(c, "image"), port(a, "c")),
        Err(GraphError::UnknownPort(a, "input", "c".into()))
    );
}

#[test]
fn connect_rejects_cycles() {
    let mut g = Graph::default();
    let (a, b) = (g.add_node(&ADD), g.add_node(&ADD));
    g.connect(port(a, "sum"), port(b, "a")).unwrap();
    assert_eq!(g.connect(port(b, "sum"), port(a, "a")), Err(GraphError::Cycle));
    assert_eq!(g.connect(port(a, "sum"), port(a, "b")), Err(GraphError::Cycle));
}

#[test]
fn connecting_an_input_replaces_its_source() {
    let mut g = Graph::default();
    let (c1, c2, a) = (g.add_node(&CONST), g.add_node(&CONST), g.add_node(&ADD));
    g.connect(port(c1, "image"), port(a, "a")).unwrap();
    g.connect(port(c2, "image"), port(a, "a")).unwrap();
    assert_eq!(g.source(&port(a, "a")), Some(&port(c2, "image")));
    assert_eq!(g.edges().count(), 1);
}

#[test]
fn removing_a_node_removes_its_edges() {
    let mut g = Graph::default();
    let (c, a, v) = (g.add_node(&CONST), g.add_node(&ADD), g.add_node(&VIEW));
    g.connect(port(c, "image"), port(a, "a")).unwrap();
    g.connect(port(a, "sum"), port(v, "image")).unwrap();
    g.remove_node(a);
    assert_eq!(g.edges().count(), 0);
    assert_ne!(g.add_node(&ADD), a, "ids are never reused");
}

#[test]
fn set_param_validates_against_the_schema() {
    let mut g = Graph::default();
    let c = g.add_node(&CONST);
    assert_eq!(g.node(c).unwrap().params["value"], json!(1.0), "defaults filled on creation");
    g.set_param(c, "value", json!(-2.5)).unwrap();
    assert!(matches!(g.set_param(c, "value", json!(11.0)), Err(GraphError::InvalidParam { .. })));
    assert!(matches!(g.set_param(c, "value", json!("x")), Err(GraphError::InvalidParam { .. })));
    assert_eq!(g.set_param(c, "gain", json!(1.0)), Err(GraphError::UnknownParam(c, "gain".into())));
    assert_eq!(g.node(c).unwrap().params["value"], json!(-2.5));
}

#[test]
fn labels_are_unique() {
    let mut g = Graph::default();
    let (a, b) = (g.add_node(&CONST), g.add_node(&CONST));
    assert_eq!(g.node(a).unwrap().label, "const");
    assert_eq!(g.node(b).unwrap().label, "const 2");
    assert_eq!(g.set_label(b, "const"), Err(GraphError::InvalidLabel("const".into())));
    assert_eq!(g.set_label(b, ""), Err(GraphError::InvalidLabel("".into())));
    g.set_label(b, "exposure").unwrap();
    assert_eq!(g.find("exposure"), Some(b));
}

#[test]
fn every_node_gets_its_own_inputs() {
    let mut g = Graph::default();
    let (w1, w2, c) = (g.add_node(&WRITE), g.add_node(&WRITE), g.add_node(&CONST));
    assert_eq!(g.inputs().collect::<Vec<_>>(), [(w1, "path"), (w2, "path")], "external by default");
    g.set_external(c, "value", true).unwrap();
    g.set_external(w2, "path", false).unwrap();
    assert_eq!(g.inputs().collect::<Vec<_>>(), [(w1, "path"), (c, "value")]);
    assert_eq!(g.set_external(c, "nope", true), Err(GraphError::UnknownParam(c, "nope".into())));
}
