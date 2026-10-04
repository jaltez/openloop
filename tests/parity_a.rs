// Ported TS behavioral coverage: model resolution precedence, global config
// defaults/merge, doctor checks, git fingerprint + stage/commit, and the
// remaining fsutil rotation/nesting cases.
//
// Every API under test takes an explicit app-home/project override, so no
// environment variables are mutated and no cross-test lock is needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use openloop::core::doctor::{format_doctor_results, run_doctor_checks, CheckResult};
use openloop::core::fsutil::{ensure_dir, rotate_log_file, write_json_file};
use openloop::core::git::{
    commit_staged_changes, get_git_diff_fingerprint, get_git_working_tree_state, run_git,
    stage_all_changes,
};
use openloop::core::global_config::{
    default_global_config, load_global_config, save_global_config,
};
use openloop::core::model_resolution::resolve_model;
use openloop::core::paths::{global_config_path, projects_registry_path, runtime_dir};
use openloop::core::project_config::{default_project_config, save_project_config};

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ol-parity-a-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_project_model(project_root: &Path, model: Option<&str>) {
    let mut config = default_project_config();
    config.pi.model = model.map(str::to_string);
    save_project_config(project_root, &config).unwrap();
}

fn write_global_model(app_home: &Path, model: Option<&str>) {
    let mut config = default_global_config();
    config.model = model.map(str::to_string);
    save_global_config(&config, app_home.to_str()).unwrap();
}

// ---------------------------------------------------------------- git

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
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn init_git_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.email", "test@test.com"]);
    git(dir, &["config", "user.name", "Test"]);
    fs::write(dir.join("README.md"), "init\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "initial"]);
}

#[test]
fn diff_fingerprint_is_empty_string_for_clean_repo() {
    let dir = temp_root("fp-clean");
    init_git_repo(&dir);

    let fingerprint = get_git_diff_fingerprint(&dir);
    assert_eq!(fingerprint.as_deref(), Some(""));
}

#[test]
fn diff_fingerprint_contains_filename_for_dirty_repo() {
    let dir = temp_root("fp-dirty");
    init_git_repo(&dir);
    fs::write(dir.join("new.txt"), "change\n").unwrap();

    let fingerprint = get_git_diff_fingerprint(&dir).expect("fingerprint for dirty repo");
    assert!(!fingerprint.is_empty());
    assert!(fingerprint.contains("new.txt"));
}

#[test]
fn diff_fingerprint_is_none_for_non_git_directory() {
    let dir = temp_root("fp-no-git");

    assert_eq!(get_git_diff_fingerprint(&dir), None);
}

#[test]
fn stage_all_and_commit_staged_create_a_commit() {
    let dir = temp_root("stage-commit");
    init_git_repo(&dir);
    fs::write(dir.join("file.txt"), "content\n").unwrap();

    stage_all_changes(&dir).unwrap();
    commit_staged_changes(&dir, "add file").unwrap();

    let state = get_git_working_tree_state(&dir).unwrap();
    assert!(!state.dirty);

    let (log, _) = run_git(&dir, &["log", "--oneline"]).unwrap();
    assert!(log.contains("add file"));
}

// ---------------------------------------------------- model resolution

#[test]
fn resolve_model_precedence_is_cli_then_project_then_global() {
    let app_home = temp_root("model-home");
    let project_root = temp_root("model-project");
    write_project_model(&project_root, Some("anthropic/project-model"));
    write_global_model(&app_home, Some("openai/global-model"));
    let home = app_home.to_str().unwrap();

    // CLI override wins over project and global.
    assert_eq!(
        resolve_model(&project_root, Some("google/cli-model"), Some(home)).unwrap(),
        Some("google/cli-model".to_string())
    );
    // An explicitly empty CLI string falls through to the project config.
    assert_eq!(
        resolve_model(&project_root, Some(""), Some(home)).unwrap(),
        Some("anthropic/project-model".to_string())
    );
    // Project beats global when no CLI override is given.
    assert_eq!(
        resolve_model(&project_root, None, Some(home)).unwrap(),
        Some("anthropic/project-model".to_string())
    );

    // Project model cleared: global model alone resolves.
    write_project_model(&project_root, None);
    assert_eq!(
        resolve_model(&project_root, None, Some(home)).unwrap(),
        Some("openai/global-model".to_string())
    );

    // No model anywhere: resolves to None.
    write_global_model(&app_home, None);
    assert_eq!(
        resolve_model(&project_root, None, Some(home)).unwrap(),
        None
    );
}

// ------------------------------------------------------- global config

#[test]
fn loads_global_config_defaults_with_runtime_settings() {
    let app_home = temp_root("gc-defaults");

    let config = load_global_config(app_home.to_str()).unwrap();
    assert_eq!(config.budgets.daily_cost_usd, 25.0);
    assert_eq!(config.runtime.run_timeout_seconds, 1800);
    assert_eq!(config.runtime.max_attempts_per_task, 3);
    assert_eq!(config.runtime.no_progress_repeat_limit, 2);
}

