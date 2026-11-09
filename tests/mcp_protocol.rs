// Integration tests for the MCP stdio server (`openloop mcp`): JSON-RPC
// framing, notification silence, control-plane tool listing/calls, and error
// semantics (isError results vs protocol errors).
//
// The message handler is private, so the tests drive the real stdio loop via
// the compiled binary with piped stdin/stdout.

use parking_lot::{Mutex, MutexGuard};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use openloop::core::project_registry::{add_project, mark_project_initialized};
use openloop::core::types::TaskLedger;

// Tests mutate OPENLOOP_HOME (the spawned child inherits it); env mutation
// must be serialized across tests in this process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Sandbox {
    home: PathBuf,
    target: PathBuf,
    saved_home: Option<String>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        match self.saved_home.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        let _ = fs::remove_dir_all(&self.home);
        let _ = fs::remove_dir_all(&self.target);
    }
}

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock()
}

fn sandbox(name: &str) -> Sandbox {
    let home = std::env::temp_dir().join(format!("ol-mcp-home-{name}-{}", std::process::id()));
    let target = std::env::temp_dir().join(format!("ol-mcp-target-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&target).unwrap();

    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);

    Sandbox {
        home,
        target,
        saved_home,
    }
}

/// Link an initialized project and seed an empty task ledger, like the CLI
/// `link` flow would leave the world.
fn seed_project(s: &Sandbox, alias: &str) {
    add_project(alias, &s.target.to_string_lossy(), None).unwrap();
    mark_project_initialized(alias, None).unwrap();
    let ledger = TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks: vec![],
    };
    fs::create_dir_all(s.target.join(".openloop")).unwrap();
    fs::write(
        s.target.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

/// Spawn `openloop mcp`, send one JSON-RPC message per line, then close stdin
/// and collect every response line the server emits before exiting. Returns
/// the parsed responses; panics when their count differs from `expected` so
/// both missing and spurious (e.g. answered notification) replies are caught.
fn mcp_round_trip(lines: &[&str], expected: usize) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_openloop"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn openloop mcp");

    {
        let mut stdin = child.stdin.take().expect("child stdin");
        for line in lines {
            writeln!(stdin, "{line}").expect("write request line");
        }
        stdin.flush().expect("flush request lines");
        // stdin drops here: the server drains its buffered input, flushes every
        // response, and exits on EOF — the pipe still holds the output bytes.
    }

    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);
    let mut responses = Vec::new();
    let mut raw = String::new();
    loop {
        raw.clear();
        let bytes = reader.read_line(&mut raw).expect("read response line");
        if bytes == 0 {
            break;
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        responses.push(serde_json::from_str(trimmed).expect("parse response JSON"));
    }
    let status = child.wait().expect("wait for mcp server");
    assert!(
        status.success(),
        "openloop mcp exited with {status} after {} responses",
        responses.len()
    );
    assert_eq!(
        responses.len(),
        expected,
        "expected {expected} responses (notifications must stay unanswered), got: {responses:?}"
    );
    responses
}

