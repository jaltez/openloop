// Behavioral ports of the TS implement-mode scheduler suites
// (tests/integration/validation-flow.test.ts + the implement cases of
// tests/integration/scheduler.test.ts). Each iteration runs end to end
// through run_project_iteration with a fake custom agent (worker_tick
// sandbox pattern): real subprocess spawns, real ledger persistence,
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use parking_lot::{Mutex, MutexGuard};

use openloop::core::scheduler::{run_project_iteration, IterationOptions, RunHooks};
use openloop::core::task_ledger::load_task_ledger;
use openloop::core::types::*;
use openloop::core::worker::run_worker_tick;

// The scheduler resolves global config (and the worker tick resolves the
// app home) from OPENLOOP_HOME; env mutation must be serialized in-process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Sandbox {
    home: PathBuf,
    target: PathBuf,
    scripts: PathBuf,
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
    let unique = format!("ol-sched-{name}-{}", std::process::id());
    let home = std::env::temp_dir().join(format!("{unique}-home"));
    let target = std::env::temp_dir().join(format!("{unique}-target"));
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(target.join(".openloop")).unwrap();
    let scripts = home.join("scripts");
    fs::create_dir_all(&scripts).unwrap();

    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);

    // Registry entry so run_worker_tick-based tests resolve the project.
    fs::write(
        home.join("projects.json"),
        serde_json::to_string(&ProjectsRegistry {
            version: 1,
            projects: vec![project_for(&target)],
        })
        .unwrap(),
    )
    .unwrap();

    Sandbox {
        home,
        target,
        scripts,
        saved_home,
    }
}

