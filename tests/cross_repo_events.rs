// Integration tests for cross-repo dependency events: when an upstream task
// lands (promoted/done/cancelled), waiting tasks get an audit event, a ledger
// note, and notifications — exactly once per ref.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openloop::core::types::*;
use openloop::core::worker::run_worker_tick;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct DepSandbox {
    home: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
    bin: PathBuf,
    saved_home: Option<String>,
    saved_path: Option<String>,
}

impl Drop for DepSandbox {
    fn drop(&mut self) {
        match self.saved_home.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        match self.saved_path.take() {
            Some(prev) => std::env::set_var("PATH", prev),
            None => std::env::remove_var("PATH"),
        }
        let _ = fs::remove_dir_all(&self.home);
        let _ = fs::remove_dir_all(&self.alpha);
        let _ = fs::remove_dir_all(&self.beta);
        let _ = fs::remove_dir_all(&self.bin);
    }
}

fn dep_sandbox(name: &str, gh_json: Option<&str>) -> DepSandbox {
    let pid = std::process::id();
    let home = std::env::temp_dir().join(format!("ol-depev-home-{name}-{pid}"));
    let alpha = std::env::temp_dir().join(format!("ol-depev-alpha-{name}-{pid}"));
    let beta = std::env::temp_dir().join(format!("ol-depev-beta-{name}-{pid}"));
    let bin = std::env::temp_dir().join(format!("ol-depev-bin-{name}-{pid}"));
    for dir in [&home, &alpha, &beta, &bin] {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
    }

    // Instant stub agent for both projects: unblocked tasks run (and finish)
    // immediately instead of invoking a real provider from PATH.
    for dir in [&alpha, &beta] {
        let mut config = openloop::core::project_config::default_project_config();
        config.agent = Some(AgentConfig {
            agent_type: Some("custom".into()),
            command: Some("true".into()),
            extra_args: None,
        });
        config.review = Some(ProjectReview { enabled: false });
        config.verification = Some(ProjectVerification { enabled: false });
        fs::create_dir_all(dir.join(".openloop")).unwrap();
        fs::write(
            dir.join(".openloop").join("project.json"),
            serde_json::to_string(&config).unwrap(),
        )
        .unwrap();
    }

    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);
    let saved_path = std::env::var("PATH").ok();
    std::env::set_var(
        "PATH",
        format!("{}:{}", bin.display(), saved_path.as_deref().unwrap_or("")),
    );

    let linked = |alias: &str, path: &Path| LinkedProject {
        alias: alias.into(),
        path: path.to_string_lossy().to_string(),
        default_branch: None,
        initialized: true,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    };
    fs::write(
        home.join("projects.json"),
        serde_json::to_string(&ProjectsRegistry {
            version: 1,
            projects: vec![linked("alpha", &alpha), linked("beta", &beta)],
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(
        home.join("config.json"),
        serde_json::to_string(&openloop::core::global_config::default_global_config()).unwrap(),
    )
    .unwrap();
    if let Some(gh_json) = gh_json {
        let stub = bin.join("gh");
        fs::write(&stub, format!("#!/bin/sh\necho '{gh_json}'\n")).unwrap();
        let mut perms = fs::metadata(&stub).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub, perms).unwrap();
    }
    DepSandbox {
        home,
        alpha,
        beta,
        bin,
        saved_home,
        saved_path,
    }
}

fn task(id: &str, status: TaskStatus, depends_on: Option<Vec<&str>>) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status,
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
        depends_on: depends_on.map(|deps| deps.iter().map(|s| s.to_string()).collect()),
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn write_ledger(dir: &Path, tasks: Vec<ProjectTask>) {
    fs::create_dir_all(dir.join(".openloop")).unwrap();
    fs::write(
        dir.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&TaskLedger {
            version: 1,
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            tasks,
        })
        .unwrap(),
    )
    .unwrap();
}

