// Integration tests for the explicit TaskStatus machine: the daemon-owned
// transition table (types.rs) and the audit trail the ledger writer emits
// for human (off-machine) status overrides.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openloop::core::task_ledger::{update_task, TaskPatch};
use openloop::core::types::*;

// Ledger writes resolve the app home from OPENLOOP_HOME (the override audit
// lands in the global run/events.jsonl); env mutation must be serialized
// across tests in this process.
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

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap()
}

fn sandbox(name: &str) -> Sandbox {
    let home = std::env::temp_dir().join(format!("ol-status-home-{name}-{}", std::process::id()));
    let target =
        std::env::temp_dir().join(format!("ol-status-target-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&target).unwrap();

    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);
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

fn make_task(id: &str, status: TaskStatus, attempts: u32) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status,
        risk: TaskRisk::LowRisk,
        scope: None,
        source: TaskSource {
            source_type: "human".into(),
            r#ref: "test".into(),
        },
        spec_id: None,
        branch: None,
        owner: None,
        acceptance_criteria: vec![],
        attempts,
        last_failure_signature: None,
        promotion: TaskPromotion::ManualOnly,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn override_events(home: &Path) -> Vec<serde_json::Value> {
    let raw = fs::read_to_string(home.join("run").join("events.jsonl")).unwrap_or_default();
    raw.lines()
        .filter(|line| line.contains("task_status_overridden"))
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn daemon_transition_table_is_exhaustive_for_writers() {
    // Every edge a daemon writer actually takes: pre-flight blocks fire from
    // Proposed/Planned/Ready, rollback restores them, the run loop moves
    // InProgress to its outcomes, review approval readies a task, promotion
    // closes out Done.
    let daemon_edges = [
        (TaskStatus::Proposed, TaskStatus::InProgress),
        (TaskStatus::Planned, TaskStatus::InProgress),
        (TaskStatus::Ready, TaskStatus::InProgress),
        (TaskStatus::Proposed, TaskStatus::Blocked),
        (TaskStatus::Planned, TaskStatus::Blocked),
        (TaskStatus::Ready, TaskStatus::Blocked),
        (TaskStatus::InProgress, TaskStatus::Proposed),
        (TaskStatus::InProgress, TaskStatus::Planned),
        (TaskStatus::InProgress, TaskStatus::Ready),
        (TaskStatus::InProgress, TaskStatus::AwaitingApproval),
        (TaskStatus::InProgress, TaskStatus::Done),
        (TaskStatus::InProgress, TaskStatus::Failed),
        (TaskStatus::InProgress, TaskStatus::Blocked),
        (TaskStatus::AwaitingApproval, TaskStatus::Ready),
        // A landed merge supersedes any prior status (mid-run PR, or a task
        // a human re-readied after the previous attempt).
        (TaskStatus::Done, TaskStatus::Promoted),
        (TaskStatus::InProgress, TaskStatus::Promoted),
        (TaskStatus::Ready, TaskStatus::Promoted),
        (TaskStatus::AwaitingApproval, TaskStatus::Promoted),
    ];
    for (from, to) in daemon_edges {
        assert!(
            TaskStatus::is_daemon_transition(from, to),
            "{from:?} -> {to:?} must be a daemon-owned edge",
        );
    }

    // Off-machine samples: human overrides, never daemon writes.
    assert!(!TaskStatus::is_daemon_transition(
        TaskStatus::Ready,
        TaskStatus::Done
    ));
    assert!(!TaskStatus::is_daemon_transition(
        TaskStatus::Blocked,
        TaskStatus::Ready
    ));
    assert!(!TaskStatus::is_daemon_transition(
        TaskStatus::Failed,
        TaskStatus::Ready
    ));
    assert!(!TaskStatus::is_daemon_transition(
        TaskStatus::Done,
        TaskStatus::Failed
    ));
    assert!(!TaskStatus::is_daemon_transition(
        TaskStatus::Promoted,
        TaskStatus::Ready
    ));
    assert!(!TaskStatus::is_daemon_transition(
        TaskStatus::Ready,
        TaskStatus::Cancelled
    ));
}

#[test]
fn off_machine_status_write_is_audited() {
    let _env = env_lock();
    let s = sandbox("audit");
    write_ledger(
        &s.target,
        vec![
            make_task("t-off", TaskStatus::Ready, 0),
            make_task("t-legal", TaskStatus::Ready, 0),
        ],
    );

    // Off-machine override through the ledger choke point: allowed, audited.
    update_task(
        &s.target,
        "t-off",
        &TaskPatch {
            status: Some(TaskStatus::Done),
            ..Default::default()
        },
    )
    .unwrap();

    let events = override_events(&s.home);
    assert_eq!(events.len(), 1, "events: {events:?}");
    assert_eq!(events[0]["taskId"], "t-off");
    assert_eq!(events[0]["taskStatus"], "done");
    assert_eq!(events[0]["detail"], "ready -> done");

    // A legal daemon edge from the same start status gains no audit line.
    update_task(
        &s.target,
        "t-legal",
        &TaskPatch {
            status: Some(TaskStatus::InProgress),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        override_events(&s.home).len(),
        1,
        "legal edge must not be audited as an override",
    );
}

#[test]
fn daemon_iteration_emits_no_override_audit() {
    let _env = env_lock();
    let s = sandbox("daemon-clean");
    // Stub agent so the implement iteration completes instantly instead of
    // invoking a real provider.
    let mut config = openloop::core::project_config::default_project_config();
    config.agent = Some(AgentConfig {
        agent_type: Some("custom".into()),
        command: Some("true".into()),
        extra_args: None,
    });
    fs::create_dir_all(s.target.join(".openloop")).unwrap();
    fs::write(
        s.target.join(".openloop").join("project.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
    write_ledger(&s.target, vec![make_task("t-run", TaskStatus::Ready, 0)]);

    openloop::core::worker::run_worker_tick(&openloop::core::now_iso()).unwrap();

    // The iteration must have completed (proving transitions ran)...
    let ledger = openloop::core::task_ledger::load_task_ledger(&s.target).unwrap();
    let task = ledger
        .tasks
        .iter()
        .find(|task| task.id == "t-run")
        .expect("task still present");
    assert_eq!(
        task.status,
        TaskStatus::Done,
        "implement iteration must complete",
    );

    // ...and every daemon transition must be a table edge: none of them may
    // surface as a human override in the audit log.
    assert_eq!(
        override_events(&s.home).len(),
        0,
        "daemon-only transitions must not emit task_status_overridden",
    );
}