/// A small executable shell script; fake agents and validation commands.
fn write_script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn write_global_config(home: &Path, mutate: impl FnOnce(&mut GlobalConfig)) {
    let mut config = openloop::core::global_config::default_global_config();
    mutate(&mut config);
    fs::write(
        home.join("config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
}

fn write_project_config(target: &Path, mutate: impl FnOnce(&mut ProjectConfig)) {
    let mut config = openloop::core::project_config::default_project_config();
    config.project.alias = Some("demo".into());
    config.project.repo_root = Some(target.to_string_lossy().to_string());
    mutate(&mut config);
    fs::write(
        target.join(".openloop").join("project.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
}

/// Configure the fake agent: a custom provider running the given script.
fn use_custom_agent(config: &mut ProjectConfig, script: &Path) {
    config.agent = Some(AgentConfig {
        agent_type: Some("custom".into()),
        command: Some(script.to_string_lossy().to_string()),
        extra_args: None,
    });
}

fn make_task(id: &str, risk: TaskRisk, promotion: TaskPromotion) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status: TaskStatus::Ready,
        risk,
        scope: None,
        source: TaskSource {
            source_type: "human".into(),
            r#ref: "test".into(),
        },
        spec_id: None,
        branch: None,
        owner: None,
        acceptance_criteria: vec!["It works".into()],
        attempts: 0,
        last_failure_signature: None,
        promotion,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: Some(Vec::new()),
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn write_ledger(target: &Path, tasks: Vec<ProjectTask>) {
    let ledger = TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks,
    };
    fs::write(
        target.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

fn project_for(target: &Path) -> LinkedProject {
    LinkedProject {
        alias: "demo".into(),
        path: target.to_string_lossy().to_string(),
        default_branch: None,
        initialized: true,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    }
}

/// One scheduler iteration with the real provider dispatch (custom agent).
fn run_iteration(target: &Path, timeout_ms: u64) -> Result<SchedulerResult, String> {
    let project = project_for(target);
    let options = IterationOptions {
        model_override: None,
        timeout_ms: Some(timeout_ms),
        max_attempts_per_task: Some(3),
        no_progress_repeat_limit: Some(2),
    };
    let mut hooks = RunHooks {
        pi_runner: None,
        reviewer_runner: None,
        verifier_runner: None,
        validation_runner: None,
    };
    run_project_iteration(&project, &options, &mut hooks)
}

fn task_of<'a>(ledger: &'a TaskLedger, id: &str) -> &'a ProjectTask {
    ledger.tasks.iter().find(|t| t.id == id).unwrap()
}

fn read_state(home: &Path) -> DaemonState {
    serde_json::from_str(&fs::read_to_string(home.join("run").join("daemon-state.json")).unwrap())
        .unwrap()
}

fn init_git_repo(target: &Path) {
    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }
    git(target, &["init", "-q"]);
    git(target, &["config", "user.email", "test@example.com"]);
    git(target, &["config", "user.name", "Test"]);
    fs::write(target.join("init.txt"), "init\n").unwrap();
    git(target, &["add", "-A"]);
    git(target, &["commit", "-q", "-m", "init"]);
}

// ---------------------------------------------------------------------------
// Happy path: agent succeeds, validations pass.
// ---------------------------------------------------------------------------

#[test]
fn implement_run_with_passing_validations_marks_task_done_and_queues_auto_merge() {
    let _env = env_lock();
    let s = sandbox("happy");
    let agent = s.scripts.join("agent-ok.sh");
    write_script(&agent, "echo 'fake agent transcript'; exit 0");
    let lint = s.scripts.join("lint-pass.sh");
    write_script(&lint, "exit 0");
    let test_cmd = s.scripts.join("test-pass.sh");
    write_script(&test_cmd, "touch \"$(dirname \"$0\")/test-ran\"; exit 0");

    write_project_config(&s.target, |config| {
        use_custom_agent(config, &agent);
        config.validation.lint_command = Some(lint.to_string_lossy().to_string());
        config.validation.test_command = Some(test_cmd.to_string_lossy().to_string());
        // TS parity: the passing-validation flow disables the verifier so the
        // promotion decision reflects validation alone.
        config.verification = Some(ProjectVerification { enabled: false });
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "impl-pass",
            TaskRisk::LowRisk,
            TaskPromotion::AutoMerge,
        )],
    );

    let result = run_iteration(&s.target, 60_000).unwrap();

    assert_eq!(result.mode, "implement");
    assert_eq!(result.task_id.as_deref(), Some("impl-pass"));
    assert_eq!(result.task_status, Some(TaskStatus::Done));
    // Both configured validations ran and passed.
    assert_eq!(result.validation.len(), 2);
    assert_eq!(result.validation[0].name.as_str(), "lint");
    assert_eq!(result.validation[0].exit_code, 0);
    assert_eq!(result.validation[1].name.as_str(), "test");
    assert_eq!(result.validation[1].exit_code, 0);
    assert!(s.scripts.join("test-ran").exists());
    // No parseable usage from the fake agent -> the configured estimate.
    assert_eq!(result.cost_source, Some(CostSource::Estimated));

    // Low-risk auto-merge promotion survives the policy gate.
    assert_eq!(
        result.promotion_decision,
        PromotionDecision::AutoMergeEligible
    );
    assert_eq!(result.promotion_action, PromotionAction::QueueAutoMerge);

    // The promotion artifact exists and records the queue decision.
    let artifact_path = result.promotion_artifact_path.as_deref().unwrap();
    let artifact: PromotionArtifact =
        serde_json::from_str(&fs::read_to_string(artifact_path).unwrap()).unwrap();
    assert_eq!(artifact.decision, PromotionDecision::AutoMergeEligible);
    assert_eq!(artifact.action, PromotionAction::QueueAutoMerge);
    assert_eq!(artifact.task_id, "impl-pass");
    assert_eq!(artifact.status, PromotionArtifactStatus::Pending);

    // The run summary referenced by the result exists on disk.
    let summary_path = result.run_summary_path.as_deref().unwrap();
    let summary = fs::read_to_string(summary_path).unwrap();
    assert!(summary.contains("impl-pass"));

    // Persisted ledger: done + completed lastRun with auto-merge bookkeeping.
    let ledger = load_task_ledger(&s.target).unwrap();
    let task = task_of(&ledger, "impl-pass");
    assert_eq!(task.status, TaskStatus::Done);
    assert!(task.last_failure_signature.is_none());
    let last_run = task.last_run.as_ref().unwrap();
    assert_eq!(last_run.outcome, RunOutcome::Completed);
    assert_eq!(last_run.effective_promotion_mode, TaskPromotion::AutoMerge);
    assert_eq!(
        last_run.promotion_decision,
        PromotionDecision::AutoMergeEligible
    );
    assert_eq!(last_run.promotion_action, PromotionAction::QueueAutoMerge);
    assert_eq!(
        last_run.promotion_artifact_path.as_deref(),
        Some(artifact_path)
    );
}

#[test]
fn implement_run_downgrades_to_manual_review_for_medium_risk() {
    let _env = env_lock();
    let s = sandbox("medium");
    let agent = s.scripts.join("agent-ok.sh");
    write_script(&agent, "exit 0");
    let lint = s.scripts.join("lint-pass.sh");
    write_script(&lint, "exit 0");

    write_project_config(&s.target, |config| {
        use_custom_agent(config, &agent);
        config.validation.lint_command = Some(lint.to_string_lossy().to_string());
        config.verification = Some(ProjectVerification { enabled: false });
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "medium-ready",
            TaskRisk::MediumRisk,
            TaskPromotion::PullRequest,
        )],
    );

    let result = run_iteration(&s.target, 60_000).unwrap();

    assert_eq!(result.task_id.as_deref(), Some("medium-ready"));
    assert_eq!(result.mode, "implement");
    assert_eq!(result.task_status, Some(TaskStatus::Done));
    assert_eq!(result.promotion_decision, PromotionDecision::ManualReview);
    assert_eq!(result.promotion_action, PromotionAction::QueueReview);
    assert!(result.promotion_artifact_path.is_some());

    let ledger = load_task_ledger(&s.target).unwrap();
    let task = task_of(&ledger, "medium-ready");
    assert_eq!(task.status, TaskStatus::Done);
    let last_run = task.last_run.as_ref().unwrap();
    assert_eq!(
        last_run.effective_promotion_mode,
        TaskPromotion::PullRequest
    );
    assert_eq!(last_run.promotion_decision, PromotionDecision::ManualReview);
    assert_eq!(last_run.promotion_action, PromotionAction::QueueReview);
}

