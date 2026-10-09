use drip::param::Parameters;
use serde_json::json;

#[derive(Debug, PartialEq, drip::Choice)]
enum Mode {
    #[choice("first")]
    First,
    #[choice("second_mode")]
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
