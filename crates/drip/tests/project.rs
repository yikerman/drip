mod common;

use common::*;
use drip::eval::{Evaluator, NodeError};
use drip::graph::GraphError;
use drip::project::Project;
use serde_json::json;

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
