// Integration tests for the daemon worker tick: the gates that guard every
// unattended iteration (pause, budget, max attempts, backpressure, schedules).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::TimeZone;
use openloop::core::types::*;
use openloop::core::worker::run_worker_tick;

// run_worker_tick resolves the app home from OPENLOOP_HOME; env mutation
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

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap()
}

fn sandbox(name: &str) -> Sandbox {
    let home = std::env::temp_dir().join(format!("ol-worker-home-{name}-{}", std::process::id()));
    let target =
        std::env::temp_dir().join(format!("ol-worker-target-{name}-{}", std::process::id()));
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

fn write_global_config(mutate: impl FnOnce(&mut GlobalConfig)) {
    let home_str = std::env::var("OPENLOOP_HOME").unwrap();
    let home = Path::new(&home_str);
    let mut config = openloop::core::global_config::default_global_config();
    mutate(&mut config);
    fs::write(
        home.join("config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
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

fn read_state(home: &Path) -> DaemonState {
    serde_json::from_str(&fs::read_to_string(home.join("run").join("daemon-state.json")).unwrap())
        .unwrap()
}

#[test]
fn paused_tick_short_circuits_without_running_anything() {
    let _env = env_lock();
    let s = sandbox("paused");
    write_global_config(|c| {
        c.budgets.daily_cost_usd = 25.0;
    });
    write_ledger(&s.target, vec![make_task("t", TaskStatus::Ready, 0)]);

    openloop::core::daemon_state::pause_daemon(None).unwrap();
    let state = run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();

    assert!(state.paused);
    assert!(state.current_run.is_none());
    // TS parity: paused is a daemon-level signal; the project row keeps
    // previous/idle because activeProjectAlias is null on this path.
    // Task untouched: no agent ran.
    let ledger = openloop::core::task_ledger::load_task_ledger(&s.target).unwrap();
    assert_eq!(ledger.tasks[0].status, TaskStatus::Ready);
}

#[test]
fn budget_blocked_tick_flags_state_and_skips_work() {
    let _env = env_lock();
    let s = sandbox("budget");
    write_global_config(|c| {
        c.budgets.daily_cost_usd = 1.0;
    });
    write_ledger(&s.target, vec![make_task("t", TaskStatus::Ready, 0)]);

    // Pre-spend past the ceiling.
    openloop::core::daemon_state::with_daemon_state(
        |state: &mut DaemonState| {
            state.budget_spent_usd = 1.5;
            state.budget_date = openloop::core::local_date_stamp();
        },
        None,
    )
    .unwrap();

    // The budget window anchors to the daemon's start date; use today so
    // the pre-spent amount stays in-window.
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let started_at = format!("{today}T00:00:00.000Z");
    let state = run_worker_tick(&started_at).unwrap();
    assert!(state.budget_blocked);
    assert!(state.current_run.is_none());
    assert!(state.active_project.is_none());
    // TS parity: the budget signal is state-level (budgetBlocked); the
    // project row keeps its previous lastResult since no project ran.

    // Task untouched.
    let ledger = openloop::core::task_ledger::load_task_ledger(&s.target).unwrap();
    assert_eq!(ledger.tasks[0].status, TaskStatus::Ready);
}

#[test]
fn review_backpressure_skips_project_at_pending_limit() {
    let _env = env_lock();
    let s = sandbox("backpressure");
    write_global_config(|c| {
        c.runtime.max_pending_reviews_per_project = Some(1);
    });
    write_ledger(&s.target, vec![make_task("t", TaskStatus::Ready, 0)]);

    // One pending promotion artifact => at the limit.
    let artifact = PromotionArtifact {
        version: 1,
        created_at: "2026-01-02T00:00:00.000Z".into(),
        project_alias: "p1".into(),
        task_id: "t".into(),
        base_branch: None,
        decision: PromotionDecision::ManualReview,
        action: PromotionAction::QueueReview,
        effective_promotion_mode: TaskPromotion::PullRequest,
        validation: vec![],
        pi_exit_code: None,
        outcome: RunOutcome::Completed,
        approval_packet_path: None,
        status: PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    };
    openloop::core::promotion_artifacts::write_promotion_artifact(&s.target, &artifact).unwrap();

    let state = run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();
    let p = state.projects.iter().find(|p| p.alias == "p1").unwrap();
    assert_eq!(p.last_result.as_deref(), Some("review-backpressure"));
    assert!(state.current_run.is_none());

    let log = fs::read_to_string(s.home.join("run").join("daemon.log")).unwrap_or_default();
    assert!(log.contains("review-backpressure"), "log: {log}");
}

#[test]
fn cron_schedule_fires_once_and_dedupes() {
    let _env = env_lock();
    let s = sandbox("schedule");
    write_global_config(|_| {});

    // Project config with an every-minute schedule and persisted last-fire.
    let mut config = openloop::core::project_config::default_project_config();
    config.schedule = Some(vec![ProjectSchedule {
        id: "tick".into(),
        cron: "* * * * *".into(),
        title: "Scheduled pass".into(),
        prompt: None,
    }]);
    let last_fired = chrono::Utc
        .timestamp_millis_opt(chrono::Utc::now().timestamp_millis() - 10 * 60_000)
        .single()
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    config.schedule_state = Some([("tick".to_string(), last_fired)].into_iter().collect());
    // Stub agent so the plan-mode run the tick schedules after enqueueing
    // completes instantly instead of invoking a real provider.
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
    write_ledger(&s.target, vec![]);

    let state = run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();
    let _ = state;

    let ledger = openloop::core::task_ledger::load_task_ledger(&s.target).unwrap();
    assert_eq!(ledger.tasks.len(), 1, "schedule must enqueue one task");
    let task = &ledger.tasks[0];
    assert_eq!(task.source.source_type, "schedule");
    assert_eq!(task.source.r#ref, "tick");
    // The same tick plans the new medium-risk task with the stub agent:
    // plan succeeds -> awaiting human approval (TS gating).
    assert_eq!(task.status, TaskStatus::AwaitingApproval);

    // Second tick within the same minute: no duplicate schedule fire. (The
    // idle pass may synthesize a continuous-improvement task since no
    // validations are configured — count only schedule-sourced tasks.)
    run_worker_tick("2026-01-01T00:00:05.000Z").unwrap();
    let ledger = openloop::core::task_ledger::load_task_ledger(&s.target).unwrap();
    let scheduled: Vec<_> = ledger
        .tasks
        .iter()
        .filter(|t| t.source.source_type == "schedule")
        .collect();
    assert_eq!(scheduled.len(), 1, "dedupe against the same fire window");
}

#[test]
fn idle_tick_with_no_work_writes_state_and_survives() {
    let _env = env_lock();
    let s = sandbox("idle");
    write_global_config(|_| {});
    write_ledger(&s.target, vec![]);

    let state = run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();
    assert!(state.current_run.is_none());
    assert!(!state.paused);
    assert!(!state.budget_blocked);

    // State persisted with the project row present.
    let persisted = read_state(&s.home);
    assert_eq!(persisted.projects.len(), 1);
    assert_eq!(persisted.projects[0].alias, "p1");
}

#[test]
fn max_attempts_task_is_not_run_again() {
    let _env = env_lock();
    let s = sandbox("maxattempts");
    write_global_config(|c| {
        c.runtime.max_attempts_per_task = 2;
    });
    // Task at the cap: run-once must refuse instead of invoking an agent.
    // (With no provider configured the tick would fail on the provider gate
    // if it tried to run — asserting the task stays untouched is the gate.)
    write_ledger(&s.target, vec![make_task("capped", TaskStatus::Ready, 2)]);

    let result = openloop::core::scheduler::select_next_task(
        &openloop::core::task_ledger::load_task_ledger(&s.target).unwrap(),
    );
    // Selection happens inside run_project_iteration; here we assert the
    // scheduler-level guard that caps attempts via runProjectIteration is
    // exercised by running one tick with a stub agent that would otherwise
    // mutate the ledger: use a no-op custom agent.
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

    let state = run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();
    let _ = state;

    let ledger = openloop::core::task_ledger::load_task_ledger(&s.target).unwrap();
    let task = ledger.tasks.iter().find(|t| t.id == "capped").unwrap();
    assert_eq!(
        task.status,
        TaskStatus::Blocked,
        "task at max attempts must be blocked, not re-run"
    );
    assert_eq!(
        task.last_failure_signature.as_deref(),
        Some("max-attempts-reached")
    );
    assert!(task
        .notes
        .as_deref()
        .unwrap_or(&[] as &[String])
        .iter()
        .any(|n| n.contains("max attempts")));
    let _ = result;
}
