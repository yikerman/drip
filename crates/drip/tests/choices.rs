use drip::param::{ParamKind, Parameters, Params};
use drip::project::Project;
use serde_json::json;

#[derive(Debug, PartialEq, drip::Choice)]
enum Mode {
    #[choice("first")]
    First,
    #[choice("second_mode")]
    Second,
}

#[derive(drip::Parameters)]
struct Settings {
    #[param(Mode::Second.schema())]
    mode: Mode,
}

#[test]
fn choices_share_schema_json_and_typed_reads() {
    let spec = &Settings::SPECS[0];
    assert_eq!(spec.kind.default_value(), json!("second_mode"));
    for (text, expected) in [("first", Mode::First), ("second_mode", Mode::Second)] {
        let values = serde_json::from_value(json!({"mode": text})).unwrap();
        assert!(spec.kind.accepts(&json!(text)));
        assert_eq!(Settings::read(Params::validated(&values)).mode, expected);
        assert_eq!(serde_json::to_value(expected).unwrap(), json!(text));
    }
    assert!(!spec.kind.accepts(&json!("third")));
    assert!(serde_json::from_value::<Mode>(json!("third")).is_err());
    assert!(serde_json::from_value::<Mode>(json!(0)).is_err());
}

#[test]
fn built_in_choices_remain_valid_project_strings() {
    let registry = drip::nodes::registry();
    let mut project = Project::default();
    for kind in registry.kinds() {
        let id = project.graph.add_node(kind);
        for spec in kind.params {
            let ParamKind::Choice { options, default } = spec.kind else { continue };
            assert_eq!(project.graph.node(id).unwrap().params[spec.name], json!(default));
            for &option in options {
                project.graph.set_param(id, spec.name, json!(option)).unwrap();
                let loaded = Project::from_json(&project.to_json(), &registry).unwrap();
                assert_eq!(loaded.graph.node(id).unwrap().params[spec.name], json!(option));
            }
            let before = project.to_json();
            assert!(project.graph.set_param(id, spec.name, json!("unknown-choice")).is_err());
            assert_eq!(project.to_json(), before);
        }
    }
}