#[test]
fn merges_missing_runtime_fields_from_older_global_config_files() {
    let app_home = temp_root("gc-merge");

    fs::write(
        global_config_path(app_home.to_str()),
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "model": "demo/model",
            "activeProjectAlias": null,
            "budgets": { "dailyCostUsd": 10 },
        }))
        .unwrap(),
    )
    .unwrap();

    let config = load_global_config(app_home.to_str()).unwrap();
    assert_eq!(config.model.as_deref(), Some("demo/model"));
    assert_eq!(config.budgets.daily_cost_usd, 10.0);
    assert_eq!(config.runtime.run_timeout_seconds, 1800);
    assert_eq!(config.runtime.max_attempts_per_task, 3);
    assert_eq!(config.runtime.no_progress_repeat_limit, 2);
}

// -------------------------------------------------------------- doctor

fn empty_registry(app_home: &str) {
    write_json_file(
        &projects_registry_path(Some(app_home)),
        &serde_json::json!({ "version": 1, "projects": [] }),
    )
    .unwrap();
}

fn doctor_result<'a>(results: &'a [CheckResult], label: &str) -> &'a CheckResult {
    results
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("missing doctor check {label}"))
}
#[test]
fn doctor_reports_ok_for_valid_global_config() {
    let app_home = temp_root("doc-ok");
    let mut config = default_global_config();
    config.budgets.daily_cost_usd = 25.0;
    save_global_config(&config, app_home.to_str()).unwrap();
    empty_registry(app_home.to_str().unwrap());
    ensure_dir(&runtime_dir(app_home.to_str())).unwrap();

    let results = run_doctor_checks(app_home.to_str()).unwrap();
    assert_eq!(doctor_result(&results, "Global config").status, "ok");
}

#[test]
fn doctor_warns_on_zero_budget_ceiling() {
    let app_home = temp_root("doc-budget");
    let mut config = default_global_config();
    config.budgets.daily_cost_usd = 0.0;
    save_global_config(&config, app_home.to_str()).unwrap();
    empty_registry(app_home.to_str().unwrap());

    let results = run_doctor_checks(app_home.to_str()).unwrap();
    assert_eq!(doctor_result(&results, "Budget ceiling").status, "warn");
}

#[test]
fn doctor_warns_when_no_projects_are_linked() {
    let app_home = temp_root("doc-projects");
    save_global_config(&default_global_config(), app_home.to_str()).unwrap();
    empty_registry(app_home.to_str().unwrap());

    let results = run_doctor_checks(app_home.to_str()).unwrap();
    let projects_check = doctor_result(&results, "Linked projects");
    assert_eq!(projects_check.status, "warn");
    assert!(projects_check.detail.contains("No projects linked"));
}

#[test]
fn doctor_fails_on_invalid_json_config() {
    let app_home = temp_root("doc-invalid");
    fs::create_dir_all(&app_home).unwrap();
    fs::write(global_config_path(app_home.to_str()), "{{invalid json").unwrap();
    empty_registry(app_home.to_str().unwrap());

    let results = run_doctor_checks(app_home.to_str()).unwrap();
    let config_check = doctor_result(&results, "Global config");
    assert_eq!(config_check.status, "fail");
    assert!(config_check.detail.contains("contains invalid JSON"));
}

#[test]
fn format_doctor_results_marks_ok_warn_and_fail_lines() {
    let results = vec![
        CheckResult {
            label: "git".to_string(),
            status: "ok",
            detail: "found at /usr/bin/git".to_string(),
        },
        CheckResult {
            label: "pi".to_string(),
            status: "fail",
            detail: "'pi' not found on PATH".to_string(),
        },
        CheckResult {
            label: "Budget".to_string(),
            status: "warn",
            detail: "Budget is 0".to_string(),
        },
    ];

    let output = format_doctor_results(&results);
    assert!(output.contains("✅ git:"));
    assert!(output.contains("❌ pi:"));
    assert!(output.contains("⚠️ Budget:"));
}

// -------------------------------------------------------------- fsutil

#[test]
fn rotate_log_file_leaves_small_files_untouched() {
    let dir = temp_root("rot-small");
    let log_path = dir.join("events.jsonl");
    fs::write(&log_path, "small\n").unwrap();

    rotate_log_file(&log_path, 1024, 3);

    assert_eq!(fs::read_to_string(&log_path).unwrap(), "small\n");
    assert!(!dir.join("events.jsonl.1").exists());
}

#[test]
fn rotate_log_file_renames_oversized_and_shifts_backups() {
    let dir = temp_root("rot-shift");
    let log_path = dir.join("events.jsonl");
    fs::write(dir.join("events.jsonl.1"), "older\n").unwrap();
    fs::write(dir.join("events.jsonl.2"), "oldest\n").unwrap();
    let oversized = "x".repeat(11);
    fs::write(&log_path, &oversized).unwrap();

    rotate_log_file(&log_path, 10, 3);

    assert!(!log_path.exists());
    assert_eq!(
        fs::read_to_string(dir.join("events.jsonl.1")).unwrap(),
        oversized
    );
    assert_eq!(
        fs::read_to_string(dir.join("events.jsonl.2")).unwrap(),
        "older\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("events.jsonl.3")).unwrap(),
        "oldest\n"
    );
}

#[test]
fn write_json_file_creates_nested_parent_directories() {
    let dir = temp_root("json-nested");
    let file_path = dir.join("nested").join("deep").join("file.json");

    write_json_file(&file_path, &serde_json::json!({ "nested": true })).unwrap();

    let raw = fs::read_to_string(&file_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value, serde_json::json!({ "nested": true }));
}
