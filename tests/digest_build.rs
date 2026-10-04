// Integration tests for the fleet digest builder (`build_digest`): per-project
// status counting inside the since window, the measured/estimated spend split
// parsed from `.openloop/runs/*.md`, and the pending-promotion review queue.
//
// Ported from tests/integration/digest.test.ts (the buildDigest aggregation
// case); the computeConfidence case lives in tests/digest.rs.

use std::fs;
use std::path::{Path, PathBuf};

use openloop::core::approval_packets::{write_approval_packet, ApprovalPacketData};
use openloop::core::digest::{build_digest, DigestConfidence};
use openloop::core::now_iso;
use openloop::core::project_registry::add_project;
use openloop::core::promotion_artifacts::write_promotion_artifact;
use openloop::core::types::*;

const STALE: &str = "2020-01-01T00:00:00.000Z";

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ol-digest-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_task(id: &str, title: &str, status: TaskStatus, risk: TaskRisk) -> ProjectTask {
    let now = now_iso();
    ProjectTask {
        id: id.into(),
        title: title.into(),
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
        attempts: 1,
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

fn last_run(completed_at: &str, outcome: RunOutcome) -> TaskRunSummary {
    TaskRunSummary {
        completed_at: completed_at.into(),
        mode: "implement".into(),
        role: Some(WorkerRole::Implementer),
        pi_exit_code: Some(0),
        outcome,
        base_branch: Some("main".into()),
        validation: vec![],
        promotion_decision: PromotionDecision::ManualReview,
        effective_promotion_mode: TaskPromotion::PullRequest,
        promotion_action: PromotionAction::QueueReview,
        promotion_artifact_path: None,
        promotion_artifact_state: PromotionArtifactStatus::Pending,
        promotion_result_artifact_path: None,
    }
}

fn write_ledger(dir: &Path, tasks: Vec<ProjectTask>) {
    let ledger = TaskLedger {
        version: 1,
        updated_at: now_iso(),
        tasks,
    };
    fs::create_dir_all(dir.join(".openloop")).unwrap();
    fs::write(
        dir.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

fn write_test_configured_project(dir: &Path) {
    let mut config = openloop::core::project_config::default_project_config();
    config.validation.test_command = Some("bun test".into());
    fs::create_dir_all(dir.join(".openloop")).unwrap();
    fs::write(
        dir.join(".openloop").join("project.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
}

fn write_run_summary(dir: &Path, file: &str, cost_usd: &str, cost_source: &str, created_at: &str) {
    let content = format!(
        "# Run Summary\n\n- costUsd: {cost_usd}\n- costSource: {cost_source}\n- createdAt: {created_at}\n"
    );
    let runs = dir.join(".openloop").join("runs");
    fs::create_dir_all(&runs).unwrap();
    fs::write(runs.join(file), content).unwrap();
}

fn pending_artifact(task_id: &str) -> PromotionArtifact {
    PromotionArtifact {
        version: 1,
        created_at: now_iso(),
        project_alias: "demo".into(),
        task_id: task_id.into(),
        base_branch: Some("main".into()),
        decision: PromotionDecision::ManualReview,
        action: PromotionAction::QueueReview,
        effective_promotion_mode: TaskPromotion::PullRequest,
        validation: vec![ValidationSummary {
            name: ValidationName::Test,
            command: "bun test".into(),
            exit_code: 0,
        }],
        pi_exit_code: Some(0),
        outcome: RunOutcome::Completed,
        approval_packet_path: None,
        status: PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    }
}

#[test]
fn build_digest_aggregates_ledgers_runs_and_pending_promotions() {
    let app_home = sandbox("home");
    let demo = sandbox("demo");
    let idle = sandbox("idle");
    let app_home_str = app_home.to_string_lossy().to_string();

    // Two registered projects: one fully seeded, one idle (no control plane).
    add_project("demo", &demo.to_string_lossy(), Some(&app_home_str)).unwrap();
    add_project("idle", &idle.to_string_lossy(), Some(&app_home_str)).unwrap();

    // Ledger: one completed, one failed, one blocked — all inside the since
    // window — plus a stale completed task from 2020 that must fall outside.
    let mut done = make_task("done-task", "Done", TaskStatus::Done, TaskRisk::LowRisk);
    done.last_run = Some(last_run(&now_iso(), RunOutcome::Completed));
    let mut failed = make_task(
        "failed-task",
        "Failed",
        TaskStatus::Failed,
        TaskRisk::MediumRisk,
    );
    failed.last_run = Some(last_run(&now_iso(), RunOutcome::ValidationFailed));
    let blocked = make_task(
        "blocked-task",
        "Blocked",
        TaskStatus::Blocked,
        TaskRisk::MediumRisk,
    );
    let mut stale = make_task("stale-task", "Stale", TaskStatus::Done, TaskRisk::LowRisk);
    stale.last_run = Some(last_run(STALE, RunOutcome::Completed));
    write_ledger(&demo, vec![done, failed, blocked, stale]);

    // Config with one validation so confidence is computable.
    write_test_configured_project(&demo);

    // Run summaries with the measured/estimated cost split, plus a stale one
    // outside the window that must not contribute to spend.
    write_run_summary(&demo, "a-measured.md", "0.42", "measured", &now_iso());
    write_run_summary(&demo, "b-estimated.md", "0.10", "estimated", &now_iso());
    write_run_summary(&demo, "c-stale.md", "5.00", "measured", STALE);

    // One pending promotion with an approval packet (first attempt, low risk,
    // the configured validation passed → high confidence). Like the TS test,
    // the task itself is not in the ledger: title/risk come from the packet.
    let pending = make_task(
        "pending-task",
        "Pending",
        TaskStatus::Done,
        TaskRisk::LowRisk,
    );
    write_approval_packet(
        &demo,
        &pending,
        &ApprovalPacketData {
            diff_stat: None,
            validation: vec![ValidationSummary {
                name: ValidationName::Test,
                command: "bun test".into(),
                exit_code: 0,
            }],
            review_findings: vec![],
            cost_usd: 0.42,
            cost_source: CostSource::Measured,
            run_summary_path: None,
            spec_path: None,
        },
    )
    .unwrap();
    write_promotion_artifact(&demo, &pending_artifact("pending-task")).unwrap();

    let digest = build_digest(Some(3_600_000), Some(&app_home_str)).unwrap();

    // Entries are sorted by alias; the idle project still appears, zeroed.
    assert_eq!(digest.projects.len(), 2);
    assert_eq!(digest.projects[0].alias, "demo");
    assert_eq!(digest.projects[1].alias, "idle");

    let project = &digest.projects[0];
    assert_eq!(
        project.completed, 1,
        "stale done task must fall outside the window"
    );
    assert_eq!(project.failed, 1);
    assert_eq!(project.blocked, 1);
    assert_eq!(project.promoted, 0);
    assert_eq!(
        project.spend_usd.measured, 0.42,
        "stale run summary must not count"
    );
    assert_eq!(project.spend_usd.estimated, 0.10);

    assert_eq!(project.review_queue.len(), 1);
    let entry = &project.review_queue[0];
    assert_eq!(entry.task_id, "pending-task");
    assert_eq!(entry.title, "Pending");
    assert_eq!(entry.risk, "low-risk");
    assert_eq!(entry.confidence, DigestConfidence::High);

    let idle_entry = &digest.projects[1];
    assert_eq!(idle_entry.completed, 0);
    assert_eq!(idle_entry.failed, 0);
    assert_eq!(idle_entry.blocked, 0);
    assert_eq!(idle_entry.promoted, 0);
    assert_eq!(idle_entry.spend_usd.measured, 0.0);
    assert_eq!(idle_entry.spend_usd.estimated, 0.0);
    assert!(idle_entry.review_queue.is_empty());

    let _ = fs::remove_dir_all(&app_home);
    let _ = fs::remove_dir_all(&demo);
    let _ = fs::remove_dir_all(&idle);
}

#[test]
fn run_summary_cost_fields_take_the_first_matching_line() {
    let app_home = sandbox("costs-home");
    let solo = sandbox("costs-project");
    let app_home_str = app_home.to_string_lossy().to_string();

    add_project("solo", &solo.to_string_lossy(), Some(&app_home_str)).unwrap();
    write_ledger(&solo, vec![]);

    // Duplicate cost fields later in the file must be ignored: the first
    // matching line per field wins (TS multiline-regex parity).
    let now = now_iso();
    let content = format!(
        "# Run Summary\n\n\
         - costUsd: 0.1234\n- costSource: measured\n- createdAt: {now}\n\n\
         ## Notes\n\n\
         - costUsd: 9.0\n- costSource: estimated\n- createdAt: {STALE}\n"
    );
    let runs = solo.join(".openloop").join("runs");
    fs::create_dir_all(&runs).unwrap();
    fs::write(runs.join("dup.md"), content).unwrap();

    let digest = build_digest(Some(3_600_000), Some(&app_home_str)).unwrap();
    let project = &digest.projects[0];

    // costUsd: the first line wins (0.1234, not the later 9.0).
    assert_eq!(project.spend_usd.measured, 0.1234);
    // costSource: the first line wins, so the duplicate estimated tag never lands.
    assert_eq!(project.spend_usd.estimated, 0.0);
    // createdAt: the first line wins too — had the stale duplicate won, the
    // run would fall outside the window and no spend would be counted at all.

    let _ = fs::remove_dir_all(&app_home);
    let _ = fs::remove_dir_all(&solo);
}
