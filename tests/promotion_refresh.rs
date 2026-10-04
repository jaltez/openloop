// Integration tests for `promotion refresh`: live PR state pulled via
// `gh pr view` and fed back into the ledger (merged PRs get promoted, failing
// CI checks append a note and leave the task alone, a missing `gh` changes
// nothing).

use std::path::{Path, PathBuf};

use clap::Parser;
use openloop::cli::{self, Cli};
use openloop::core::promotion_artifacts::{
    list_promotion_result_artifacts, write_promotion_result_artifact,
};
use openloop::core::types::*;
use parking_lot::{Mutex, MutexGuard};
use std::fs;
use std::os::unix::fs::PermissionsExt;

// refresh resolves the app home from OPENLOOP_HOME and spawns `gh` from PATH;
// both env mutations must be serialized across tests in this process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Sandbox {
    base: Option<PathBuf>,
    project_root: PathBuf,
    bin: PathBuf,
    saved_home: Option<String>,
    saved_path: Option<String>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // base is the parent of the sandbox tree — one removal suffices.
        if let Some(base) = self.base.as_ref() {
            let _ = fs::remove_dir_all(base);
        }
        match self.saved_home.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        match self.saved_path.take() {
            Some(prev) => std::env::set_var("PATH", prev),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock()
}

/// App home with the "demo" project registered, a done `pr-task` in the
/// ledger, and PATH pointing at an empty bin dir (no `gh` available).
fn seed(name: &str) -> Sandbox {
    let base = std::env::temp_dir().join(format!("ol-promo-refresh-{name}-{}", std::process::id()));
    let home = base.join("home");
    let project_root = base.join("project");
    let bin = base.join("bin");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&project_root).unwrap();
    fs::create_dir_all(&bin).unwrap();

    fs::write(
        home.join("projects.json"),
        serde_json::to_string(&ProjectsRegistry {
            version: 1,
            projects: vec![LinkedProject {
                alias: "demo".into(),
                path: project_root.to_string_lossy().to_string(),
                default_branch: None,
                initialized: true,
                created_at: "2026-01-01T00:00:00.000Z".into(),
                updated_at: "2026-01-01T00:00:00.000Z".into(),
            }],
        })
        .unwrap(),
    )
    .unwrap();

    let ledger = TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks: vec![make_done_task("pr-task")],
    };
    fs::create_dir_all(project_root.join(".openloop")).unwrap();
    fs::write(
        project_root.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();

    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);
    let saved_path = std::env::var("PATH").ok();
    std::env::set_var("PATH", &bin);

    Sandbox {
        base: Some(base),
        project_root,
        bin,
        saved_home,
        saved_path,
    }
}

/// Executable `gh` stub that prints the given `gh pr view --json` payload;
/// prepends the stub dir to PATH so it wins over the real binary.
fn install_gh_stub(bin: &Path, view_json: &str) {
    let script = format!("#!/bin/sh\necho '{view_json}'\n");
    fs::write(bin.join("gh"), script).unwrap();
    fs::set_permissions(bin.join("gh"), fs::Permissions::from_mode(0o755)).unwrap();
    let current = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{current}", bin.display()));
}

fn seed_pr_result(project_root: &Path, pr_url: &str) {
    let artifact = PromotionResultArtifact {
        version: 1,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        project_alias: "demo".into(),
        task_id: "pr-task".into(),
        source_promotion_artifact_path: "/tmp/source.json".into(),
        source_promotion_action: PromotionAction::QueueReview,
        source_promotion_decision: PromotionDecision::ManualReview,
        result: "rejected".into(),
        branch: Some("openloop/pr-task".into()),
        base_branch: Some("main".into()),
        note: None,
        pr_url: Some(pr_url.into()),
        state: None,
        checks: None,
    };
    write_promotion_result_artifact(project_root, &artifact).unwrap();
}

fn run_refresh() -> Result<(), String> {
    let cli = Cli::parse_from([
        "openloop",
        "promotion",
        "refresh",
        "-p",
        "demo",
        "--task",
        "pr-task",
    ]);
    cli::dispatch(cli.command)
}

