// Integration tests for extensible validation gates: the built-in
// lint/test/typecheck trio followed by project-defined custom gates, which
// are first-class — they run in order, stop on failure, gate auto-merge, and
// round-trip through JSON as plain strings.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openloop::core::types::*;
use openloop::core::{project_config, validation, validation_utils};
use std::fs;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn config_with(mutate: impl FnOnce(&mut ProjectConfig)) -> ProjectConfig {
    let mut config = project_config::default_project_config();
    mutate(&mut config);
    config
}

fn custom(name: &str, command: impl Into<String>) -> CustomValidation {
    CustomValidation {
        name: name.to_string(),
        command: command.into(),
    }
}

#[test]
fn configured_steps_order_trio_then_custom() {
    let config = config_with(|c| {
        c.validation.lint_command = Some("lint-cmd".into());
        c.validation.typecheck_command = Some("typecheck-cmd".into());
        c.validation.custom = Some(vec![
            custom("clippy", "cargo clippy"),
            custom("fmt", "cargo fmt"),
        ]);
    });
    let steps = validation::configured_steps(&config);
    let names: Vec<String> = steps.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec!["lint", "typecheck", "clippy", "fmt"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
    // Unconfigured trio members and empty commands are skipped.
    let config = config_with(|c| {
        c.validation.lint_command = Some("".into()); // "" is not configured
        c.validation.custom = Some(vec![custom("empty", ""), custom("real", "true")]);
    });
    let names: Vec<String> = validation::configured_steps(&config)
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(names, vec!["real".to_string()]);
}

#[test]
fn custom_gates_run_in_order_and_stop_on_failure() {
    let dir = std::env::temp_dir().join(format!("ol-valgates-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("marker");

    let config = config_with(|c| {
        c.validation.lint_command = Some(format!("echo ran-lint >> {}", marker.display()));
        c.validation.custom = Some(vec![
            custom("gate-a", format!("echo ran-a >> {}", marker.display())),
            custom("gate-fail", "exit 3".to_string()),
            custom("gate-b", format!("echo ran-b >> {}", marker.display())),
        ]);
    });

    let results = validation::run_configured_validations(&dir, &config, || None).unwrap();
    let names: Vec<String> = results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["lint", "gate-a", "gate-fail"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
        "stops at the failing gate"
    );
    assert_eq!(results[2].exit_code, 3);

    let ran = fs::read_to_string(&marker).unwrap();
    assert!(ran.contains("ran-lint"));
    assert!(ran.contains("ran-a"));
    assert!(
        !ran.contains("ran-b"),
        "gate after a failure must not run: {ran}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn custom_gates_gate_auto_merge_readiness() {
    let config = config_with(|c| {
        c.validation.lint_command = Some("true".into());
        c.validation.custom = Some(vec![custom("clippy", "cargo clippy")]);
    });
    let names = validation_utils::get_configured_validation_names(&config);
    assert_eq!(
        names,
        vec![
            ValidationName::Lint,
            ValidationName::Custom("clippy".into())
        ]
    );

    let passed_lint_only = vec![ValidationSummary {
        name: ValidationName::Lint,
        command: "true".into(),
        exit_code: 0,
    }];
    assert!(
        !validation_utils::has_all_required_auto_merge_validations(&passed_lint_only, &config),
        "auto-merge must refuse when a configured custom gate has not run"
    );

    let passed_all = vec![
        ValidationSummary {
            name: ValidationName::Lint,
            command: "true".into(),
            exit_code: 0,
        },
        ValidationSummary {
            name: ValidationName::Custom("clippy".into()),
            command: "cargo clippy".into(),
            exit_code: 0,
        },
    ];
    assert!(
        validation_utils::has_all_required_auto_merge_validations(&passed_all, &config),
        "all configured gates passing satisfies the requirement"
    );

    let failed_custom = vec![
        ValidationSummary {
            name: ValidationName::Lint,
            command: "true".into(),
            exit_code: 0,
        },
        ValidationSummary {
            name: ValidationName::Custom("clippy".into()),
            command: "cargo clippy".into(),
            exit_code: 101,
        },
    ];
    assert!(!validation_utils::has_all_required_auto_merge_validations(
        &failed_custom,
        &config
    ));
}

#[test]
fn validation_name_round_trips_as_plain_string() {
    // Known names stay exactly as before.
    for (value, wire) in [
        (ValidationName::Lint, "lint"),
        (ValidationName::Test, "test"),
        (ValidationName::Typecheck, "typecheck"),
    ] {
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, format!("\"{wire}\""));
        let back: ValidationName = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value);
    }
    // Custom names round-trip as their own string.
    let custom = ValidationName::Custom("clippy".into());
    let json = serde_json::to_string(&custom).unwrap();
    assert_eq!(json, "\"clippy\"");
    let back: ValidationName = serde_json::from_str(&json).unwrap();
    assert_eq!(back, custom);
    assert_eq!(back.as_str(), "clippy");

    // ValidationSummary round-trips inside a task ledger payload.
    let summary = ValidationSummary {
        name: ValidationName::Custom("cargo-deny".into()),
        command: "cargo deny check".into(),
        exit_code: 0,
    };
    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains("\"name\":\"cargo-deny\""), "json: {json}");
    let back: ValidationSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, summary.name);
}

