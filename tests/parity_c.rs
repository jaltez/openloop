// Parity ports (batch C): task ledger summarize/ordering, project registry,
// enqueue shape, lifecycle hooks, notifications filter matrix, review extras,
// active runs, cron malformed extras, and run report generation/formatting.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use openloop::core::active_runs::{
    kill_active_runs, list_active_runs, register_active_run, unregister_active_run,
};
use openloop::core::cron::matches_five_field_cron;
use openloop::core::daemon_state::{create_default_daemon_state, save_daemon_state};
use openloop::core::global_config::{default_global_config, save_global_config};
use openloop::core::hooks::{run_lifecycle_hooks, LifecycleHookPayload};
use openloop::core::notifications::fire_notifications;
use openloop::core::now_iso;
use openloop::core::paths::runtime_dir;
use openloop::core::project_registry::{add_project, list_projects, save_projects_registry};
use openloop::core::review::{check_scope_drift, extract_changed_files};
use openloop::core::run_report::{format_run_report, generate_run_report};
use openloop::core::task_ledger::{
    get_task, list_tasks, load_task_ledger, summarize_queue, summarize_tasks, TaskListFilters,
};
use openloop::core::types::*;

// The active-runs registry is process-global and cargo test runs these tests
// on parallel threads; serialize the tests that mutate it.
static ACTIVE_RUNS_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("ol-parity-c-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn as_str(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

fn make_task(id: &str, status: TaskStatus, risk: TaskRisk, updated_at: &str) -> ProjectTask {
    ProjectTask {
        id: id.to_string(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status,
        risk,
        scope: None,
        source: TaskSource {
            source_type: "human".to_string(),
            r#ref: "test".to_string(),
        },
        spec_id: None,
        branch: None,
        owner: None,
        acceptance_criteria: vec![],
        attempts: 0,
        last_failure_signature: None,
        promotion: TaskPromotion::PullRequest,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: "2026-01-01T00:00:00.000Z".to_string(),
        updated_at: updated_at.to_string(),
    }
}

fn write_ledger(project_root: &Path, tasks: Vec<ProjectTask>) {
    let ledger = TaskLedger {
        version: 1,
        updated_at: now_iso(),
        tasks,
    };
    fs::create_dir_all(project_root.join(".openloop")).unwrap();
    fs::write(
        project_root.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

/// Mirrors `basePolicy` from the TS review suite.
fn review_policy() -> ProjectPolicy {
    ProjectPolicy {
        version: 1,
        scope: PolicyScope {
            allow_globs: vec![],
            deny_globs: vec!["secrets/**".to_string()],
            high_risk_areas: vec!["src/auth/**".to_string()],
        },
        risk_classes: std::collections::BTreeMap::from([
            (
                "low-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: true,
                    requires_human_review: false,
                },
            ),
            (
                "medium-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: false,
                    requires_human_review: true,
                },
            ),
            (
                "high-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: false,
                    requires_human_review: true,
                },
            ),
        ]),
        self_healing: SelfHealingPolicy {
            enabled: true,
            allowed_task_kinds: vec![
                "lint-fix".to_string(),
                "type-fix".to_string(),
                "localized-test-fix".to_string(),
            ],
        },
        promotion: PolicyPromotionModes {
            low_risk_mode: TaskPromotion::AutoMerge,
            medium_risk_mode: TaskPromotion::PullRequest,
            high_risk_mode: TaskPromotion::PullRequest,
        },
    }
}

fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn hook_script(dir: &Path, name: &str, stdout: &str) -> PathBuf {
    let path = dir.join(format!("{name}.sh"));
    fs::write(&path, format!("#!/bin/sh\necho '{stdout}'\n")).unwrap();
    set_executable(&path);
    path
}

fn command_hook(path: &Path, events: &[&str]) -> LifecycleHookConfig {
    LifecycleHookConfig {
        hook_type: "command".to_string(),
        events: events.iter().map(|e| e.to_string()).collect(),
        command: Some(path.to_string_lossy().to_string()),
        url: None,
        timeout_seconds: Some(10),
        disabled: None,
    }
}

fn minimal_project_config(hooks: Vec<LifecycleHookConfig>) -> ProjectConfig {
    ProjectConfig {
        version: 1,
        project: ProjectMeta {
            alias: Some("demo".to_string()),
            repo_root: None,
            initialized_at: None,
        },
        pi: PiConfig {
            model: None,
            prompt_files: vec![],
        },
        agent: None,
        runtime: ProjectRuntime {
            use_worktree: false,
            branch_prefix: "openloop".to_string(),
            pr_command: None,
            worktree_setup_command: None,
        },
        validation: ProjectValidation {
            lint_command: None,
            test_command: None,
            typecheck_command: None,
        },
        risk: ProjectRisk {
            default_unknown_area_classification: "medium-risk".to_string(),
            require_policy_for_auto_merge: true,
        },
        issue_source: None,
        hooks: Some(hooks),
        schedule: None,
        schedule_state: None,
        review: None,
        verification: None,
    }
}

fn notification_payload(event: &str) -> LifecycleHookPayload {
    LifecycleHookPayload {
        event: event.to_string(),
        project: "myapp".to_string(),
        task_id: "task-1".to_string(),
        message: "Task completed".to_string(),
        timestamp: now_iso(),
        ..Default::default()
    }
}

/// Fire notifications at a webhook channel bound to a throwaway local
/// listener and report the raw HTTP request if the channel fired. This is the
/// deterministic Rust equivalent of the TS suites' command/desktop channels.
fn captured_webhook_request(events: &[&str], event: &str) -> Option<String> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    let server = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut conn, _)) => {
                    conn.set_read_timeout(Some(Duration::from_millis(300))).ok();
                    let mut request = String::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        match conn.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => request.push_str(&String::from_utf8_lossy(&buf[..n])),
                            Err(_) => break,
                        }
                    }
                    let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                    let _ = tx.send(request);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let mut config = default_global_config();
    config.notification_channels = Some(vec![NotificationChannelConfig::Webhook {
        url: format!("http://{addr}/hook"),
        events: events.iter().map(|e| e.to_string()).collect(),
    }]);
    fire_notifications(&config, &notification_payload(event)).unwrap();

    let request = rx.recv_timeout(Duration::from_secs(2)).ok();
    let _ = server.join();
    request
}