fn content_text(response: &serde_json::Value) -> String {
    response
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn result_json(response: &serde_json::Value) -> serde_json::Value {
    serde_json::from_str(&content_text(response)).expect("content text is JSON")
}

#[test]
fn mcp_server_speaks_jsonrpc_and_serves_control_plane_tools() {
    let _env = env_lock();
    let s = sandbox("control-plane");
    seed_project(&s, "demo");

    let responses = mcp_round_trip(
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"openloop_list_projects","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"openloop_add_task","arguments":{"project":"demo","title":"MCP task"}}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"openloop_task_show","arguments":{"project":"demo","taskId":"mcp-task"}}}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"openloop_bogus_tool","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":7,"method":"bogus/method","params":{}}"#,
        ],
        7, // notifications/initialized is a notification — no response
    );

    // initialize advertises the pinned protocol version and server identity.
    assert_eq!(
        responses[0].pointer("/result/protocolVersion"),
        Some(&serde_json::json!("2025-06-18"))
    );
    assert_eq!(
        responses[0].pointer("/result/serverInfo/name"),
        Some(&serde_json::json!("openloop"))
    );

    // tools/list exposes the control-plane tools.
    let names: Vec<&str> = responses[1]
        .pointer("/result/tools")
        .and_then(serde_json::Value::as_array)
        .expect("tools/list result has a tools array")
        .iter()
        .map(|tool| {
            tool.get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        })
        .collect();
    assert!(
        names.contains(&"openloop_list_projects"),
        "tools/list missing openloop_list_projects: {names:?}"
    );
    assert!(
        names.contains(&"openloop_add_task"),
        "tools/list missing openloop_add_task: {names:?}"
    );

    // tools/call list_projects returns the linked alias.
    let listed = result_json(&responses[2]);
    let has_demo = listed.as_array().is_some_and(|projects| {
        projects
            .iter()
            .any(|p| p.get("alias").and_then(serde_json::Value::as_str) == Some("demo"))
    });
    assert!(
        has_demo,
        "list_projects payload missing alias demo: {listed}"
    );

    // add + show round-trip creates a real ledger task.
    let shown = result_json(&responses[4]);
    assert_eq!(shown["id"], "mcp-task");
    assert_eq!(shown["title"], "MCP task");
    assert_eq!(shown["status"], "proposed");

    // unknown tool → isError result, not a protocol error.
    assert_eq!(
        responses[5].pointer("/result/isError"),
        Some(&serde_json::json!(true))
    );
    assert!(responses[5].get("error").is_none());

    // unknown method → -32601.
    assert_eq!(
        responses[6].pointer("/error/code"),
        Some(&serde_json::json!(-32601))
    );
}

#[test]
fn mcp_tool_errors_are_reported_as_iserror_results() {
    let _env = env_lock();
    let _s = sandbox("tool-error");

    let responses = mcp_round_trip(
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"openloop_list_tasks","arguments":{"project":"missing-alias"}}}"#,
        ],
        2, // notifications/initialized is a notification — no response
    );

    // The alias error surfaces as tool content carrying the alias text.
    let text = content_text(&responses[1]);
    assert!(
        text.contains("missing-alias"),
        "error content should mention the alias: {text}"
    );
    assert_eq!(
        responses[1].pointer("/result/isError"),
        Some(&serde_json::json!(true))
    );
    assert!(responses[1].get("error").is_none());
}

#[test]
fn mcp_server_never_replies_to_unknown_notifications() {
    let _env = env_lock();
    let _s = sandbox("notifications");

    let responses = mcp_round_trip(
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        ],
        2, // only initialize and tools/list answer; notifications stay silent
    );

    assert_eq!(responses[0].get("id"), Some(&serde_json::json!(1)));
    assert_eq!(responses[1].get("id"), Some(&serde_json::json!(2)));
}