// ---------------------------------------------------------------------------
// CLI round-trip
// ---------------------------------------------------------------------------

struct CliSandbox {
    home: PathBuf,
    target: PathBuf,
    saved_home: Option<String>,
}

impl Drop for CliSandbox {
    fn drop(&mut self) {
        match self.saved_home.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        let _ = fs::remove_dir_all(&self.home);
        let _ = fs::remove_dir_all(&self.target);
    }
}

fn cli_sandbox(name: &str) -> CliSandbox {
    let pid = std::process::id();
    let home = std::env::temp_dir().join(format!("ol-valcli-home-{name}-{pid}"));
    let target = std::env::temp_dir().join(format!("ol-valcli-target-{name}-{pid}"));
    for dir in [&home, &target] {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
    }
    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);
    fs::write(
        home.join("projects.json"),
        serde_json::to_string(&ProjectsRegistry {
            version: 1,
            projects: vec![LinkedProject {
                alias: "demo".into(),
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
    CliSandbox {
        home,
        target,
        saved_home,
    }
}

fn run_cli(args: &[&str]) -> Result<(), String> {
    use clap::Parser;
    let cli =
        openloop::cli::Cli::parse_from(std::iter::once("openloop").chain(args.iter().copied()));
    openloop::cli::dispatch(cli.command)
}

fn load_config(target: &Path) -> ProjectConfig {
    project_config::load_project_config(target).unwrap()
}

#[test]
fn cli_add_remove_validation_round_trips() {
    let _env = env_lock();
    let s = cli_sandbox("roundtrip");

    run_cli(&[
        "config",
        "project-add-validation",
        "demo",
        "clippy",
        "--command",
        "cargo clippy --all-targets -- -D warnings",
    ])
    .unwrap();
    run_cli(&[
        "config",
        "project-add-validation",
        "demo",
        "cargo_deny",
        "--command",
        "cargo deny check",
    ])
    .unwrap();

    let config = load_config(&s.target);
    assert_eq!(
        config.validation.custom.unwrap(),
        vec![
            custom("clippy", "cargo clippy --all-targets -- -D warnings"),
            custom("cargo_deny", "cargo deny check"),
        ]
    );

    // Reserved and malformed names are rejected without touching config.
    assert!(run_cli(&[
        "config",
        "project-add-validation",
        "demo",
        "lint",
        "--command",
        "x"
    ])
    .is_err());
    assert!(run_cli(&[
        "config",
        "project-add-validation",
        "demo",
        "bad name!",
        "--command",
        "x"
    ])
    .is_err());
    assert!(run_cli(&[
        "config",
        "project-add-validation",
        "demo",
        "clippy",
        "--command",
        "dup"
    ])
    .is_err());
    assert_eq!(load_config(&s.target).validation.custom.unwrap().len(), 2);

    run_cli(&["config", "project-remove-validation", "demo", "clippy"]).unwrap();
    let config = load_config(&s.target);
    assert_eq!(
        config.validation.custom.unwrap(),
        vec![custom("cargo_deny", "cargo deny check")]
    );

    assert!(run_cli(&["config", "project-remove-validation", "demo", "ghost"]).is_err());

    // Removing the last gate drops the key entirely.
    run_cli(&["config", "project-remove-validation", "demo", "cargo_deny"]).unwrap();
    assert!(load_config(&s.target).validation.custom.is_none());
}
