use std::fs;

use openloop::core::fsutil::{read_file_tail, read_json_file, write_json_file};
use openloop::core::types::{PromotionArtifact, PromotionResultArtifact};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("openloop-fs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample_artifact() -> PromotionArtifact {
    PromotionArtifact {
        version: 1,
        created_at: "2026-10-01T12:34:56.789Z".into(),
        project_alias: "p".into(),
        task_id: "fix-readme-typo".into(),
        base_branch: Some("main".into()),
        decision: openloop::core::types::PromotionDecision::ManualReview,
        action: openloop::core::types::PromotionAction::QueueReview,
        effective_promotion_mode: openloop::core::types::TaskPromotion::PullRequest,
        validation: vec![],
        pi_exit_code: Some(0),
        outcome: openloop::core::types::RunOutcome::Completed,
        approval_packet_path: None,
        status: openloop::core::types::PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    }
}

#[test]
fn json_round_trip_is_pretty_with_trailing_newline() {
    let dir = temp_dir("json");
    let path = dir.join("state.json");
    write_json_file(&path, &sample_artifact()).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert!(raw.ends_with("}\n"));
    assert!(raw.contains("\n  ")); // pretty-printed
    let back: PromotionArtifact = read_json_file(&path, serde_json::Value::Null)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap();
    assert_eq!(back.task_id, "fix-readme-typo");
}

#[test]
fn corrupt_json_is_a_hard_error() {
    let dir = temp_dir("corrupt");
    let path = dir.join("bad.json");
    fs::write(&path, "{ not json").unwrap();
    let result: Result<PromotionArtifact, String> = read_json_file(&path, serde_json::Value::Null)
        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()));
    assert!(result.is_err());
}

#[test]
fn missing_file_returns_fallback_verbatim() {
    let dir = temp_dir("missing");
    // The fallback value is returned as-is when the file does not exist.
    let value: serde_json::Value =
        read_json_file(&dir.join("nope.json"), serde_json::Value::Null).unwrap();
    assert!(value.is_null());
    let value: serde_json::Value =
        read_json_file(&dir.join("nope.json"), serde_json::json!({"a": 1})).unwrap();
    assert_eq!(value, serde_json::json!({"a": 1}));
}

#[test]
fn artifact_filenames_strip_colons_and_dots() {
    // Regression: TS strips /[:.]/g; the port used to keep '.' producing a
    // different on-disk convention in shared homes.
    let dir = temp_dir("names");
    let path =
        openloop::core::promotion_artifacts::write_promotion_artifact(&dir, &sample_artifact())
            .unwrap();
    let name = std::path::Path::new(&path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(name, "2026-10-01T12-34-56-789Z-fix-readme-typo.json");
    assert!(!name.trim_end_matches(".json").contains('.'));

    let result_path = openloop::core::promotion_artifacts::write_promotion_result_artifact(
        &dir,
        &PromotionResultArtifact {
            version: 1,
            created_at: "2026-10-01T12:34:56.789Z".into(),
            project_alias: "p".into(),
            task_id: "fix-readme-typo".into(),
            source_promotion_artifact_path: path.clone(),
            source_promotion_action: openloop::core::types::PromotionAction::QueueReview,
            source_promotion_decision: openloop::core::types::PromotionDecision::ManualReview,
            result: "applied".into(),
            branch: None,
            base_branch: None,
            note: None,
            pr_url: None,
            state: None,
            checks: None,
        },
    )
    .unwrap();
    let result_name = std::path::Path::new(&result_path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(result_name, "2026-10-01T12-34-56-789Z-fix-readme-typo.json");
}

#[test]
fn run_summary_filename_strips_colons_and_dots() {
    let dir = temp_dir("summary");
    let summary = openloop::core::types::SchedulerResult {
        project_alias: "p".into(),
        task_id: Some("t".into()),
        mode: "idle".into(),
        role: None,
        reason: "r".into(),
        model: None,
        exit_code: None,
        prompt: None,
        validation: vec![],
        promotion_decision: openloop::core::types::PromotionDecision::None,
        promotion_action: openloop::core::types::PromotionAction::None,
        promotion_artifact_path: None,
        promotion_result_artifact_path: None,
        task_status: None,
        promoted_at: None,
        stopped_by: openloop::core::types::RunStoppedBy::None,
        attempt_number: None,
        dirty_tree_detected: false,
        budget_snapshot_usd: None,
        review_findings: None,
        run_summary_path: None,
        cost_usd: None,
        cost_source: None,
        approval_packet_path: None,
    };
    let path = openloop::core::run_summaries::write_run_summary(&dir, &summary, None).unwrap();
    let name = std::path::Path::new(&path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(name.ends_with("-t.md"), "got {name}");
    assert!(!name.trim_end_matches(".md").contains('.'), "got {name}");
}

#[test]
fn read_file_tail_returns_last_lines_aligned() {
    let dir = temp_dir("tail");
    let path = dir.join("log.txt");
    let mut content = String::new();
    for i in 0..2000 {
        content.push_str(&format!("line-{i:06}\n"));
    }
    fs::write(&path, &content).unwrap();

    let tail = read_file_tail(&path, 1024).unwrap();
    assert!(tail.starts_with("line-"));
    assert!(tail.contains("line-001999"));
    // No partial first line.
    let first = tail.lines().next().unwrap();
    assert_eq!(first.len(), "line-001999".len());
}

#[test]
fn read_file_tail_small_file_reads_all() {
    let dir = temp_dir("tail-small");
    let path = dir.join("log.txt");
    fs::write(&path, "a\nb\nc\n").unwrap();
    let tail = read_file_tail(&path, 1024).unwrap();
    assert_eq!(tail, "a\nb\nc\n");
}
