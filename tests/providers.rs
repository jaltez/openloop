use serde_json::json;

use openloop::core::providers::extract_usage;
use openloop::core::scheduler::resolve_run_cost;
use openloop::core::types::CostSource;
use openloop::core::types::{AgentRunResult, AgentUsage};

#[test]
fn nested_snake_case_fields() {
    let v = json!({
        "usage": {"input_tokens": 100, "output_tokens": 50, "total_tokens": 150},
        "total_cost_usd": 0.42
    });
    let usage = extract_usage(&v).unwrap();
    assert_eq!(usage.input_tokens, Some(100.0));
    assert_eq!(usage.output_tokens, Some(50.0));
    assert_eq!(usage.total_tokens, Some(150.0));
    assert_eq!(usage.cost_usd, Some(0.42));
}

#[test]
fn camel_case_fields_and_cost_aliases() {
    let v = json!({
        "usage": {"inputTokens": 1, "outputTokens": 2, "totalTokens": 3, "cost_usd": 9.5}
    });
    let usage = extract_usage(&v).unwrap();
    assert_eq!(usage.cost_usd, Some(9.5));
    assert_eq!(usage.total_tokens, Some(3.0));
}

#[test]
fn top_level_total_tokens() {
    let v = json!({"total_tokens": 77});
    let usage = extract_usage(&v).unwrap();
    assert_eq!(usage.total_tokens, Some(77.0));
    assert!(usage.cost_usd.is_none());
}

#[test]
fn non_object_yields_none() {
    assert!(extract_usage(&json!(null)).is_none());
    assert!(extract_usage(&json!("text")).is_none());
    assert!(extract_usage(&json!(42)).is_none());
    // An object with no usage-shaped keys at all yields None.
    assert!(extract_usage(&json!({"hello": "world"})).is_none());
}

#[test]
fn cost_resolution_prefers_positive_measured() {
    let with_cost = AgentRunResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        usage: Some(AgentUsage {
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cost_usd: Some(1.5),
        }),
    };
    assert_eq!(
        resolve_run_cost(&with_cost, 0.1),
        (1.5, CostSource::Measured)
    );

    let zero = AgentRunResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        usage: Some(AgentUsage {
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cost_usd: Some(0.0),
        }),
    };
    assert_eq!(resolve_run_cost(&zero, 0.1), (0.1, CostSource::Estimated));

    let none = AgentRunResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        usage: None,
    };
    assert_eq!(resolve_run_cost(&none, 0.1), (0.1, CostSource::Estimated));
}
