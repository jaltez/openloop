// Integration tests for the worker tick's budget-charging semantics, ported
// from the TS suite (tests/integration/worker.test.ts cost/budget cases and
// the runtime-controls pause race): measured vs estimated cost, delta
// charging against freshly-loaded state, a mid-run pause surviving the
// post-run save, the once-per-day digest stamp, and maxPendingReviews=0.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use openloop::core::daemon_state::{pause_daemon, with_daemon_state};
use openloop::core::promotion_artifacts::write_promotion_artifact;
use openloop::core::task_ledger::load_task_ledger;
use openloop::core::types::*;
use openloop::core::worker::run_worker_tick;
use parking_lot::{Mutex, MutexGuard};

// run_worker_tick (and the daemon-state helpers) resolve the app home from
// OPENLOOP_HOME; env mutation must be serialized across tests in this process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

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

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock()
}

fn sandbox(name: &str) -> Sandbox {
    let home = std::env::temp_dir().join(format!("ol-budget-home-{name}-{}", std::process::id()));
    let target =
        std::env::temp_dir().join(format!("ol-budget-target-{name}-{}", std::process::id()));
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

    Sandbox {
        home,
        target,
        saved_home,
    }
}

fn write_global_config(mutate: impl FnOnce(&mut GlobalConfig)) {
    let home_str = std::env::var("OPENLOOP_HOME").unwrap();
    let home = Path::new(&home_str);
    let mut config = openloop::core::global_config::default_global_config();
    mutate(&mut config);
    fs::write(
        home.join("config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
}

fn write_ledger(target: &Path, tasks: Vec<ProjectTask>) {
    let ledger = TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks,
    };
    fs::create_dir_all(target.join(".openloop")).unwrap();
    fs::write(
        target.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

fn make_task(id: &str, status: TaskStatus, attempts: u32) -> ProjectTask {
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
        attempts,
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

fn read_state(home: &Path) -> DaemonState {
    serde_json::from_str(&fs::read_to_string(home.join("run").join("daemon-state.json")).unwrap())
        .unwrap()
}

/// Configure the project to run `command` through the custom agent provider.
fn write_custom_agent(target: &Path, command: &str) {
    let mut config = openloop::core::project_config::default_project_config();
    config.agent = Some(AgentConfig {
        agent_type: Some("custom".into()),
        command: Some(command.into()),
        extra_args: None,
    });
    fs::create_dir_all(target.join(".openloop")).unwrap();
    fs::write(
        target.join(".openloop").join("project.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
}

fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// Restore PATH on drop (held together with ENV_LOCK by the caller).
struct PathRestore {
    saved: Option<String>,
}

impl Drop for PathRestore {
    fn drop(&mut self) {
        match self.saved.take() {
            Some(prev) => std::env::set_var("PATH", prev),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    path.exists()
}

/// Mirror of the worker's date_stamp_of: the daily budget window is anchored
/// to the local calendar date of the daemon's startedAt timestamp.
fn local_date_of(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string()
}

fn marker_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|c| c.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

fn pending_artifact(task_id: &str, created_at: &str) -> PromotionArtifact {
    PromotionArtifact {
        version: 1,
        created_at: created_at.into(),
        project_alias: "p1".into(),
        task_id: task_id.into(),
        base_branch: None,
        decision: PromotionDecision::ManualReview,
        action: PromotionAction::QueueReview,
        effective_promotion_mode: TaskPromotion::PullRequest,
        validation: vec![],
        pi_exit_code: None,
        outcome: RunOutcome::Completed,
        approval_packet_path: None,
        status: PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    }
}

#[test]
fn successful_run_charges_measured_cost_into_state_and_task() {
    let _env = env_lock();
    let s = sandbox("measured");

    // The custom agent provider never parses usage from stdout, so the
    // measured-cost path needs a last-line provider: a fake `pi` on PATH whose
    // final stdout line is the usage JSON shape extract_usage consumes.
    let bin = s.home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    write_script(
        &bin.join("pi"),
        concat!(
            "#!/bin/sh\n",
            "echo 'agent progress line'\n",
            "echo '{\"usage\":{\"input_tokens\":100,\"output_tokens\":50,",
            "\"total_tokens\":150},\"total_cost_usd\":0.42}'\n",
        ),
    );
    let saved_path = std::env::var("PATH").ok();
    std::env::set_var(
        "PATH",
        format!("{}:{}", bin.display(), saved_path.as_deref().unwrap_or("")),
    );
    let _path_restore = PathRestore { saved: saved_path };

    write_global_config(|c| {
        c.budgets.daily_cost_usd = 25.0;
        // The estimate differs from the measured cost so the assertion proves
        // the measured value won.
        c.budgets.estimated_cost_per_run_usd = Some(0.1);
    });
    let mut config = openloop::core::project_config::default_project_config();
    config.agent = Some(AgentConfig {
        agent_type: Some("pi".into()),
        command: None,
        extra_args: None,
    });
    fs::create_dir_all(s.target.join(".openloop")).unwrap();
    fs::write(
        s.target.join(".openloop").join("project.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
    write_ledger(&s.target, vec![make_task("t1", TaskStatus::Ready, 0)]);

    let state = run_worker_tick("2026-03-09T10:00:00.000Z").unwrap();

    // Measured cost lands in both the daily and lifetime budget counters.
    assert_close(state.budget_spent_usd, 0.42);
    assert_close(state.total_budget_spent_usd, 0.42);
    let persisted = read_state(&s.home);
    assert_close(persisted.budget_spent_usd, 0.42);
    assert_close(persisted.total_budget_spent_usd, 0.42);

    // The measured cost is also accumulated on the task itself.
    let ledger = load_task_ledger(&s.target).unwrap();
    let task = ledger.tasks.iter().find(|t| t.id == "t1").unwrap();
    assert_close(task.estimated_cost_usd.unwrap(), 0.42);
}

#[test]
fn timed_out_run_charges_configured_estimate() {
    let _env = env_lock();
    let s = sandbox("timeout");

    let script = s.home.join("agent.sh");
    write_script(&script, "#!/bin/sh\nsleep 30\n");
    write_custom_agent(&s.target, &script.to_string_lossy());
    write_global_config(|c| {
        c.budgets.daily_cost_usd = 25.0;
        c.budgets.estimated_cost_per_run_usd = Some(0.1);
        c.runtime.run_timeout_seconds = 1;
    });
    write_ledger(&s.target, vec![make_task("t1", TaskStatus::Ready, 0)]);

    let state = run_worker_tick("2026-03-09T10:00:00.000Z").unwrap();

    // Timeout-killed runs produce no parseable usage — they charge the
    // configured estimate.
    assert_close(state.budget_spent_usd, 0.1);
    assert_close(state.total_budget_spent_usd, 0.1);

    // The run really was the timeout path, not a fast failure.
    let p = state.projects.iter().find(|p| p.alias == "p1").unwrap();
    assert!(
        p.last_result
            .as_deref()
            .unwrap_or_default()
            .contains("exceeded timeout"),
        "lastResult: {:?}",
        p.last_result
    );
    let ledger = load_task_ledger(&s.target).unwrap();
    let task = ledger.tasks.iter().find(|t| t.id == "t1").unwrap();
    assert_eq!(task.attempts, 1);
    assert_eq!(task.last_failure_signature.as_deref(), Some("timeout"));
}

#[test]
fn mid_run_external_budget_write_survives_as_delta() {
    let _env = env_lock();
    let s = sandbox("delta");
    const STARTED: &str = "2026-03-09T10:00:00.000Z";

    // The agent signals that it is running, then waits until the external
    // writer below has charged mid-run (after the tick loaded its state).
    let marker = s.home.join("run-agent-started");
    let done = s.home.join("run-ext-done");
    let script = s.home.join("agent.sh");
    write_script(
        &script,
        &format!(
            "#!/bin/sh\ntouch '{}'\ni=0\nwhile [ ! -f '{}' ] && [ \"$i\" -lt 30 ]; do\n  sleep 1\n  i=$((i+1))\ndone\nexit 0\n",
            marker.display(),
            done.display()
        ),
    );
    write_custom_agent(&s.target, &script.to_string_lossy());
    write_global_config(|c| {
        c.budgets.daily_cost_usd = 25.0;
        c.budgets.estimated_cost_per_run_usd = Some(0.1);
    });
    write_ledger(&s.target, vec![make_task("t1", TaskStatus::Ready, 0)]);

    let writer = std::thread::spawn(move || {
        assert!(
            wait_for_file(&marker, Duration::from_secs(15)),
            "agent never started"
        );
        // An external writer (e.g. another openloop process) charges mid-run,
        // under the current budget window like any real charger would.
        with_daemon_state(
            |fresh| {
                fresh.budget_date = local_date_of(STARTED);
                fresh.budget_spent_usd = 0.5;
                fresh.total_budget_spent_usd = 0.5;
            },
            None,
        )
        .expect("external budget write");
        fs::write(&done, b"ok").unwrap();
    });

    let state = run_worker_tick(STARTED).unwrap();
    writer.join().unwrap();

    // The tick's 0.1 estimate charge is a delta on fresh state: 0.5 + 0.1,
    // not 0.1.
    assert_close(state.budget_spent_usd, 0.6);
    assert_close(state.total_budget_spent_usd, 0.6);
    assert_eq!(state.budget_date, local_date_of(STARTED));
    let persisted = read_state(&s.home);
    assert_close(persisted.budget_spent_usd, 0.6);
    assert_close(persisted.total_budget_spent_usd, 0.6);
}

#[test]
fn mid_run_pause_is_not_reverted_by_post_run_save() {
    let _env = env_lock();
    let s = sandbox("pausemid");

    let marker = s.home.join("run-agent-started");
    let done = s.home.join("run-pause-done");
    let script = s.home.join("agent.sh");
    write_script(
        &script,
        &format!(
            "#!/bin/sh\ntouch '{}'\ni=0\nwhile [ ! -f '{}' ] && [ \"$i\" -lt 30 ]; do\n  sleep 1\n  i=$((i+1))\ndone\nexit 0\n",
            marker.display(),
            done.display()
        ),
    );
    write_custom_agent(&s.target, &script.to_string_lossy());
    write_global_config(|c| {
        c.budgets.daily_cost_usd = 25.0;
        c.budgets.estimated_cost_per_run_usd = Some(0.1);
    });
    write_ledger(&s.target, vec![make_task("t1", TaskStatus::Ready, 0)]);

    // A concurrent `openloop service pause` (or MCP openloop_pause) lands
    // while the agent run is in flight — after the tick loaded its state.
    let pauser = std::thread::spawn(move || {
        assert!(
            wait_for_file(&marker, Duration::from_secs(15)),
            "agent never started"
        );
        pause_daemon(None).expect("mid-run pause");
        fs::write(&done, b"ok").unwrap();
    });

    let state = run_worker_tick("2026-03-09T10:00:00.000Z").unwrap();
    pauser.join().unwrap();

    // The post-run persist must merge into fresh state, not clobber the pause.
    assert!(state.paused);
    assert!(state.paused_at.is_some());
    // The run still completed and charged its estimate (pause stops NEW runs).
    assert!(state.current_run.is_none());
    assert_close(state.budget_spent_usd, 0.1);

    let persisted = read_state(&s.home);
    assert!(persisted.paused);
    assert!(persisted.paused_at.is_some());
    assert!(persisted.current_run.is_none());
}

#[test]
fn digest_stamp_persists_and_fires_once_per_local_day() {
    let _env = env_lock();
    let s = sandbox("digest");

    // A daily-digest-only lifecycle hook records every firing.
    let fired = s.home.join("digest-fired.txt");
    write_global_config(|c| {
        c.hooks = Some(vec![LifecycleHookConfig {
            hook_type: "command".into(),
            events: vec!["daily-digest".into()],
            command: Some(format!("echo fired >> '{}'", fired.display())),
            url: None,
            timeout_seconds: None,
            disabled: None,
        }]);
    });
    // The digest is daemon-level: an idle tick (no runnable work) still fires.
    write_ledger(&s.target, vec![]);

    let before = chrono::Local::now().format("%Y-%m-%d").to_string();
    let state = run_worker_tick("2026-03-09T10:00:00.000Z").unwrap();
    let after = chrono::Local::now().format("%Y-%m-%d").to_string();

    // The stamp is the local calendar day (tolerating a midnight crossing
    // between the tick and this assertion).
    let stamp = state.last_digest_date.expect("digest stamp set");
    assert!(
        stamp == before || stamp == after,
        "lastDigestDate {stamp} is not the local today ({before}/{after})"
    );
    assert_eq!(
        marker_lines(&fired),
        1,
        "digest must fire on the first tick"
    );
    let persisted = read_state(&s.home);
    assert_eq!(persisted.last_digest_date.as_deref(), Some(stamp.as_str()));

    // Second tick on the same day: the persisted stamp suppresses a re-fire.
    run_worker_tick("2026-03-09T10:00:05.000Z").unwrap();
    assert_eq!(
        marker_lines(&fired),
        1,
        "digest must fire at most once per local day"
    );
    assert_eq!(
        read_state(&s.home).last_digest_date.as_deref(),
        Some(stamp.as_str())
    );
}

#[test]
fn zero_pending_reviews_limit_disables_backpressure() {
    let _env = env_lock();
    let s = sandbox("nopressure");

    // Five pending promotion artifacts — over the default limit of 3.
    for i in 0..5 {
        write_promotion_artifact(
            &s.target,
            &pending_artifact(&format!("r{i}"), &format!("2026-03-09T10:00:0{i}.000Z")),
        )
        .unwrap();
    }

    let ran = s.home.join("agent-ran");
    let script = s.home.join("agent.sh");
    write_script(
        &script,
        &format!("#!/bin/sh\ntouch '{}'\nexit 0\n", ran.display()),
    );
    write_custom_agent(&s.target, &script.to_string_lossy());
    write_global_config(|c| {
        c.budgets.daily_cost_usd = 25.0;
        c.budgets.estimated_cost_per_run_usd = Some(0.1);
        c.runtime.max_pending_reviews_per_project = Some(0);
    });
    write_ledger(&s.target, vec![make_task("t1", TaskStatus::Ready, 0)]);

    let state = run_worker_tick("2026-03-09T10:00:00.000Z").unwrap();

    // The project ran despite the pending pile-up: the agent was invoked and
    // the task completed instead of being skipped for review backpressure.
    assert!(ran.exists(), "the tick must run the project");
    let p = state.projects.iter().find(|p| p.alias == "p1").unwrap();
    assert_ne!(p.last_result.as_deref(), Some("review-backpressure"));
    assert!(
        p.last_result
            .as_deref()
            .unwrap_or_default()
            .starts_with("implement:t1:"),
        "lastResult: {:?}",
        p.last_result
    );
    let ledger = load_task_ledger(&s.target).unwrap();
    let task = ledger.tasks.iter().find(|t| t.id == "t1").unwrap();
    assert_eq!(task.status, TaskStatus::Done);
}