/// App home with a global config (daily budget $25) and daemon state showing
/// $4.23 spent — the TS run-report fixture.
fn report_home(name: &str) -> TempDir {
    let home = TempDir::new(name);
    let mut config = default_global_config();
    config.budgets.daily_cost_usd = 25.0;
    save_global_config(&config, Some(home.as_str())).unwrap();

    let mut state = create_default_daemon_state();
    state.budget_spent_usd = 4.23;
    fs::create_dir_all(runtime_dir(Some(home.as_str()))).unwrap();
    save_daemon_state(&state, Some(home.as_str())).unwrap();

    save_projects_registry(
        &ProjectsRegistry {
            version: 1,
            projects: vec![],
        },
        Some(home.as_str()),
    )
    .unwrap();
    home
}

// ---------------------------------------------------------------------------
// Task ledger: summarize + ordering + get_task
// ---------------------------------------------------------------------------

#[test]
fn summarize_tasks_zero_fills_status_and_risk_maps_for_empty_input() {
    let summary = summarize_tasks(&[]);
    assert_eq!(summary.total, 0);
    // Every status key is present and zeroed (TS parity for JSON consumers).
    assert_eq!(summary.by_status.len(), 10);
    for key in [
        "proposed",
        "planned",
        "ready",
        "awaiting-approval",
        "in_progress",
        "blocked",
        "done",
        "failed",
        "cancelled",
        "promoted",
    ] {
        assert_eq!(summary.by_status.get(key), Some(&0), "status {key}");
    }
    assert_eq!(summary.by_risk.len(), 3);
    for key in ["low-risk", "medium-risk", "high-risk"] {
        assert_eq!(summary.by_risk.get(key), Some(&0), "risk {key}");
    }
}

#[test]
fn summarize_tasks_counts_by_status_and_risk() {
    let tasks = vec![
        make_task(
            "a",
            TaskStatus::Proposed,
            TaskRisk::LowRisk,
            "2026-03-08T10:00:00.000Z",
        ),
        make_task(
            "b",
            TaskStatus::Done,
            TaskRisk::LowRisk,
            "2026-03-08T11:00:00.000Z",
        ),
        make_task(
            "c",
            TaskStatus::Proposed,
            TaskRisk::HighRisk,
            "2026-03-08T12:00:00.000Z",
        ),
    ];
    let summary = summarize_tasks(&tasks);
    assert_eq!(summary.total, 3);
    assert_eq!(summary.by_status.get("proposed"), Some(&2));
    assert_eq!(summary.by_status.get("done"), Some(&1));
    assert_eq!(summary.by_risk.get("low-risk"), Some(&2));
    assert_eq!(summary.by_risk.get("high-risk"), Some(&1));
}

