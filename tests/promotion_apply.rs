// Integration tests for promotion apply / auto-merge safety gates.
//
// These exercise the git-mutating paths of the promotion queue: every gate
// that stands between a pending artifact and the user's base branch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use openloop::core::types::*;
use openloop::core::{promotion_artifacts, promotion_queue};

fn sandbox(name: &str) -> Sandbox {
    let dir = std::env::temp_dir().join(format!("ol-promo-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    Sandbox(dir)
}

struct Sandbox(PathBuf);

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

use std::ops::{Deref, DerefMut};
impl Deref for Sandbox {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}
impl DerefMut for Sandbox {
    fn deref_mut(&mut self) -> &mut PathBuf {
        &mut self.0
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(
        status.success(),
        "git {:?} failed in {}",
        args,
        dir.display()
    );
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    // Ignore the control plane so staging helpers never dirty the tree
    // (the apply gates refuse dirty repos) and never move the base branch.
    fs::write(dir.join(".gitignore"), ".openloop/\n").unwrap();
    fs::write(dir.join("README.md"), "base\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "init"]);
}

fn make_task(id: &str, risk: TaskRisk) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status: TaskStatus::Done,
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
        attempts: 1,
        last_failure_signature: None,
        promotion: TaskPromotion::AutoMerge,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: Some(TaskRunSummary {
            completed_at: now.clone(),
            mode: "implement".into(),
            role: Some(WorkerRole::Implementer),
            pi_exit_code: Some(0),
            outcome: RunOutcome::Completed,
            base_branch: Some("main".into()),
            validation: vec![ValidationSummary {
                name: ValidationName::Lint,
                command: "true".into(),
                exit_code: 0,
            }],
            promotion_decision: PromotionDecision::AutoMergeEligible,
            effective_promotion_mode: TaskPromotion::AutoMerge,
            promotion_action: PromotionAction::QueueAutoMerge,
            promotion_artifact_path: None,
            promotion_artifact_state: PromotionArtifactStatus::Pending,
            promotion_result_artifact_path: None,
        }),
        created_at: now.clone(),
        updated_at: now,
    }
}

fn make_task_with_artifact(id: &str, risk: TaskRisk, artifact_path: &str) -> ProjectTask {
    let mut task = make_task(id, risk);
    task.last_run.as_mut().unwrap().promotion_artifact_path = Some(artifact_path.to_string());
    task
}

fn write_ledger(dir: &Path, tasks: Vec<ProjectTask>) {
    let ledger = TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks,
    };
    fs::create_dir_all(dir.join(".openloop")).unwrap();
    fs::write(
        dir.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
    // .openloop/ is gitignored: the tree stays clean without commits.
}

fn write_project_config(dir: &Path, lint: bool) {
    let mut config = openloop::core::project_config::default_project_config();
    if lint {
        config.validation.lint_command = Some("true".into());
    }
    fs::create_dir_all(dir.join(".openloop")).unwrap();
    fs::write(
        dir.join(".openloop").join("project.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
    // .openloop/ is gitignored: the tree stays clean without commits.
}

fn stage_pending_artifact(dir: &Path, task_id: &str, action: PromotionAction) -> String {
    let artifact = PromotionArtifact {
        version: 1,
        created_at: "2026-01-02T00:00:00.000Z".into(),
        project_alias: "p".into(),
        task_id: task_id.into(),
        base_branch: Some("main".into()),
        decision: if action == PromotionAction::QueueAutoMerge {
            PromotionDecision::AutoMergeEligible
        } else {
            PromotionDecision::ManualReview
        },
        action,
        effective_promotion_mode: if action == PromotionAction::QueueAutoMerge {
            TaskPromotion::AutoMerge
        } else {
            TaskPromotion::PullRequest
        },
        validation: vec![ValidationSummary {
            name: ValidationName::Lint,
            command: "true".into(),
            exit_code: 0,
        }],
        pi_exit_code: Some(0),
        outcome: RunOutcome::Completed,
        approval_packet_path: None,
        status: PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    };
    let path = promotion_artifacts::write_promotion_artifact(dir, &artifact).unwrap();
    // .openloop/ is gitignored: the tree stays clean without commits.
    path
}

fn head(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn current_branch(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn auto_merge_fast_forwards_base_branch() {
    let dir = sandbox("ff-merge");
    init_repo(&dir);
    write_project_config(&dir, true);

    // Task branch with one commit on top of main.
    let task_id = "fix-ff";
    git(&dir, &["checkout", "-q", "-b", "openloop/fix-ff"]);
    fs::write(dir.join("change.txt"), "change\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "task work"]);
    git(&dir, &["checkout", "-q", "main"]);
    let main_head = head(&dir);

    let artifact_path = stage_pending_artifact(&dir, task_id, PromotionAction::QueueAutoMerge);
    let mut task = make_task_with_artifact(task_id, TaskRisk::LowRisk, &artifact_path);
    task.branch = Some("openloop/fix-ff".into());
    write_ledger(&dir, vec![task]);

    let item = promotion_queue::apply_promotion_artifact(&dir, task_id, None).unwrap();
    assert_eq!(item.artifact.status, PromotionArtifactStatus::Applied);

    // Base branch fast-forwarded: HEAD moved past the old main head.
    assert_ne!(head(&dir), main_head);
    assert_eq!(current_branch(&dir), "main");
    assert!(dir.join("change.txt").exists(), "merged content present");

    // Ledger reflects the promotion.
    let ledger = openloop::core::task_ledger::load_task_ledger(&dir).unwrap();
    let task = ledger.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Promoted);
    assert!(task.promoted_at.is_some());

    // Result artifact written.
    let results =
        promotion_artifacts::list_promotion_result_artifacts(&dir, Some(task_id)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.result, "applied");
}

#[test]
fn auto_merge_refuses_dirty_tree() {
    let dir = sandbox("dirty");
    init_repo(&dir);
    write_project_config(&dir, true);
    write_ledger(&dir, vec![make_task("t-dirty", TaskRisk::LowRisk)]);
    stage_pending_artifact(&dir, "t-dirty", PromotionAction::QueueAutoMerge);
    fs::write(dir.join("README.md"), "dirty\n").unwrap();

    let err = promotion_queue::apply_promotion_artifact(&dir, "t-dirty", None).unwrap_err();
    assert!(err.contains("dirty"), "unexpected error: {err}");
}

#[test]
fn auto_merge_refuses_base_drift() {
    let dir = sandbox("drift");
    init_repo(&dir);
    write_project_config(&dir, true);

    let task_id = "t-drift";
    git(
        &dir,
        &["checkout", "-q", "-b", &format!("openloop/{task_id}")],
    );
    fs::write(dir.join("task.txt"), "task\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "task work"]);
    git(&dir, &["checkout", "-q", "main"]);
    // Base advances after the task branch split.
    fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "base moves"]);

    let artifact_path = stage_pending_artifact(&dir, task_id, PromotionAction::QueueAutoMerge);
    let mut task = make_task_with_artifact(task_id, TaskRisk::LowRisk, &artifact_path);
    task.branch = Some(format!("openloop/{task_id}"));
    write_ledger(&dir, vec![task]);

    let err = promotion_queue::apply_promotion_artifact(&dir, task_id, None).unwrap_err();
    assert!(err.contains("drifted"), "unexpected error: {err}");
}

#[test]
fn auto_merge_refuses_high_risk() {
    let dir = sandbox("risk");
    init_repo(&dir);
    write_project_config(&dir, true);
    write_ledger(&dir, vec![make_task("t-risk", TaskRisk::HighRisk)]);
    stage_pending_artifact(&dir, "t-risk", PromotionAction::QueueAutoMerge);

    let err = promotion_queue::apply_promotion_artifact(&dir, "t-risk", None).unwrap_err();
    assert!(err.contains("low-risk"), "unexpected error: {err}");
}

#[test]
fn auto_merge_refuses_without_configured_validations() {
    let dir = sandbox("noval");
    init_repo(&dir);
    write_project_config(&dir, false); // no lint configured
    write_ledger(&dir, vec![make_task("t-noval", TaskRisk::LowRisk)]);
    stage_pending_artifact(&dir, "t-noval", PromotionAction::QueueAutoMerge);

    let err = promotion_queue::apply_promotion_artifact(&dir, "t-noval", None).unwrap_err();
    assert!(err.contains("validation"), "unexpected error: {err}");
}

#[test]
fn review_action_prepares_branch_without_merging() {
    let dir = sandbox("review");
    init_repo(&dir);
    write_project_config(&dir, true);
    let main_head = head(&dir);

    let task_id = "t-review";
    let artifact_path = stage_pending_artifact(&dir, task_id, PromotionAction::QueueReview);
    let mut task = make_task_with_artifact(task_id, TaskRisk::LowRisk, &artifact_path);
    task.promotion = TaskPromotion::PullRequest;
    task.last_run.as_mut().unwrap().promotion_action = PromotionAction::QueueReview;
    write_ledger(&dir, vec![task]);

    let item = promotion_queue::apply_promotion_artifact(&dir, task_id, None).unwrap();
    assert_eq!(item.artifact.status, PromotionArtifactStatus::Applied);

    // No merge happened: main unchanged, task branch created and checked out.
    assert_eq!(head(&dir), main_head, "base must not move for review path");
    assert_eq!(current_branch(&dir), "openloop/t-review");

    // Review flow leaves the task done (not promoted), promotedAt null.
    let ledger = openloop::core::task_ledger::load_task_ledger(&dir).unwrap();
    let task = ledger.tasks.iter().find(|t| t.id == task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Done);
    assert!(task.promoted_at.is_none());
    assert_eq!(task.branch.as_deref(), Some("openloop/t-review"));
}

#[test]
fn reapply_after_resolution_fails_without_pending() {
    let dir = sandbox("reapply");
    init_repo(&dir);
    write_project_config(&dir, true);
    write_ledger(&dir, vec![make_task("t-once", TaskRisk::LowRisk)]);
    stage_pending_artifact(&dir, "t-once", PromotionAction::QueueReview);
    promotion_queue::apply_promotion_artifact(&dir, "t-once", None).unwrap();

    let err = promotion_queue::apply_promotion_artifact(&dir, "t-once", None).unwrap_err();
    assert!(
        err.contains("No pending promotion artifact"),
        "unexpected: {err}"
    );
}

#[test]
fn reject_marks_rejected_and_writes_result() {
    let dir = sandbox("reject");
    init_repo(&dir);
    write_ledger(&dir, vec![make_task("t-rej", TaskRisk::LowRisk)]);
    stage_pending_artifact(&dir, "t-rej", PromotionAction::QueueReview);

    let item = promotion_queue::update_promotion_artifact(
        &dir,
        "t-rej",
        PromotionArtifactStatus::Rejected,
        Some("not now"),
    )
    .unwrap();
    assert_eq!(item.artifact.status, PromotionArtifactStatus::Rejected);
    assert_eq!(item.artifact.note.as_deref(), Some("not now"));

    let results =
        promotion_artifacts::list_promotion_result_artifacts(&dir, Some("t-rej")).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.result, "rejected");
}

#[test]
fn dry_run_reports_gates_without_mutating() {
    let dir = sandbox("dryrun");
    init_repo(&dir);
    write_project_config(&dir, true);
    let main_head = head(&dir);

    let task_id = "t-dry";
    write_ledger(&dir, vec![make_task(task_id, TaskRisk::LowRisk)]);
    stage_pending_artifact(&dir, task_id, PromotionAction::QueueReview);

    let preview = promotion_queue::dry_run_promotion_apply(&dir, task_id).unwrap();
    assert!(preview.would_apply);
    assert_eq!(preview.action, "queue-review");
    assert!(preview.reason.contains("openloop/t-dry"));

    // Nothing moved: git untouched, artifact still pending.
    assert_eq!(head(&dir), main_head);
    let items = promotion_queue::list_promotion_artifacts(&dir).unwrap();
    assert_eq!(items[0].artifact.status, PromotionArtifactStatus::Pending);
}

#[test]
fn history_merges_requests_and_results_chronologically() {
    let dir = sandbox("history");
    init_repo(&dir);
    write_ledger(&dir, vec![make_task("t-hist", TaskRisk::LowRisk)]);
    stage_pending_artifact(&dir, "t-hist", PromotionAction::QueueReview);
    promotion_queue::update_promotion_artifact(
        &dir,
        "t-hist",
        PromotionArtifactStatus::Rejected,
        None,
    )
    .unwrap();

    let history = promotion_queue::get_promotion_history(&dir, "t-hist").unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].kind, "request");
    assert_eq!(history[1].kind, "result");
    assert!(history[0].created_at <= history[1].created_at);
}

#[test]
fn auto_merge_refuses_artifact_without_recorded_base() {
    let dir = sandbox("no-base");
    init_repo(&dir);
    write_project_config(&dir, true);
    let task_id = "t-nobase";

    // Task branch exists, so apply would merge if it got past the guard.
    git(&dir, &["checkout", "-q", "-b", "openloop/t-nobase"]);
    fs::write(dir.join("change.txt"), "change\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "task work"]);
    git(&dir, &["checkout", "-q", "main"]);
    let main_head = head(&dir);

    // Pending auto-merge artifact WITHOUT a recorded base branch.
    let artifact = PromotionArtifact {
        version: 1,
        created_at: "2026-01-02T00:00:00.000Z".into(),
        project_alias: "p".into(),
        task_id: task_id.into(),
        base_branch: None,
        decision: PromotionDecision::AutoMergeEligible,
        action: PromotionAction::QueueAutoMerge,
        effective_promotion_mode: TaskPromotion::AutoMerge,
        validation: vec![ValidationSummary {
            name: ValidationName::Lint,
            command: "true".into(),
            exit_code: 0,
        }],
        pi_exit_code: Some(0),
        outcome: RunOutcome::Completed,
        approval_packet_path: None,
        status: PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    };
    let artifact_path = promotion_artifacts::write_promotion_artifact(&dir, &artifact).unwrap();
    let mut task = make_task_with_artifact(task_id, TaskRisk::LowRisk, &artifact_path);
    task.branch = Some("openloop/t-nobase".into());
    write_ledger(&dir, vec![task]);

    // Dry-run reports the refusal up front.
    let dry = promotion_queue::dry_run_promotion_apply(&dir, task_id).unwrap();
    assert!(!dry.would_apply);
    assert!(
        dry.reason.contains("no recorded base branch"),
        "{}",
        dry.reason
    );

    // Apply refuses instead of merging into whatever is checked out.
    let err = promotion_queue::apply_promotion_artifact(&dir, task_id, None).unwrap_err();
    assert!(err.contains("no recorded base branch"), "error: {err}");
    assert_eq!(head(&dir), main_head, "main must be untouched");
    assert!(!dir.join("change.txt").exists(), "no merge happened");
}
