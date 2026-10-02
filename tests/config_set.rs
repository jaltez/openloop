use openloop::cli::coerce_config_set_value;

#[test]
fn whole_numbers_serialize_as_integers() {
    // Regression: json!(f64) always emits a float ("10.0") which failed the
    // u64/u32 round-trip in GlobalConfig.
    let value = coerce_config_set_value("number", "10", Some(1.0), None).unwrap();
    assert!(value.is_i64(), "expected integer, got {value}");
    assert_eq!(value.as_i64(), Some(10));

    let value = coerce_config_set_value("number", "5", Some(1.0), None).unwrap();
    assert!(value.is_i64());
}

#[test]
fn fractional_numbers_stay_floats() {
    let value = coerce_config_set_value("number", "12.5", Some(0.0), None).unwrap();
    assert!(value.is_f64());
    assert_eq!(value.as_f64(), Some(12.5));
}

#[test]
fn min_bound_is_enforced() {
    assert!(coerce_config_set_value("number", "0", Some(1.0), None).is_err());
    assert!(coerce_config_set_value("number", "1", Some(1.0), None).is_ok());
}

#[test]
fn non_finite_numbers_are_rejected() {
    // Regression: "Infinity"/"NaN" used to parse and then corrupt the config.
    assert!(coerce_config_set_value("number", "Infinity", None, None).is_err());
    assert!(coerce_config_set_value("number", "NaN", None, None).is_err());
    assert!(coerce_config_set_value("number", "abc", None, None).is_err());
}

#[test]
fn string_kind_passthrough_and_null() {
    let value = coerce_config_set_value("string", "round-robin", None, None).unwrap();
    assert_eq!(value.as_str(), Some("round-robin"));
    let value = coerce_config_set_value("string", "null", None, None).unwrap();
    assert!(value.is_null());
}

#[test]
fn integer_round_trip_through_global_config() {
    // Full round-trip: coerce a runtime tunable, apply, deserialize.
    let value = coerce_config_set_value("number", "900", Some(1.0), None).unwrap();
    let mut raw =
        serde_json::to_value(openloop::core::global_config::default_global_config()).unwrap();
    openloop::core::global_config::set_nested_json_value(
        &mut raw,
        "runtime.runTimeoutSeconds",
        value,
    );
    let config: openloop::core::types::GlobalConfig =
        serde_json::from_value(raw).expect("integer config must round-trip");
    assert_eq!(config.runtime.run_timeout_seconds, 900);
}