#[test]
fn summarize_queue_counts_queued_and_blocked_tasks() {
    let empty = TaskLedger {
        version: 1,
        updated_at: now_iso(),
        tasks: vec![],
    };
    assert_eq!(summarize_queue(&empty), (0, 0));

    let ledger = TaskLedger {
        version: 1,
        updated_at: now_iso(),
        tasks: vec![
            make_task(
                "a",
                TaskStatus::Proposed,
                TaskRisk::LowRisk,
                "2026-03-08T10:00:00.000Z",
            ),
            make_task(
                "b",
                TaskStatus::Ready,
                TaskRisk::LowRisk,
                "2026-03-08T11:00:00.000Z",
            ),
            make_task(
                "c",
                TaskStatus::Blocked,
                TaskRisk::LowRisk,
                "2026-03-08T12:00:00.000Z",
            ),
            make_task(
                "d",
                TaskStatus::Done,
                TaskRisk::LowRisk,
                "2026-03-08T13:00:00.000Z",
            ),
        ],
    };
    // Queue = proposed + ready (done and blocked are not queued), blocked = 1.
    assert_eq!(summarize_queue(&ledger), (2, 1));
}

#[test]
fn list_tasks_orders_by_updated_at_desc_with_id_tiebreak() {
    let project = TempDir::new("ledger-list");
    write_ledger(
        &project,
        vec![
            make_task(
                "task-old",
                TaskStatus::Done,
                TaskRisk::LowRisk,
                "2026-03-08T12:00:00.000Z",
            ),
            make_task(
                "task-new",
                TaskStatus::Done,
                TaskRisk::LowRisk,
                "2026-03-08T13:00:00.000Z",
            ),
            make_task(
                "task-blocked",
                TaskStatus::Blocked,
                TaskRisk::HighRisk,
                "2026-03-08T09:30:00.000Z",
            ),
            // Same updated_at as task-old: id ascending breaks the tie.
            make_task(
                "tie-b",
                TaskStatus::Done,
                TaskRisk::LowRisk,
                "2026-03-08T12:00:00.000Z",
            ),
            make_task(
                "tie-a",
                TaskStatus::Done,
                TaskRisk::LowRisk,
                "2026-03-08T12:00:00.000Z",
            ),
        ],
    );

    let filters = TaskListFilters {
        status: Some(TaskStatus::Done),
        risk: Some(TaskRisk::LowRisk),
    };
    let done = list_tasks(&project, &filters).unwrap();
    let ids: Vec<&str> = done.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["task-new", "task-old", "tie-a", "tie-b"]);

    let blocked = list_tasks(
        &project,
        &TaskListFilters {
            status: Some(TaskStatus::Blocked),
            risk: None,
        },
    )
    .unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].id, "task-blocked");

    let all = list_tasks(&project, &TaskListFilters::default()).unwrap();
    let ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["task-new", "task-old", "tie-a", "tie-b", "task-blocked"]
    );
}

#[test]
fn get_task_returns_hit_and_errors_on_miss() {
    let project = TempDir::new("ledger-get");
    write_ledger(
        &project,
        vec![make_task(
            "task-lookup",
            TaskStatus::Done,
            TaskRisk::LowRisk,
            "2026-03-08T12:00:00.000Z",
        )],
    );

    let task = get_task(&project, "task-lookup").unwrap();
    assert_eq!(task.id, "task-lookup");
    assert_eq!(task.title, "Task task-lookup");

    let miss = get_task(&project, "missing").unwrap_err();
    assert!(miss.contains("Unknown task id"), "unexpected error: {miss}");
}

// ---------------------------------------------------------------------------
// Project registry
// ---------------------------------------------------------------------------

#[test]
fn add_project_round_trips_through_list_projects() {
    let home = TempDir::new("registry-home");
    let added = add_project("demo", "/tmp/demo", Some(home.as_str())).unwrap();
    assert_eq!(added.alias, "demo");
    assert_eq!(added.path, "/tmp/demo");
    assert!(!added.initialized);

    let projects = list_projects(Some(home.as_str())).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].alias, "demo");
    assert_eq!(projects[0].path, "/tmp/demo");
}

