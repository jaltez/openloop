// Integration tests for control-plane retention: newest-N run artifacts and
// promotion results per task; foreign files and the promotions queue are
// never touched; the daemon tick enforces the policy.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::TimeZone;
use openloop::core::types::*;
use openloop::core::{project_config, retention, worker::run_worker_tick};
use std::fs;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn stamp(offset_seconds: i64) -> String {
    chrono::Utc
        .timestamp_millis_opt(1_800_000_000_000 + offset_seconds * 1000)
        .single()
        .unwrap()
        .format("%Y-%m-%dT%H-%M-%S%.3fZ")
        .to_string()
}

struct RetSandbox {
    home: Option<PathBuf>,
    target: PathBuf,
    saved_home: Option<String>,
}

impl Drop for RetSandbox {
    fn drop(&mut self) {
        if let Some(home) = &self.home {
            let _ = fs::remove_dir_all(home);
        }
        let _ = fs::remove_dir_all(&self.target);
        if self.home.is_some() {
            match self.saved_home.take() {
                Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
                None => std::env::remove_var("OPENLOOP_HOME"),
            }
        }
    }
}

/// Project dir only (no daemon home): direct enforce_retention tests.
fn project_sandbox(name: &str) -> RetSandbox {
    let target = std::env::temp_dir().join(format!("ol-retention-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).unwrap();
    RetSandbox {
        home: None,
        target,
        saved_home: None,
    }
}

/// Sandbox with a daemon home and a single registered project.
fn home_sandbox(name: &str) -> RetSandbox {
    let pid = std::process::id();
    let home = std::env::temp_dir().join(format!("ol-ret-home-{name}-{pid}"));
    let target = std::env::temp_dir().join(format!("ol-ret-target-{name}-{pid}"));
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
    RetSandbox {
        home: Some(home),
        target,
        saved_home,
    }
}

fn seed_runs(target: &Path, task: &str, count: i64) {
    let dir = target.join(".openloop").join("runs");
    fs::create_dir_all(&dir).unwrap();
    for i in 0..count {
        let ts = stamp(i * 10);
        fs::write(dir.join(format!("{ts}-{task}.md")), "summary").unwrap();
        fs::write(
            dir.join(format!("{ts}-{task}.transcript.log")),
            "transcript",
        )
        .unwrap();
    }
}

fn seed_results(target: &Path, task: &str, count: i64) {
    let dir = target.join(".openloop").join("promotion-results");
    fs::create_dir_all(&dir).unwrap();
    for i in 0..count {
        let ts = stamp(i * 10);
        fs::write(dir.join(format!("{ts}-{task}.json")), "{}").unwrap();
    }
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

fn config_with(retention_policy: ProjectRetention) -> ProjectConfig {
    let mut config = project_config::default_project_config();
    config.retention = Some(retention_policy);
    config
}

#[test]
fn keeps_newest_runs_and_results_per_task() {
    let s = project_sandbox("newest");
    seed_runs(&s.target, "alpha", 6); // 12 files
    seed_runs(&s.target, "beta", 2); // 4 files
    seed_results(&s.target, "alpha", 4);
    // Foreign files and the promotions queue must survive untouched.
    let runs_dir = s.target.join(".openloop").join("runs");
    fs::write(runs_dir.join("README.md"), "notes").unwrap();
    let promo_dir = s.target.join(".openloop").join("promotions");
    fs::create_dir_all(&promo_dir).unwrap();
    fs::write(promo_dir.join("keep-me.json"), "{}").unwrap();

    let (runs_pruned, results_pruned) = retention::enforce_retention(
        &s.target,
        &config_with(ProjectRetention {
            max_run_artifacts_per_task: 3,
            max_promotion_results_per_task: 2,
        }),
    )
    .unwrap();

    assert_eq!(runs_pruned, 6, "alpha drops 3 runs x2 files, beta none");
    assert_eq!(results_pruned, 2, "alpha keeps 2 of 4 results");

    let names = file_names(&runs_dir);
    assert_eq!(names.len(), 11, "3+2 runs (x2 files) + README: {names:?}");
    assert!(names.contains(&"README.md".to_string()));
    // Newest alpha run survives.
    assert!(names.contains(&format!("{}-alpha.md", stamp(50))));
    assert!(!names.contains(&format!("{}-alpha.md", stamp(0))));
    // Beta fully below the cap.
    assert!(names.contains(&format!("{}-beta.md", stamp(10))));

    let results = file_names(&s.target.join(".openloop").join("promotion-results"));
    assert_eq!(results.len(), 2);
    assert!(results.contains(&format!("{}-alpha.json", stamp(30))));
    assert!(!results.contains(&format!("{}-alpha.json", stamp(0))));

    assert!(s
        .target
        .join(".openloop")
        .join("promotions")
        .join("keep-me.json")
        .exists());
}

#[test]
fn defaults_apply_when_policy_absent() {
    let s = project_sandbox("defaults");
    seed_runs(&s.target, "alpha", 30); // 60 files, over the default 20
    let (runs_pruned, results_pruned) =
        retention::enforce_retention(&s.target, &project_config::default_project_config()).unwrap();
    assert_eq!(
        runs_pruned, 20,
        "30 runs - 20 default cap = 10 runs x2 files"
    );
    assert_eq!(results_pruned, 0);
    assert_eq!(
        file_names(&s.target.join(".openloop").join("runs")).len(),
        40
    );
}

#[test]
fn worker_tick_enforces_retention() {
    let _env = env_lock();
    let s = home_sandbox("tick");
    seed_runs(&s.target, "alpha", 8);
    let config = config_with(ProjectRetention {
        max_run_artifacts_per_task: 2,
        max_promotion_results_per_task: 5,
    });
    fs::create_dir_all(s.target.join(".openloop")).unwrap();
    fs::write(
        s.target.join(".openloop").join("project.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
    fs::write(
        s.home.as_ref().unwrap().join("config.json"),
        serde_json::to_string(&openloop::core::global_config::default_global_config()).unwrap(),
    )
    .unwrap();
    fs::write(
        s.target.join(".openloop").join("tasks.json"),
        serde_json::to_string(&TaskLedger {
            version: 1,
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            tasks: vec![],
        })
        .unwrap(),
    )
    .unwrap();

    run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();

    assert_eq!(
        file_names(&s.target.join(".openloop").join("runs")).len(),
        4
    );
    let log = fs::read_to_string(s.home.as_ref().unwrap().join("run").join("daemon.log"))
        .unwrap_or_default();
    assert!(log.contains("retention pruned 12 run"), "log: {log}");
}