// ---------------------------------------------------------------------------
// Failing validation: lint exits 1, the run stops at the first failing step.
// ---------------------------------------------------------------------------

#[test]
fn implement_run_stops_at_first_failing_validation_and_fails_task() {
    let _env = env_lock();
    let s = sandbox("lintfail");
    let agent = s.scripts.join("agent-ok.sh");
    write_script(&agent, "exit 0");
    let lint = s.scripts.join("lint-fail.sh");
    write_script(&lint, "exit 1");
    let test_cmd = s.scripts.join("test-pass.sh");
    write_script(&test_cmd, "touch \"$(dirname \"$0\")/test-ran\"; exit 0");

    write_project_config(&s.target, |config| {
        use_custom_agent(config, &agent);
        config.validation.lint_command = Some(lint.to_string_lossy().to_string());
        config.validation.test_command = Some(test_cmd.to_string_lossy().to_string());
        config.verification = Some(ProjectVerification { enabled: false });
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "impl-fail",
            TaskRisk::LowRisk,
            TaskPromotion::AutoMerge,
        )],
    );

    let result = run_iteration(&s.target, 60_000).unwrap();

    // Only the failing lint step ran; the test command never executed.
    assert_eq!(result.validation.len(), 1);
    assert_eq!(result.validation[0].name.as_str(), "lint");
    assert_eq!(result.validation[0].exit_code, 1);
    assert!(!s.scripts.join("test-ran").exists());

    assert_eq!(result.task_status, Some(TaskStatus::Failed));
    assert_eq!(result.promotion_decision, PromotionDecision::Blocked);
    assert_eq!(result.promotion_action, PromotionAction::Block);
    let artifact: PromotionArtifact = serde_json::from_str(
        &fs::read_to_string(result.promotion_artifact_path.as_deref().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(artifact.decision, PromotionDecision::Blocked);
    assert_eq!(artifact.action, PromotionAction::Block);

    let ledger = load_task_ledger(&s.target).unwrap();
    let task = task_of(&ledger, "impl-fail");
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.attempts, 1);
    assert_eq!(
        task.last_failure_signature.as_deref(),
        Some("validation-lint")
    );
    let last_run = task.last_run.as_ref().unwrap();
    assert_eq!(last_run.outcome, RunOutcome::ValidationFailed);
    assert_eq!(last_run.promotion_decision, PromotionDecision::Blocked);
    assert_eq!(last_run.promotion_action, PromotionAction::Block);
}