#[test]
fn add_project_rejects_duplicate_alias() {
    let home = TempDir::new("registry-dup");
    add_project("demo", "/tmp/demo", Some(home.as_str())).unwrap();
    let dup = add_project("demo", "/tmp/other", Some(home.as_str())).unwrap_err();
    assert!(dup.contains("already exists"), "unexpected error: {dup}");
    // The registry still holds exactly one entry.
    assert_eq!(list_projects(Some(home.as_str())).unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// Enqueue CLI shape
// ---------------------------------------------------------------------------

#[test]
fn enqueue_creates_proposed_feature_task_from_ref() {
    let home = TempDir::new("enqueue-home");
    let project = TempDir::new("enqueue-project");
    add_project("demo", project.as_str(), Some(home.as_str())).unwrap();
    // TS fixture pre-seeds an empty ledger before enqueueing.
    write_ledger(&project, vec![]);

    let output = Command::new(env!("CARGO_BIN_EXE_openloop"))
        .args(["enqueue", "--project", "demo", "--ref", "ISSUE-123"])
        .env("OPENLOOP_HOME", home.as_str())
        .output()
        .expect("openloop binary should start");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"taskId\": \"issue-123\""),
        "stdout: {stdout}"
    );

    let ledger = load_task_ledger(&project).unwrap();
    assert_eq!(ledger.tasks.len(), 1);
    let task = &ledger.tasks[0];
    assert_eq!(task.id, "issue-123");
    assert_eq!(task.kind, TaskKind::Feature);
    assert_eq!(task.status, TaskStatus::Proposed);
    assert_eq!(task.source.source_type, "issue");
    assert_eq!(task.source.r#ref, "ISSUE-123");
    assert!(task.owner.is_none());
}

// ---------------------------------------------------------------------------
// Lifecycle hooks
// ---------------------------------------------------------------------------

#[test]
fn run_lifecycle_hooks_collects_notes_and_ors_manual_review() {
    let dir = TempDir::new("hooks");
    let global_review = hook_script(
        &dir,
        "global-review",
        r#"{"note":"Needs review","requireManualReview":true}"#,
    );
    let global_noop = hook_script(
        &dir,
        "global-noop",
        r#"{"note":"No gating","requireManualReview":false}"#,
    );
    let project_hook = hook_script(&dir, "project-note", r#"{"note":"Project says hi"}"#);
    let wrong_event = hook_script(&dir, "wrong-event", r#"{"note":"should not appear"}"#);

    let mut global = default_global_config();
    global.hooks = Some(vec![
        command_hook(&global_review, &["promotion-auto-merge-queued"]),
        command_hook(&global_noop, &["promotion-auto-merge-queued"]),
        // Different event filter: must be skipped entirely.
        command_hook(&wrong_event, &["some-other-event"]),
    ]);
    let project = minimal_project_config(vec![command_hook(
        &project_hook,
        &["promotion-auto-merge-queued"],
    )]);

    let payload = LifecycleHookPayload {
        event: "promotion-auto-merge-queued".to_string(),
        project: "demo".to_string(),
        task_id: "task-1".to_string(),
        message: "Promotion queued".to_string(),
        timestamp: now_iso(),
        ..Default::default()
    };

    let result = run_lifecycle_hooks(&global, Some(&project), &payload, None);

    // Notes carry the "[scope:type] note" prefix; global hooks run first.
    assert_eq!(
        result.notes,
        vec![
            "[global:command] Needs review",
            "[global:command] No gating",
            "[project:command] Project says hi",
        ]
    );
    // requireManualReview is OR-ed across hooks (true | false | unset).
    assert!(result.require_manual_review);
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

#[test]
fn fire_notifications_never_fails_with_no_channels_configured() {
    let mut config = default_global_config();
    config.notification_channels = None;
    assert!(fire_notifications(&config, &notification_payload("task-complete")).is_ok());
}

#[test]
fn fire_notifications_fires_only_for_matching_event_filters() {
    // Exact event match: the channel fires.
    let request = captured_webhook_request(&["task-complete"], "task-complete")
        .expect("exact event match should fire the webhook");
    assert!(request.contains("POST /hook"), "request: {request}");
    assert!(request.contains("task-1"), "request: {request}");

    // Wildcard filter matches any event.
    let request = captured_webhook_request(&["*"], "budget-blocked")
        .expect("wildcard filter should fire the webhook");
    assert!(request.contains("budget-blocked"), "request: {request}");

    // No match: nothing fires within the listener window.
    let request = captured_webhook_request(&["task-failed"], "task-complete");
    assert!(request.is_none(), "non-matching filter must not fire");
}

#[test]
fn fire_notifications_swallows_desktop_channel_failures() {
    // notify-send typically does not exist in test environments; firing must
    // still succeed (TS parity: errors are swallowed, never thrown).
    let mut config = default_global_config();
    config.notification_channels = Some(vec![NotificationChannelConfig::Desktop {
        events: vec!["task-complete".to_string()],
    }]);
    assert!(fire_notifications(&config, &notification_payload("task-complete")).is_ok());
}

// ---------------------------------------------------------------------------
// Review extras
// ---------------------------------------------------------------------------

#[test]
fn scope_drift_warns_on_high_risk_area_touch() {
    let mut task = make_task(
        "t1",
        TaskStatus::Ready,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
    );
    task.scope = Some(TaskScope {
        paths: vec!["src/**".to_string()],
    });
    let findings = check_scope_drift(&["src/auth/login.ts".to_string()], &task, &review_policy());
    let high_risk = findings
        .iter()
        .find(|f| f.rule == "high-risk-area-touched")
        .expect("high-risk-area finding expected");
    assert_eq!(high_risk.severity, ReviewFindingSeverity::Warn);
    assert_eq!(high_risk.file.as_deref(), Some("src/auth/login.ts"));
    // The file is inside the declared scope, so no scope-drift warning fires.
    assert!(!findings.iter().any(|f| f.rule == "scope-drift"));
}

#[test]
fn scope_drift_does_not_flag_tasks_without_declared_scope() {
    let task = make_task(
        "t1",
        TaskStatus::Ready,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
    );
    assert!(task.scope.is_none());
    let findings = check_scope_drift(&["README.md".to_string()], &task, &review_policy());
    assert!(
        !findings.iter().any(|f| f.rule == "scope-drift"),
        "findings: {findings:?}"
    );
    // README matches neither deny globs nor high-risk areas: nothing at all.
    assert!(findings.is_empty());
}

#[test]
fn extract_changed_files_returns_empty_for_empty_patch() {
    assert!(extract_changed_files("").is_empty());
}

// ---------------------------------------------------------------------------
// Active runs
// ---------------------------------------------------------------------------

#[test]
fn active_runs_register_list_and_unregister() {
    let _guard = ACTIVE_RUNS_LOCK.lock();

    let id = register_active_run("demo", 424242);
    let runs = list_active_runs();
    assert!(
        runs.iter()
            .any(|r| r.id == id && r.project_alias == "demo" && r.pgid == 424242),
        "run missing from registry"
    );

    unregister_active_run(id);
    assert!(list_active_runs().is_empty());
}

#[test]
fn kill_active_runs_terminates_registered_children_and_clears_registry() {
    use std::os::unix::process::{CommandExt, ExitStatusExt};

    let _guard = ACTIVE_RUNS_LOCK.lock();

    let mut children = Vec::new();
    for alias in ["one", "two"] {
        let child = Command::new("sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .expect("sleep should spawn");
        // process_group(0) makes the child its own group leader: pgid == pid.
        let pgid = child.id() as i32;
        register_active_run(alias, pgid);
        children.push(child);
    }
    assert_eq!(list_active_runs().len(), 2);

    kill_active_runs();

    assert!(list_active_runs().is_empty(), "registry must be drained");
    for child in &mut children {
        let status = child.wait().expect("child should be reaped");
        assert_eq!(status.signal(), Some(15), "expected SIGTERM termination");
    }

    // A second sweep with an empty registry is a no-op.
    kill_active_runs();
    assert!(list_active_runs().is_empty());
}

// ---------------------------------------------------------------------------
// Cron malformed extras
// ---------------------------------------------------------------------------

#[test]
fn rejects_out_of_range_dom_month_dow_and_empty_list_entries() {
    use chrono::TimeZone;
    let ts = chrono::Utc
        .with_ymd_and_hms(2026, 1, 15, 10, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    for expr in [
        "0 0 32 * *",   // dom out of range (1-31)
        "0 0 1 13 *",   // month out of range (1-12)
        "0 0 * * 7",    // dow out of range (0-6)
        "1,,2 * * * *", // empty list entry
    ] {
        assert!(
            matches_five_field_cron(expr, ts).is_err(),
            "expected rejection for {expr}"
        );
    }
}

// ---------------------------------------------------------------------------
// Run report
// ---------------------------------------------------------------------------

#[test]
fn generate_run_report_is_empty_when_no_projects_exist() {
    let home = report_home("report-empty");
    let report = generate_run_report(None, Some(home.as_str())).unwrap();
    assert_eq!(report.total_completed, 0);
    assert_eq!(report.total_failed, 0);
    assert_eq!(report.total_blocked, 0);
    assert!(report.projects.is_empty());
    assert_eq!(report.period_label, "last 24 hours");
    assert!((report.budget_spent_usd - 4.23).abs() < 1e-9);
}

#[test]
fn generate_run_report_buckets_totals_and_tracks_budget() {
    let home = report_home("report-buckets");
    let project = TempDir::new("report-project");
    save_projects_registry(
        &ProjectsRegistry {
            version: 1,
            projects: vec![LinkedProject {
                alias: "myapp".to_string(),
                path: project.to_string_lossy().to_string(),
                default_branch: None,
                initialized: true,
                created_at: now_iso(),
                updated_at: now_iso(),
            }],
        },
        Some(home.as_str()),
    )
    .unwrap();

    let now = now_iso();
    let mut done = make_task("add-validation", TaskStatus::Done, TaskRisk::LowRisk, &now);
    done.last_run = Some(TaskRunSummary {
        completed_at: now.clone(),
        mode: "implement".to_string(),
        role: None,
        pi_exit_code: Some(0),
        outcome: RunOutcome::Completed,
        base_branch: None,
        validation: vec![ValidationSummary {
            name: ValidationName::Lint,
            command: "bun run lint".to_string(),
            exit_code: 0,
        }],
        promotion_decision: PromotionDecision::AutoMergeEligible,
        effective_promotion_mode: TaskPromotion::AutoMerge,
        promotion_action: PromotionAction::QueueAutoMerge,
        promotion_artifact_path: None,
        promotion_artifact_state: PromotionArtifactStatus::Applied,
        promotion_result_artifact_path: None,
    });
    let failed = make_task("fix-auth", TaskStatus::Failed, TaskRisk::MediumRisk, &now);
    let blocked = make_task("hold-me", TaskStatus::Blocked, TaskRisk::LowRisk, &now);
    let wip = make_task("wip-task", TaskStatus::InProgress, TaskRisk::LowRisk, &now);
    let ready = make_task("ready-task", TaskStatus::Ready, TaskRisk::LowRisk, &now);
    // Updated in 2020: outside the default 24h window, excluded everywhere.
    let stale = make_task(
        "stale-task",
        TaskStatus::Done,
        TaskRisk::LowRisk,
        "2020-01-01T00:00:00.000Z",
    );
    write_ledger(&project, vec![done, failed, blocked, wip, ready, stale]);

    let report = generate_run_report(None, Some(home.as_str())).unwrap();
    assert_eq!(report.total_completed, 1);
    assert_eq!(report.total_failed, 1);
    assert_eq!(report.total_blocked, 1);
    assert_eq!(report.total_idle, 0);
    assert_eq!(report.projects.len(), 1);

    let section = &report.projects[0];
    assert_eq!(section.alias, "myapp");
    assert!(!section.idle);
    assert_eq!(section.completed.len(), 1);
    assert_eq!(section.completed[0].task_id, "add-validation");
    assert_eq!(section.completed[0].risk, "low-risk");
    // last_run promotion action is carried onto the report entry.
    assert_eq!(section.completed[0].promotion, "queue-auto-merge");
    assert_eq!(section.completed[0].validation.len(), 1);
    assert_eq!(section.failed.len(), 1);
    assert_eq!(section.failed[0].task_id, "fix-auth");
    assert_eq!(section.blocked.len(), 1);
    // Ready and in_progress share the in-progress bucket.
    assert_eq!(section.in_progress.len(), 2);

    assert!((report.budget_spent_usd - 4.23).abs() < 1e-9);
    assert!((report.budget_remaining_usd - 20.77).abs() < 1e-6);
}

#[test]
fn format_run_report_contains_header_and_spend_line() {
    let home = report_home("report-format");
    let report = generate_run_report(None, Some(home.as_str())).unwrap();
    let output = format_run_report(&report);
    assert!(output.contains("=== OpenLoop Summary"), "output: {output}");
    assert!(output.contains("$4.23 spent"), "output: {output}");
    assert!(output.contains("$20.77 remaining"), "output: {output}");
}