fn read_task(dir: &Path, id: &str) -> ProjectTask {
    openloop::core::task_ledger::load_task_ledger(dir)
        .unwrap()
        .tasks
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

fn dependency_events(s: &DepSandbox) -> usize {
    fs::read_to_string(s.home.join("run").join("events.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("\"dependency_satisfied\""))
        .count()
}

#[test]
fn satisfaction_fires_once_with_note_and_event() {
    let _env = env_lock();
    let s = dep_sandbox("once", Some(r#"{"state":"OPEN","statusCheckRollup":[]}"#));
    write_ledger(
        &s.alpha,
        vec![task("deploy", TaskStatus::Ready, Some(vec!["beta:api"]))],
    );
    write_ledger(&s.beta, vec![task("api", TaskStatus::InProgress, None)]);

    // Tick 1 seeds the pending set; nothing fires.
    run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();
    assert_eq!(dependency_events(&s), 0);

    // Upstream lands.
    write_ledger(&s.beta, vec![task("api", TaskStatus::Promoted, None)]);
    run_worker_tick("2026-01-01T00:00:05.000Z").unwrap();
    assert_eq!(
        dependency_events(&s),
        1,
        "transition must fire exactly once"
    );

    let deploy = read_task(&s.alpha, "deploy");
    let notes = deploy.notes.unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.contains("beta:api satisfied (promoted)")),
        "notes: {notes:?}"
    );

    // Ticks after the transition never re-fire.
    run_worker_tick("2026-01-01T00:00:10.000Z").unwrap();
    run_worker_tick("2026-01-01T00:00:15.000Z").unwrap();
    assert_eq!(dependency_events(&s), 1);
}

#[test]
fn already_satisfied_before_first_tick_fires_nothing() {
    let _env = env_lock();
    let s = dep_sandbox("seeded", Some(r#"{"state":"OPEN","statusCheckRollup":[]}"#));
    write_ledger(
        &s.alpha,
        vec![task("deploy", TaskStatus::Ready, Some(vec!["beta:api"]))],
    );
    write_ledger(&s.beta, vec![task("api", TaskStatus::Done, None)]);

    // The dependency predates the daemon: seeding must not fire retroactively.
    run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();
    assert_eq!(dependency_events(&s), 0);
    let deploy = read_task(&s.alpha, "deploy");
    assert!(deploy
        .notes
        .unwrap_or_default()
        .iter()
        .all(|n| !n.contains("satisfied")));
}

#[test]
fn pr_merge_unblocks_downstream_within_the_same_tick() {
    let _env = env_lock();
    let s = dep_sandbox(
        "sametick",
        Some(r#"{"state":"MERGED","statusCheckRollup":[]}"#),
    );

    // beta's task is mid-flight with a PR result artifact; alpha waits on it.
    // (InProgress does not satisfy a dependency — the PR merge is what
    // promotes the task and unblocks alpha in this same tick.)
    write_ledger(
        &s.alpha,
        vec![task("deploy", TaskStatus::Ready, Some(vec!["beta:api"]))],
    );
    let mut api = task("api", TaskStatus::InProgress, None);
    api.promotion = TaskPromotion::PullRequest;
    write_ledger(&s.beta, vec![api]);
    let artifact = PromotionResultArtifact {
        version: 1,
        created_at: "2026-01-02T00:00:00.000Z".into(),
        project_alias: "beta".into(),
        task_id: "api".into(),
        source_promotion_artifact_path: "unused".into(),
        source_promotion_action: PromotionAction::QueueReview,
        source_promotion_decision: PromotionDecision::ManualReview,
        result: "applied".into(),
        branch: Some("openloop/api".into()),
        base_branch: None,
        note: Some("Created PR".into()),
        pr_url: Some("https://github.com/org/repo/pull/11".into()),
        state: None,
        checks: None,
    };
    openloop::core::promotion_artifacts::write_promotion_result_artifact(&s.beta, &artifact)
        .unwrap();

    // One tick: the PR poll promotes beta's task, then the dependency scan
    // unblocks alpha — no extra tick needed.
    run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();

    let events = fs::read_to_string(s.home.join("run").join("events.jsonl")).unwrap_or_default();
    assert!(events.contains("pr_merged"), "events: {events}");
    assert!(events.contains("dependency_satisfied"), "events: {events}");

    let api = read_task(&s.beta, "api");
    assert_eq!(api.status, TaskStatus::Promoted);
    let deploy = read_task(&s.alpha, "deploy");
    assert!(deploy
        .notes
        .unwrap_or_default()
        .iter()
        .any(|n| n.contains("beta:api satisfied (promoted)")));
}

#[test]
fn unknown_dependency_ref_never_fires_and_never_fails_the_tick() {
    let _env = env_lock();
    let s = dep_sandbox("ghost", Some(r#"{"state":"OPEN","statusCheckRollup":[]}"#));
    write_ledger(
        &s.alpha,
        vec![task(
            "deploy",
            TaskStatus::Ready,
            Some(vec!["ghost:missing"]),
        )],
    );
    write_ledger(&s.beta, vec![]);

    run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();
    run_worker_tick("2026-01-01T00:00:05.000Z").unwrap();
    assert_eq!(dependency_events(&s), 0);
    assert_eq!(read_task(&s.alpha, "deploy").status, TaskStatus::Ready);
}
