// Integration tests for the independent verifier stage inside
// run_project_iteration: verdict paths (fail / needs-human / missing /
// malformed), the disabled shortcut, and the stale-verdict guard.
//
// The agent is a fake custom-agent shell script (project config
// `agent: { type: "custom", command: ... }`) so the real subprocess dispatch
// runs: implement prompts succeed silently, verification prompts copy a
use parking_lot::{Mutex, MutexGuard};
use std::fs;
use std::path::{Path, PathBuf};

use openloop::core::scheduler::{run_project_iteration, IterationOptions, RunHooks};
use openloop::core::types::*;

// run_project_iteration resolves the app home from OPENLOOP_HOME; env
// mutation must be serialized across tests in this process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock()
}

const TASK_ID: &str = "verify-task";

const FAKE_AGENT: &str = r#"#!/bin/sh
# Fake agent: the scheduler passes the prompt as $1.
# - implement prompt: succeed without touching anything
# - verification prompt: copy the staged payload (.openloop/verifier-payload.json)
#   to the absolute verdicts path embedded in the prompt; a
#   .openloop/verifier-crash marker simulates a crashed verifier (non-zero
#   exit, nothing written)
prompt=$1
case $prompt in
  *"verification role"*) role=verify ;;
  *) role=implement ;;
esac
printf '%s\n' "$role" >> agent-calls.log
if [ "$role" = implement ]; then
  exit 0
fi
if [ -f .openloop/verifier-crash ]; then
  exit 3
fi
verdicts=$(printf '%s\n' "$prompt" | sed -n 's/^Create the directory containing `\(.*\)` if needed.*/\1/p')
if [ -n "$verdicts" ] && [ -f .openloop/verifier-payload.json ]; then
  mkdir -p "$(dirname "$verdicts")"
  cp .openloop/verifier-payload.json "$verdicts"
fi
exit 0
"#;

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