#[test]
fn implement_run_downgrades_auto_merge_when_no_validations_configured() {
    let _env = env_lock();
    let s = sandbox("noval");
    let agent = s.scripts.join("agent-ok.sh");
    write_script(&agent, "exit 0");

    write_project_config(&s.target, |config| {
        use_custom_agent(config, &agent);
        config.verification = Some(ProjectVerification { enabled: false });
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "impl-no-validation",
            TaskRisk::LowRisk,
            TaskPromotion::AutoMerge,
        )],
    );

    let result = run_iteration(&s.target, 60_000).unwrap();

    // Auto-merge is unsafe without validation commands: manual review.
    assert_eq!(result.promotion_decision, PromotionDecision::ManualReview);
    assert_eq!(result.promotion_action, PromotionAction::QueueReview);

    let ledger = load_task_ledger(&s.target).unwrap();
    let task = task_of(&ledger, "impl-no-validation");
    assert_eq!(task.status, TaskStatus::Done);
    let last_run = task.last_run.as_ref().unwrap();
    assert_eq!(last_run.effective_promotion_mode, TaskPromotion::AutoMerge);
    assert_eq!(last_run.promotion_decision, PromotionDecision::ManualReview);
}

// ---------------------------------------------------------------------------
// Agent timeout: the run exceeds the configured budget and fails closed.
// ---------------------------------------------------------------------------

#[test]
fn agent_timeout_records_timeout_signature_and_error_outcome() {
    let _env = env_lock();
    let s = sandbox("timeout");
    let agent = s.scripts.join("agent-sleep.sh");
    write_script(&agent, "sleep 30; exit 0");

    write_project_config(&s.target, |config| {
        use_custom_agent(config, &agent);
        config.verification = Some(ProjectVerification { enabled: false });
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "impl-timeout",
            TaskRisk::LowRisk,
            TaskPromotion::AutoMerge,
        )],
    );

    let error = run_iteration(&s.target, 1_000).unwrap_err();
    assert!(error.contains("exceeded timeout"), "error was: {error}");

    let ledger = load_task_ledger(&s.target).unwrap();
    let task = task_of(&ledger, "impl-timeout");
    assert_eq!(task.attempts, 1);
    assert_eq!(task.last_failure_signature.as_deref(), Some("timeout"));
    let last_run = task.last_run.as_ref().unwrap();
    assert_eq!(last_run.outcome, RunOutcome::Error);
    // The in-progress claim is reverted: the task is ready for a retry,
    // not stuck in_progress (TS "recorded and reverted" semantics).
    assert_eq!(task.status, TaskStatus::Ready);
}

