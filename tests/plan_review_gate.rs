// Integration tests for the plan-review gate: plan-mode risk gating and the
// `task approve` transition (TS: tests/integration/plan-review-gate.test.ts;
// the medium-risk case is covered in tests/worker_tick.rs).

use parking_lot::{Mutex, MutexGuard};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use openloop::core::scheduler::{run_project_iteration, IterationOptions, RunHooks};
use openloop::core::task_ledger::load_task_ledger;
use openloop::core::types::*;

// The scheduler resolves OPENLOOP_HOME for the global config; env mutation
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
    let home = std::env::temp_dir().join(format!("ol-gate-home-{name}-{}", std::process::id()));
    let target = std::env::temp_dir().join(format!("ol-gate-target-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&target).unwrap();

    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);
    // The linked project "p1" the CLI resolves by alias.
    fs::write(
        home.join("projects.json"),
        serde_json::to_string(&ProjectsRegistry {
            version: 1,
            projects: vec![LinkedProject {
                alias: "p1".into(),
                path: target.to_string_lossy().to_string(),
                default_branch: None,
                initialized: true,
                created_at: "2026-01-01T00:00:00.000Z".into(),
                updated_at: "2026-01-01T00:00:00.000Z".into(),
            }],
        })
        .unwrap(),
    )
    .unwrap();

    Sandbox {
        home,
        target,
        saved_home,
    }
}

fn write_global_config() {
    let home_str = std::env::var("OPENLOOP_HOME").unwrap();
    let home = Path::new(&home_str);
    fs::write(
        home.join("config.json"),
        serde_json::to_string(&openloop::core::global_config::default_global_config()).unwrap(),
    )
    .unwrap();
}

// Project config with a stub agent so plan runs complete instantly instead of
// invoking a real provider.
fn write_project_config(target: &Path, mutate: impl FnOnce(&mut ProjectConfig)) {
    let mut config = openloop::core::project_config::default_project_config();
    config.agent = Some(AgentConfig {
        agent_type: Some("custom".into()),
        command: Some("true".into()),
        extra_args: None,
    });
    mutate(&mut config);
    fs::create_dir_all(target.join(".openloop")).unwrap();
    fs::write(
        target.join(".openloop").join("project.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
}

fn write_ledger(target: &Path, tasks: Vec<ProjectTask>) {
    let ledger = TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks,
    };
    fs::create_dir_all(target.join(".openloop")).unwrap();
    fs::write(
        target.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

fn make_task(id: &str, status: TaskStatus, risk: TaskRisk) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status,
        risk,
        scope: None,
        source: TaskSource {
            source_type: "human".into(),
            r#ref: "test".into(),
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
        created_at: now.clone(),
        updated_at: now,
    }
}

fn linked_project(target: &Path) -> LinkedProject {
    LinkedProject {
        alias: "p1".into(),
        path: target.to_string_lossy().to_string(),
        default_branch: None,
        initialized: true,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    }
}

fn plan_run(target: &Path) -> SchedulerResult {
    run_project_iteration(
        &linked_project(target),
        &IterationOptions {
            model_override: None,
            timeout_ms: None,
            max_attempts_per_task: None,
            no_progress_repeat_limit: None,
        },
        &mut RunHooks {
            pi_runner: None,
            reviewer_runner: None,
            verifier_runner: None,
            validation_runner: None,
        },
    )
    .unwrap()
}

fn run_cli(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_openloop"))
        .args(args)
        .env("OPENLOOP_HOME", home)
        .output()
        .expect("spawn openloop binary")
}

#[test]
fn plan_run_gates_high_risk_tasks_to_awaiting_approval() {
    let _env = env_lock();
    let s = sandbox("high");
    write_global_config();
    write_project_config(&s.target, |c| {
        c.risk.default_unknown_area_classification = "high-risk".to_string();
        c.risk.require_policy_for_auto_merge = true;
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "gate-me-high",
            TaskStatus::Proposed,
            TaskRisk::HighRisk,
        )],
    );

    let result = plan_run(&s.target);

    assert_eq!(result.mode, "plan");
    assert_eq!(result.task_status, Some(TaskStatus::AwaitingApproval));

    let persisted = load_task_ledger(&s.target).unwrap();
    assert_eq!(persisted.tasks[0].status, TaskStatus::AwaitingApproval);
    assert!(persisted.tasks[0]
        .notes
        .as_deref()
        .unwrap_or(&[] as &[String])
        .iter()
        .any(|n| n.contains("awaiting human approval")));
}

#[test]
fn plan_run_lets_low_risk_tasks_go_straight_to_ready() {
    let _env = env_lock();
    let s = sandbox("low");
    write_global_config();
    write_project_config(&s.target, |c| {
        c.risk.default_unknown_area_classification = "low-risk".to_string();
        c.risk.require_policy_for_auto_merge = true;
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "gate-me-low",
            TaskStatus::Proposed,
            TaskRisk::LowRisk,
        )],
    );

    let result = plan_run(&s.target);

    assert_eq!(result.mode, "plan");
    assert_eq!(result.task_status, Some(TaskStatus::Ready));

    let persisted = load_task_ledger(&s.target).unwrap();
    assert_eq!(persisted.tasks[0].status, TaskStatus::Ready);
}

#[test]
fn task_approve_transitions_awaiting_approval_to_ready() {
    let _env = env_lock();
    let s = sandbox("approve");
    write_ledger(
        &s.target,
        vec![make_task(
            "awaiting-task",
            TaskStatus::AwaitingApproval,
            TaskRisk::MediumRisk,
        )],
    );

    let out = run_cli(
        &s.home,
        &[
            "task",
            "approve",
            "--project",
            "p1",
            "--task",
            "awaiting-task",
        ],
    );
    assert!(
        out.status.success(),
        "exit: {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let persisted = load_task_ledger(&s.target).unwrap();
    assert_eq!(persisted.tasks[0].status, TaskStatus::Ready);
    assert!(persisted.tasks[0]
        .notes
        .as_deref()
        .unwrap_or(&[] as &[String])
        .iter()
        .any(|n| n == "Approved via 'task approve'."));
}

#[test]
fn task_approve_fails_when_task_is_not_awaiting_approval() {
    let _env = env_lock();
    let s = sandbox("approve-guard");
    write_ledger(
        &s.target,
        vec![make_task(
            "ready-task",
            TaskStatus::Ready,
            TaskRisk::MediumRisk,
        )],
    );

    let out = run_cli(
        &s.home,
        &["task", "approve", "--project", "p1", "--task", "ready-task"],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not awaiting approval"), "stderr: {stderr}");

    // The ledger is untouched: still ready, no approval note appended.
    let persisted = load_task_ledger(&s.target).unwrap();
    assert_eq!(persisted.tasks[0].status, TaskStatus::Ready);
    assert!(persisted.tasks[0]
        .notes
        .as_deref()
        .unwrap_or(&[] as &[String])
        .iter()
        .all(|n| n != "Approved via 'task approve'."));
}
