use drip::param::Parameters;
use serde_json::json;

#[derive(Debug, PartialEq, drip::Choice)]
enum Mode {
    #[choice("first")]
    First,
    #[choice("second_mode")]
    #[label("Second mode")]
    Second,
}

#[derive(drip::Parameters, serde::Deserialize)]
struct Settings {
    #[param(Mode::Second.schema())]
    mode: Mode,
}

#[test]
fn choices_share_schema_json_and_typed_reads() {
    let spec = &Settings::SPECS[0];
    assert_eq!(spec.label, "Mode");
    assert_eq!(Mode::CHOICES[1].label, "Second mode");
    assert_eq!(spec.kind.default_value(), json!("second_mode"));
    for (text, expected) in [("first", Mode::First), ("second_mode", Mode::Second)] {
        let values: Settings = serde_json::from_value(json!({"mode": text})).unwrap();
        assert!(spec.kind.accepts(&json!(text)));
        assert_eq!(values.mode, expected);
        assert_eq!(serde_json::to_value(expected).unwrap(), json!(text));
    }
    assert!(!spec.kind.accepts(&json!("third")));
    assert!(serde_json::from_value::<Mode>(json!("third")).is_err());
    assert!(serde_json::from_value::<Mode>(json!(0)).is_err());
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize, drip::Parameters)]
#[serde(deny_unknown_fields)]
struct Options {
    #[param(drip::param::ParamKind::Bool { default: true })]
    enabled: bool,
}

#[derive(Debug, PartialEq, drip::Choice)]
enum Nested {
    #[choice("none")]
    None,
    #[choice("configured")]
    Configured(Options),
}

#[test]
fn enum_payloads_use_serde_tagging_and_generated_defaults() {
    let schema = Nested::Configured(Options { enabled: true }).schema();
    let value = schema.default_value();
    assert_eq!(value, json!({"configured": {"enabled": true}}));
    assert!(schema.accepts(&value));
    let typed: Nested = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(typed, Nested::Configured(Options { enabled: true }));
    assert_eq!(serde_json::to_value(typed).unwrap(), value);
    assert_eq!(serde_json::to_value(Nested::None).unwrap(), json!("none"));
    for invalid in [
        json!("configured"),
        json!({"configured": {}}),
        json!({"configured": {"enabled": true, "unknown": 0}}),
    ] {
        assert!(serde_json::from_value::<Nested>(invalid).is_err());
    }
}