#[test]
fn worker_tick_charges_budget_estimate_when_agent_times_out() {
    let _env = env_lock();
    let s = sandbox("timeout-budget");
    let agent = s.scripts.join("agent-sleep.sh");
    write_script(&agent, "sleep 30; exit 0");

    write_global_config(&s.home, |config| {
        config.runtime.run_timeout_seconds = 1;
        config.budgets.estimated_cost_per_run_usd = Some(0.25);
    });
    write_project_config(&s.target, |config| {
        use_custom_agent(config, &agent);
        config.verification = Some(ProjectVerification { enabled: false });
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "impl-timeout",
            TaskRisk::LowRisk,
            TaskPromotion::AutoMerge,
        )],
    );

    let state = run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();

    // Timeout-killed runs produce no parseable usage: the estimate is charged.
    assert!((state.budget_spent_usd - 0.25).abs() < 1e-9);
    assert!((state.total_budget_spent_usd - 0.25).abs() < 1e-9);
    assert!(!state.budget_blocked);
    assert!(state.current_run.is_none());

    // The project row surfaces the timeout as the iteration result.
    let project = state.projects.iter().find(|p| p.alias == "demo").unwrap();
    assert!(project
        .last_result
        .as_deref()
        .unwrap_or_default()
        .contains("exceeded timeout"));

    let ledger = load_task_ledger(&s.target).unwrap();
    let task = task_of(&ledger, "impl-timeout");
    assert_eq!(task.attempts, 1);
    assert_eq!(task.last_failure_signature.as_deref(), Some("timeout"));
    assert_eq!(
        task.last_run.as_ref().map(|run| run.outcome),
        Some(RunOutcome::Error)
    );

    // The persisted state on disk matches the returned state.
    let persisted = read_state(&s.home);
    assert!((persisted.budget_spent_usd - 0.25).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// Dirty tree on a failing run: the ledger is reverted, the tree is left as
// the agent left it (the scheduler never resets the main working tree).
// ---------------------------------------------------------------------------

#[test]
fn failing_agent_run_on_dirty_tree_reverts_ledger_and_flags_dirty() {
    let _env = env_lock();
    let s = sandbox("dirty");
    let agent = s.scripts.join("agent-dirty-fail.sh");
    write_script(&agent, "echo agent-junk > agent-dirty.txt; exit 1");

    write_project_config(&s.target, |config| {
        use_custom_agent(config, &agent);
        config.verification = Some(ProjectVerification { enabled: false });
    });
    write_ledger(
        &s.target,
        vec![make_task(
            "impl-dirty",
            TaskRisk::LowRisk,
            TaskPromotion::AutoMerge,
        )],
    );
    // A committed baseline so the dirty-tree fingerprint comparison works.
    init_git_repo(&s.target);

    let result = run_iteration(&s.target, 60_000).unwrap();

    assert_eq!(result.exit_code, Some(1));
    assert_eq!(result.stopped_by, RunStoppedBy::None);
    // The agent mutated tracked/untracked state: the run is flagged dirty.
    assert!(result.dirty_tree_detected);
    // No promotion bookkeeping for a failed run.
    assert_eq!(result.promotion_decision, PromotionDecision::None);
    assert_eq!(result.promotion_action, PromotionAction::None);
    assert!(result.promotion_artifact_path.is_none());

    let ledger = load_task_ledger(&s.target).unwrap();
    let task = task_of(&ledger, "impl-dirty");
    // Recorded as a failed attempt and reverted, not stuck in-progress.
    assert_eq!(task.status, TaskStatus::Ready);
    assert_eq!(task.attempts, 1);
    assert_eq!(task.last_failure_signature.as_deref(), Some("pi-exit-1"));
    let last_run = task.last_run.as_ref().unwrap();
    assert_eq!(last_run.outcome, RunOutcome::PiFailed);
    assert_eq!(last_run.pi_exit_code, Some(1));

    // The revert is ledger-level: the main working tree keeps the agent's
    // dirty state (only worktree-mode runs are cleaned up).
    assert!(s.target.join("agent-dirty.txt").exists());
    assert_eq!(
        fs::read_to_string(s.target.join("agent-dirty.txt")).unwrap(),
        "agent-junk\n"
    );
}
