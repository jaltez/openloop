// Integration tests for template versioning and `project upgrade`: the
// embedded templates carry a version; init stamps it, upgrade brings linked
// repos up to it while preserving user-owned state, and doctor reports drift.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openloop::core::templates::{
    extract_templates_to, initialize_project_from_templates, template_version_of,
    upgrade_project_from_templates, TEMPLATE_VERSION,
};
use openloop::core::types::*;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn project(alias: &str, path: &Path) -> LinkedProject {
    LinkedProject {
        alias: alias.into(),
        path: path.to_string_lossy().to_string(),
        default_branch: None,
        initialized: true,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    }
}

fn repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ol-tplupgrade-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn embedded_file(rel: &str) -> String {
    let tmp = std::env::temp_dir().join(format!(
        "ol-tplupgrade-embedded-{}-{rel}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    extract_templates_to(&tmp).unwrap();
    let content = fs::read_to_string(tmp.join(rel)).unwrap();
    let _ = fs::remove_dir_all(&tmp);
    content
}

fn backup_count(target: &Path) -> usize {
    fs::read_dir(target.join(".openloop").join("backup"))
        .map(|entries| entries.flatten().count())
        .unwrap_or(0)
}

#[test]
fn init_stamps_the_current_template_version() {
    let target = repo("stamp");
    initialize_project_from_templates(Path::new("/"), &project("demo", &target), false).unwrap();
    assert_eq!(template_version_of(&target), Some(TEMPLATE_VERSION));
    // --force re-init re-stamps it too.
    initialize_project_from_templates(Path::new("/"), &project("demo", &target), true).unwrap();
    assert_eq!(template_version_of(&target), Some(TEMPLATE_VERSION));
}

#[test]
fn upgrade_refreshes_templates_but_preserves_user_state() {
    let target = repo("upgrade");
    initialize_project_from_templates(Path::new("/"), &project("demo", &target), false).unwrap();

    // Simulate an older project: stale marker + user-owned state.
    fs::write(target.join(".openloop").join("template-version"), "0\n").unwrap();
    fs::write(
        target.join(".openloop").join("policy.yaml"),
        "# user-tuned policy\ndenyGlobs:\n  - secret/**\n",
    )
    .unwrap();
    let mut config = openloop::core::project_config::load_project_config(&target).unwrap();
    config.validation.lint_command = Some("my-lint".into());
    config.validation.custom = Some(vec![CustomValidation {
        name: "clippy".into(),
        command: "cargo clippy".into(),
    }]);
    openloop::core::project_config::save_project_config(&target, &config).unwrap();
    fs::write(
        target.join(".openloop").join("tasks.json"),
        serde_json::to_string(&TaskLedger {
            version: 1,
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            tasks: vec![ProjectTask {
                id: "keep-me".into(),
                title: "Keep me".into(),
                kind: TaskKind::Feature,
                status: TaskStatus::Ready,
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
                created_at: "2026-01-01T00:00:00.000Z".into(),
                updated_at: "2026-01-01T00:00:00.000Z".into(),
            }],
        })
        .unwrap(),
    )
    .unwrap();
    // Stale template-owned file: upgrade must refresh it.
    fs::write(
        target
            .join(".agents")
            .join("skills")
            .join("openloop")
            .join("SKILL.md"),
        "stale skill doc",
    )
    .unwrap();

    let report = upgrade_project_from_templates(&project("demo", &target)).unwrap();
    assert!(!report.already_current);
    assert_eq!(report.from, Some(0));
    assert_eq!(report.to, TEMPLATE_VERSION);
    assert!(report.backup_dir.is_some());

    // Marker is current now.
    assert_eq!(template_version_of(&target), Some(TEMPLATE_VERSION));

    // User-owned files preserved exactly.
    assert_eq!(
        fs::read_to_string(target.join(".openloop").join("policy.yaml")).unwrap(),
        "# user-tuned policy\ndenyGlobs:\n  - secret/**\n",
        "policy.yaml must never be overwritten"
    );
    let config = openloop::core::project_config::load_project_config(&target).unwrap();
    assert_eq!(config.validation.lint_command.as_deref(), Some("my-lint"));
    assert_eq!(
        config.validation.custom.unwrap()[0].name,
        "clippy",
        "custom gates survive the merge"
    );

    // Task ledger untouched.
    let ledger = openloop::core::task_ledger::load_task_ledger(&target).unwrap();
    assert_eq!(ledger.tasks[0].id, "keep-me");

    // Template-owned file refreshed from the embedded copy.
    assert_eq!(
        fs::read_to_string(
            target
                .join(".agents")
                .join("skills")
                .join("openloop")
                .join("SKILL.md")
        )
        .unwrap(),
        embedded_file(".agents/skills/openloop/SKILL.md"),
        "stale skill doc must be refreshed"
    );

    // Backup captured the pre-upgrade state.
    assert_eq!(backup_count(&target), 1);

    // Upgrading again is a no-op.
    let report = upgrade_project_from_templates(&project("demo", &target)).unwrap();
    assert!(report.already_current);
    assert!(report.backup_dir.is_none());
    assert_eq!(backup_count(&target), 1, "no second backup for a no-op");
}

#[test]
fn upgrade_fails_cleanly_on_uninitialized_project() {
    let target = repo("uninit");
    let error = upgrade_project_from_templates(&project("demo", &target)).unwrap_err();
    assert!(error.contains("not initialized"), "error: {error}");
}

// ---------------------------------------------------------------------------
// doctor drift check + CLI
// ---------------------------------------------------------------------------

struct HomeSandbox {
    home: PathBuf,
    target: PathBuf,
    saved_home: Option<String>,
}

impl Drop for HomeSandbox {
    fn drop(&mut self) {
        match self.saved_home.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        let _ = fs::remove_dir_all(&self.home);
        let _ = fs::remove_dir_all(&self.target);
    }
}

fn home_sandbox(name: &str) -> HomeSandbox {
    let pid = std::process::id();
    let home = std::env::temp_dir().join(format!("ol-tpl-home-{name}-{pid}"));
    let target = std::env::temp_dir().join(format!("ol-tpl-target-{name}-{pid}"));
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
            projects: vec![project("demo", &target)],
        })
        .unwrap(),
    )
    .unwrap();
    initialize_project_from_templates(Path::new("/"), &project("demo", &target), false).unwrap();
    HomeSandbox {
        home,
        target,
        saved_home,
    }
}