#[test]
fn mcp_add_task_rejects_invalid_kind_and_risk_values() {
    let _env = env_lock();
    let s = sandbox("validate");
    seed_project(&s, "demo");

    let responses = mcp_round_trip(
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"openloop_add_task","arguments":{"project":"demo","title":"Bad risk","risk":"urgent"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"openloop_add_task","arguments":{"project":"demo","title":"Bad kind","kind":"hack"}}}"#,
        ],
        3, // notifications/initialized is a notification — no response
    );

    let risk_text = content_text(&responses[1]);
    assert!(
        risk_text.contains("Invalid risk: urgent"),
        "expected invalid-risk error, got: {risk_text}"
    );
    assert_eq!(
        responses[1].pointer("/result/isError"),
        Some(&serde_json::json!(true))
    );

    let kind_text = content_text(&responses[2]);
    assert!(
        kind_text.contains("Invalid kind: hack"),
        "expected invalid-kind error, got: {kind_text}"
    );
    assert_eq!(
        responses[2].pointer("/result/isError"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn mcp_review_tools_expose_promotion_detail_and_approval_packet() {
    let _env = env_lock();
    let s = sandbox("review");
    seed_project(&s, "demo");

    // PromotionDetail reads git working-tree state: the target must be a repo.
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
    ] {
        std::process::Command::new("git")
            .args(&args)
            .current_dir(&s.target)
            .output()
            .unwrap();
    }
    fs::write(s.target.join("README.md"), "x\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&s.target)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-qm", "init"])
        .current_dir(&s.target)
        .output()
        .unwrap();

    // A pending promotion artifact and an approval packet for review.
    let task_id = "fix-login";
    let artifact = openloop::core::types::PromotionArtifact {
        version: 1,
        created_at: "2026-01-02T00:00:00.000Z".into(),
        project_alias: "demo".into(),
        task_id: task_id.into(),
        base_branch: Some("main".into()),
        decision: openloop::core::types::PromotionDecision::ManualReview,
        action: openloop::core::types::PromotionAction::QueueReview,
        effective_promotion_mode: openloop::core::types::TaskPromotion::PullRequest,
        validation: vec![openloop::core::types::ValidationSummary {
            name: openloop::core::types::ValidationName::Lint,
            command: "true".into(),
            exit_code: 0,
        }],
        pi_exit_code: Some(0),
        outcome: openloop::core::types::RunOutcome::Completed,
        approval_packet_path: None,
        status: openloop::core::types::PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    };
    openloop::core::promotion_artifacts::write_promotion_artifact(&s.target, &artifact).unwrap();
    let packet = openloop::core::approval_packets::ApprovalPacketData {
        diff_stat: None,
        validation: vec![],
        review_findings: vec![],
        cost_usd: 0.12,
        cost_source: openloop::core::types::CostSource::Measured,
        run_summary_path: None,
        spec_path: None,
    };
    let task = openloop::core::types::ProjectTask {
        id: task_id.into(),
        title: "Fix login".into(),
        kind: openloop::core::types::TaskKind::Bugfix,
        status: openloop::core::types::TaskStatus::Done,
        risk: openloop::core::types::TaskRisk::MediumRisk,
        scope: None,
        source: openloop::core::types::TaskSource {
            source_type: "human".into(),
            r#ref: "test".into(),
        },
        spec_id: None,
        branch: Some("openloop/fix-login".into()),
        owner: None,
        acceptance_criteria: vec![],
        attempts: 1,
        last_failure_signature: None,
        promotion: openloop::core::types::TaskPromotion::PullRequest,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    };
    openloop::core::approval_packets::write_approval_packet(&s.target, &task, &packet).unwrap();
    fs::write(
        s.target.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&TaskLedger {
            version: 1,
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            tasks: vec![task],
        })
        .unwrap(),
    )
    .unwrap();

    let responses = mcp_round_trip(
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"openloop_promotion_show","arguments":{"project":"demo","taskId":"fix-login"}}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"openloop_approval_packet","arguments":{"project":"demo","taskId":"fix-login"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"openloop_promotion_show","arguments":{"project":"demo","taskId":"ghost"}}}"#,
        ],
        3,
    );

    let detail = result_json(&responses[0]);
    assert_eq!(detail["artifact"]["taskId"], "fix-login");
    assert_eq!(detail["artifact"]["action"], "queue-review");
    assert_eq!(detail["artifact"]["status"], "pending");
    assert_eq!(detail["taskBranch"], "openloop/fix-login");

    let packet = result_json(&responses[1]);
    assert_eq!(packet["taskId"], "fix-login");
    assert_eq!(packet["costUsd"], 0.12);
    assert_eq!(packet["costSource"], "measured");

    // Unknown task: isError result, not a protocol error.
    assert!(
        responses[2]
            .pointer("/result/isError")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "unknown task must be an isError result: {}",
        responses[2]
    );
}