fn make_done_task(id: &str) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: id.into(),
        title: "PR task".into(),
        kind: TaskKind::Feature,
        status: TaskStatus::Done,
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
        attempts: 0,
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

fn read_task(project_root: &Path) -> ProjectTask {
    let raw = fs::read_to_string(project_root.join(".openloop").join("tasks.json")).unwrap();
    let ledger: TaskLedger = serde_json::from_str(&raw).unwrap();
    ledger
        .tasks
        .into_iter()
        .find(|task| task.id == "pr-task")
        .unwrap()
}

#[test]
fn promotion_refresh_marks_merged_prs_promoted() {
    let _env = env_lock();
    let sandbox = seed("merged");
    seed_pr_result(&sandbox.project_root, "https://github.com/org/repo/pull/7");
    install_gh_stub(&sandbox.bin, r#"{"state":"MERGED","statusCheckRollup":[]}"#);

    run_refresh().expect("refresh should succeed");

    let task = read_task(&sandbox.project_root);
    assert_eq!(task.status, TaskStatus::Promoted);
    let promoted_at = task.promoted_at.expect("promotedAt should be set");
    assert!(!promoted_at.trim().is_empty());

    let results = list_promotion_result_artifacts(&sandbox.project_root, Some("pr-task")).unwrap();
    let refreshed = results
        .iter()
        .find(|(_, artifact)| artifact.result == "refreshed")
        .expect("refreshed result artifact should be written");
    assert_eq!(refreshed.1.state.as_deref(), Some("MERGED"));
    assert!(refreshed.1.checks.as_deref().unwrap_or(&[]).is_empty());
}

#[test]
fn promotion_refresh_records_failed_ci_checks() {
    let _env = env_lock();
    let sandbox = seed("failed-ci");
    seed_pr_result(&sandbox.project_root, "https://github.com/org/repo/pull/8");
    install_gh_stub(
        &sandbox.bin,
        r#"{"state":"OPEN","statusCheckRollup":[{"name":"ci/test","status":"COMPLETED","conclusion":"FAILURE"}]}"#,
    );

    run_refresh().expect("refresh should succeed");

    let task = read_task(&sandbox.project_root);
    assert_eq!(task.status, TaskStatus::Done);
    let notes = task.notes.expect("a note should be appended");
    assert!(notes
        .iter()
        .any(|note| note.contains("PR checks failed: ci/test")));
}

#[test]
fn promotion_refresh_detects_failed_ci_reported_via_legacy_status_contexts() {
    let _env = env_lock();
    let sandbox = seed("legacy-status");
    seed_pr_result(&sandbox.project_root, "https://github.com/org/repo/pull/10");
    install_gh_stub(
        &sandbox.bin,
        r#"{"state":"OPEN","statusCheckRollup":[{"context":"jenkins/build","state":"FAILURE"}]}"#,
    );

    run_refresh().expect("refresh should succeed");

    let task = read_task(&sandbox.project_root);
    assert_eq!(task.status, TaskStatus::Done);
    let notes = task.notes.expect("a note should be appended");
    assert!(notes
        .iter()
        .any(|note| note.contains("PR checks failed: jenkins/build")));
}

#[test]
fn promotion_refresh_fails_cleanly_when_gh_is_unavailable() {
    let _env = env_lock();
    let sandbox = seed("no-gh");
    seed_pr_result(&sandbox.project_root, "https://github.com/org/repo/pull/9");
    // PATH points only at the empty stub dir: `gh` cannot be spawned.

    let error = run_refresh().expect_err("missing gh should surface an error");
    assert!(!error.trim().is_empty());

    // No state change when gh is missing.
    let task = read_task(&sandbox.project_root);
    assert_eq!(task.status, TaskStatus::Done);
    assert!(task.promoted_at.is_none());
    assert!(task.notes.is_none());
    let results = list_promotion_result_artifacts(&sandbox.project_root, Some("pr-task")).unwrap();
    assert!(results
        .iter()
        .all(|(_, artifact)| artifact.result != "refreshed"));
}