#[test]
fn doctor_reports_template_drift_and_current() {
    let _env = env_lock();
    let s = home_sandbox("doctor");

    let find = |results: &[openloop::core::doctor::CheckResult]| {
        results
            .iter()
            .find(|c| c.label == "demo: templates")
            .unwrap()
            .clone()
    };

    // Current: ok.
    let results = openloop::core::doctor::run_doctor_checks(None).unwrap();
    let check = find(&results);
    assert_eq!(check.status, "ok");

    // Stale marker: warn with the upgrade hint.
    fs::write(s.target.join(".openloop").join("template-version"), "0\n").unwrap();
    let results = openloop::core::doctor::run_doctor_checks(None).unwrap();
    let check = find(&results);
    assert_eq!(check.status, "warn");
    assert!(check.detail.contains("project upgrade"), "{}", check.detail);

    // Pre-versioning (no marker): warn with the versioning hint.
    fs::remove_file(s.target.join(".openloop").join("template-version")).unwrap();
    let results = openloop::core::doctor::run_doctor_checks(None).unwrap();
    let check = find(&results);
    assert_eq!(check.status, "warn");
    assert!(check.detail.contains("versioning"), "{}", check.detail);
}

#[test]
fn cli_upgrade_round_trips() {
    use clap::Parser;
    let _env = env_lock();
    let s = home_sandbox("cli");
    fs::write(s.target.join(".openloop").join("template-version"), "0\n").unwrap();

    let cli = openloop::cli::Cli::parse_from(["openloop", "project", "upgrade", "demo"]);
    openloop::cli::dispatch(cli.command).unwrap();

    assert_eq!(template_version_of(&s.target), Some(TEMPLATE_VERSION));

    // Second call reports the no-op.
    let cli = openloop::cli::Cli::parse_from(["openloop", "project", "upgrade", "demo"]);
    openloop::cli::dispatch(cli.command).unwrap();
    assert_eq!(backup_count(&s.target), 1);
}