fn sandbox(name: &str, verification_enabled: bool) -> Sandbox {
    let home = std::env::temp_dir().join(format!("ol-verifier-home-{name}-{}", std::process::id()));
    let target =
        std::env::temp_dir().join(format!("ol-verifier-target-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(target.join(".openloop")).unwrap();

    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);

    fs::write(
        home.join("config.json"),
        serde_json::to_string_pretty(&openloop::core::global_config::default_global_config())
            .unwrap(),
    )
    .unwrap();

    // The fake agent script lives in the app home (never inside the project).
    let script = home.join("fake-agent.sh");
    fs::write(&script, FAKE_AGENT).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    write_project_config(&target, &script, verification_enabled);
    fs::write(target.join(".openloop").join("policy.yaml"), "version: 1\n").unwrap();
    write_ledger(&target);

    Sandbox {
        home,
        target,
        saved_home,
    }
}

fn write_project_config(target: &Path, script: &Path, verification_enabled: bool) {
    let config = serde_json::json!({
        "version": 1,
        "project": { "alias": "demo", "repoRoot": null, "initializedAt": null },
        "pi": { "model": null, "promptFiles": [] },
        "agent": {
            "type": "custom",
            "command": script.to_string_lossy(),
        },
        "runtime": { "useWorktree": false, "branchPrefix": "openloop/" },
        "validation": { "lintCommand": null, "testCommand": "true", "typecheckCommand": null },
        "risk": {
            "defaultUnknownAreaClassification": "low-risk",
            "requirePolicyForAutoMerge": true,
        },
        "review": { "enabled": false },
        "verification": { "enabled": verification_enabled },
    });
    fs::write(
        target.join(".openloop").join("project.json"),
        format!("{}\n", serde_json::to_string_pretty(&config).unwrap()),
    )
    .unwrap();
}

fn write_ledger(target: &Path) {
    let now = "2026-01-01T00:00:00.000Z";
    let ledger = TaskLedger {
        version: 1,
        updated_at: now.to_string(),
        tasks: vec![ProjectTask {
            id: TASK_ID.to_string(),
            title: "Verified task".to_string(),
            kind: TaskKind::Feature,
            status: TaskStatus::Ready,
            risk: TaskRisk::LowRisk,
            scope: None,
            source: TaskSource {
                source_type: "human".to_string(),
                r#ref: "test".to_string(),
            },
            spec_id: None,
            branch: None,
            owner: None,
            acceptance_criteria: vec!["Criterion one".to_string(), "Criterion two".to_string()],
            attempts: 0,
            last_failure_signature: None,
            promotion: TaskPromotion::AutoMerge,
            promoted_at: None,
            depends_on: None,
            estimated_cost_usd: None,
            notes: Some(Vec::new()),
            last_run: None,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        }],
    };
    fs::write(
        target.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

fn stage_verdict_payload(target: &Path, content: &str) {
    fs::write(
        target.join(".openloop").join("verifier-payload.json"),
        content,
    )
    .unwrap();
}

fn verdicts_path(target: &Path) -> PathBuf {
    target
        .join(".openloop")
        .join("verifications")
        .join(format!("{TASK_ID}.json"))
}

fn agent_calls(target: &Path) -> Vec<String> {
    fs::read_to_string(target.join("agent-calls.log"))
        .map(|log| log.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

fn persisted_task(target: &Path) -> ProjectTask {
    let ledger: TaskLedger = serde_json::from_str(
        &fs::read_to_string(target.join(".openloop").join("tasks.json")).unwrap(),
    )
    .unwrap();
    ledger
        .tasks
        .into_iter()
        .find(|task| task.id == TASK_ID)
        .expect("seeded task must stay in the ledger")
}

fn notes_contain(task: &ProjectTask, needle: &str) -> bool {
    task.notes
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|note| note.contains(needle))
}

fn run_iteration(target: &Path, no_progress_repeat_limit: Option<u32>) -> SchedulerResult {
    let project = LinkedProject {
        alias: "demo".to_string(),
        path: target.to_string_lossy().to_string(),
        default_branch: None,
        initialized: true,
        created_at: "2026-01-01T00:00:00.000Z".to_string(),
        updated_at: "2026-01-01T00:00:00.000Z".to_string(),
    };
    let options = IterationOptions {
        model_override: None,
        timeout_ms: Some(120_000),
        max_attempts_per_task: None,
        no_progress_repeat_limit,
    };
    let mut hooks = RunHooks {
        pi_runner: None,
        reviewer_runner: None,
        verifier_runner: None,
        validation_runner: None,
    };
    run_project_iteration(&project, &options, &mut hooks).expect("iteration should succeed")
}

#[test]
fn fail_verdict_fails_the_task_with_verifier_rejected_signature() {
    let _env = env_lock();
    let sandbox = sandbox("fail", true);
    let target = &sandbox.target;
    stage_verdict_payload(
        target,
        r#"[
            { "index": 1, "criterion": "Criterion one", "verdict": "pass", "evidence": "ok" },
            { "index": 2, "criterion": "Criterion two", "verdict": "fail", "evidence": "missing" }
        ]"#,
    );

    let result = run_iteration(target, None);

    assert_eq!(result.task_status, Some(TaskStatus::Failed));
    // The verdicts file landed at the absolute control-plane path from the
    // verification prompt (the fake agent parses it out of the prompt).
    assert!(verdicts_path(target).exists());

    let task = persisted_task(target);
    assert_eq!(task.attempts, 1);
    assert_eq!(
        task.last_failure_signature.as_deref(),
        Some("verifier-rejected")
    );
    assert!(notes_contain(&task, "Verifier rejected criterion 2."));
    assert!(!notes_contain(&task, "Verifier rejected criterion 1."));
}

#[test]
fn needs_human_verdict_completes_the_task_but_downgrades_to_manual_review() {
    let _env = env_lock();
    let sandbox = sandbox("needs-human", true);
    let target = &sandbox.target;
    stage_verdict_payload(
        target,
        r#"[
            { "index": 1, "criterion": "Criterion one", "verdict": "pass", "evidence": "ok" },
            { "index": 2, "criterion": "Criterion two", "verdict": "needs-human", "evidence": "can't tell" }
        ]"#,
    );

    let result = run_iteration(target, None);

    assert_eq!(result.task_status, Some(TaskStatus::Done));
    // Without the verdict this low-risk auto-merge task with a passing test
    // command would be auto-merge-eligible; needs-human downgrades it.
    assert_eq!(result.promotion_decision, PromotionDecision::ManualReview);
    assert_eq!(result.promotion_action, PromotionAction::QueueReview);

    let task = persisted_task(target);
    assert_eq!(task.status, TaskStatus::Done);
    assert!(notes_contain(
        &task,
        "Verifier verdict requires human review."
    ));
}

#[test]
fn missing_verdict_file_fails_open_to_manual_review() {
    let _env = env_lock();
    let sandbox = sandbox("missing", true);
    let target = &sandbox.target;
    // No payload staged: the verifier runs and exits 0 without writing any
    // verdicts file.
    assert!(!verdicts_path(target).exists());

    let result = run_iteration(target, None);

    assert_eq!(result.task_status, Some(TaskStatus::Done));
    assert_eq!(result.promotion_decision, PromotionDecision::ManualReview);
    assert_eq!(result.promotion_action, PromotionAction::QueueReview);
    assert!(!verdicts_path(target).exists());

    let task = persisted_task(target);
    assert_eq!(task.status, TaskStatus::Done);
    assert!(notes_contain(
        &task,
        "Verifier verdict requires human review."
    ));
}

#[test]
fn malformed_verdict_file_fails_open_to_manual_review() {
    let _env = env_lock();
    let sandbox = sandbox("malformed", true);
    let target = &sandbox.target;
    stage_verdict_payload(target, "{ this is not json");

    let result = run_iteration(target, None);

    assert_eq!(result.task_status, Some(TaskStatus::Done));
    assert_eq!(result.promotion_decision, PromotionDecision::ManualReview);
    assert_eq!(result.promotion_action, PromotionAction::QueueReview);
    // The malformed content really was delivered to the verdicts path (so
    // this exercises parsing, not the missing-file branch).
    assert_eq!(
        fs::read_to_string(verdicts_path(target)).unwrap(),
        "{ this is not json"
    );

    let task = persisted_task(target);
    assert_eq!(task.status, TaskStatus::Done);
    assert!(notes_contain(
        &task,
        "Verifier verdict requires human review."
    ));
}

#[test]
fn verification_disabled_skips_the_stage_entirely() {
    let _env = env_lock();
    let sandbox = sandbox("disabled", false);
    let target = &sandbox.target;
    // A payload is staged to prove the verifier branch never consumes it.
    stage_verdict_payload(
        target,
        r#"[{ "index": 1, "criterion": "Criterion one", "verdict": "fail", "evidence": "broken" }]"#,
    );

    let result = run_iteration(target, None);

    // Only the implementer was invoked — no verifier run at all.
    assert_eq!(agent_calls(target), vec!["implement".to_string()]);
    assert_eq!(result.task_status, Some(TaskStatus::Done));
    assert_eq!(
        result.promotion_decision,
        PromotionDecision::AutoMergeEligible
    );
    assert_eq!(result.promotion_action, PromotionAction::QueueAutoMerge);
    // No verification artifact was produced.
    assert!(!verdicts_path(target).exists());
    assert!(!target.join(".openloop").join("verifications").exists());

    let task = persisted_task(target);
    assert_eq!(task.status, TaskStatus::Done);
    assert!(!notes_contain(
        &task,
        "Verifier verdict requires human review."
    ));
}

#[test]
fn stale_verdicts_from_a_previous_attempt_are_not_replayed_against_a_retry() {
    let _env = env_lock();
    let sandbox = sandbox("stale", true);
    let target = &sandbox.target;

    // Attempt 1: the verifier writes a fail verdict and the task fails.
    stage_verdict_payload(
        target,
        r#"[{ "index": 1, "criterion": "Criterion one", "verdict": "fail", "evidence": "broken" }]"#,
    );
    let first = run_iteration(target, Some(10));
    assert_eq!(first.task_status, Some(TaskStatus::Failed));
    assert!(verdicts_path(target).exists());

    // Reset the failed task to ready (a retry cycle) and fill out the
    // validation config so no continuous-improvement task preempts it.
    let config_path = target.join(".openloop").join("project.json");
    let mut config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["validation"] = serde_json::json!({
        "lintCommand": "true",
        "testCommand": "true",
        "typecheckCommand": "true",
    });
    fs::write(
        &config_path,
        format!("{}\n", serde_json::to_string_pretty(&config).unwrap()),
    )
    .unwrap();
    let ledger_path = target.join(".openloop").join("tasks.json");
    let mut ledger: TaskLedger =
        serde_json::from_str(&fs::read_to_string(&ledger_path).unwrap()).unwrap();
    ledger.tasks[0].status = TaskStatus::Ready;
    fs::write(&ledger_path, serde_json::to_string_pretty(&ledger).unwrap()).unwrap();

    // Attempt 2: the verifier crashes (non-zero exit, writes nothing). The
    // stale fail verdict from attempt 1 must not be replayed: the run
    // degrades to human review instead of failing again.
    fs::remove_file(target.join(".openloop").join("verifier-payload.json")).unwrap();
    fs::write(target.join(".openloop").join("verifier-crash"), "").unwrap();
    let second = run_iteration(target, Some(10));

    assert_eq!(second.task_status, Some(TaskStatus::Done));
    assert_eq!(second.promotion_action, PromotionAction::QueueReview);
    assert!(!verdicts_path(target).exists());

    let task = persisted_task(target);
    assert_eq!(task.status, TaskStatus::Done);
    // The task was not failed again by the stale verdict: attempts still
    // reflect only attempt 1 and no second rejection note was added.
    assert_eq!(task.attempts, 1);
    assert_eq!(
        task.notes
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|note| note.contains("Verifier rejected criterion"))
            .count(),
        1
    );
    assert!(notes_contain(
        &task,
        "Verifier verdict requires human review."
    ));
}
