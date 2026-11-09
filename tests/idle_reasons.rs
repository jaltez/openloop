// Integration tests for the fleet-idle inspector: per-project gate reasons
// computed live, mirroring the tick's own gating order.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openloop::core::types::*;
use openloop::core::{daemon_state, global_config, idle_reasons};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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

fn sandbox(name: &str, initialized: bool) -> Sandbox {
    let pid = std::process::id();
    let home = std::env::temp_dir().join(format!("ol-idle-home-{name}-{pid}"));
    let target = std::env::temp_dir().join(format!("ol-idle-target-{name}-{pid}"));
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
                alias: "demo".into(),
                path: target.to_string_lossy().to_string(),
                default_branch: None,
                initialized,
                created_at: "2026-01-01T00:00:00.000Z".into(),
                updated_at: "2026-01-01T00:00:00.000Z".into(),
            }],
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(
        home.join("config.json"),
        serde_json::to_string(&global_config::default_global_config()).unwrap(),
    )
    .unwrap();
    Sandbox {
        home,
        target,
        saved_home,
    }
}

fn write_project(target: &Path) {
    let mut config = openloop::core::project_config::default_project_config();
    config.agent = Some(AgentConfig {
        agent_type: Some("custom".into()),
        command: Some("true".into()),
        extra_args: None,
    });
    fs::create_dir_all(target.join(".openloop")).unwrap();
    fs::write(
        target.join(".openloop").join("project.json"),
        serde_json::to_string(&config).unwrap(),
    )
    .unwrap();
}

fn task(id: &str, status: TaskStatus) -> ProjectTask {
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
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn write_ledger(target: &Path, tasks: Vec<ProjectTask>) {
    fs::create_dir_all(target.join(".openloop")).unwrap();
    fs::write(
        target.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&TaskLedger {
            version: 1,
            updated_at: "2026-01-01T00:00:00.000Z".into(),
            tasks,
        })
        .unwrap(),
    )
    .unwrap();
}

fn kind_for(status: &idle_reasons::FleetIdleStatus) -> String {
    status.projects[0].kind.clone()
}

#[test]
fn names_each_gate_in_tick_order() {
    let _env = env_lock();

    // Paused daemon dominates everything.
    let s = sandbox("paused", true);
    write_project(&s.target);
    write_ledger(&s.target, vec![task("t", TaskStatus::Ready)]);
    daemon_state::pause_daemon(None).unwrap();
    let status = idle_reasons::analyze().unwrap();
    assert_eq!(status.daemon.as_ref().unwrap().kind, "paused");
    assert_eq!(kind_for(&status), "paused");

    // Budget exhausted.
    let s = sandbox("budget", true);
    write_project(&s.target);
    write_ledger(&s.target, vec![task("t", TaskStatus::Ready)]);
    daemon_state::with_daemon_state(
        |state| {
            state.budget_spent_usd = 100.0;
        },
        None,
    )
    .unwrap();
    let status = idle_reasons::analyze().unwrap();
    assert_eq!(status.daemon.as_ref().unwrap().kind, "budget-blocked");
    assert_eq!(kind_for(&status), "budget-blocked");

    // Not initialized.
    let _s = sandbox("uninit", false);
    let status = idle_reasons::analyze().unwrap();
    assert_eq!(kind_for(&status), "not-initialized");

    // Review backpressure (limit 3 by default).
    let s = sandbox("backpressure", true);
    write_project(&s.target);
    write_ledger(&s.target, vec![task("t", TaskStatus::Ready)]);
    for i in 0..3 {
        let artifact = PromotionArtifact {
            version: 1,
            created_at: "2026-01-02T00:00:00.000Z".into(),
            project_alias: "demo".into(),
            task_id: format!("t{i}"),
            base_branch: Some("main".into()),
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
        };
        openloop::core::promotion_artifacts::write_promotion_artifact(&s.target, &artifact)
            .unwrap();
    }
    let status = idle_reasons::analyze().unwrap();
    assert_eq!(kind_for(&status), "review-backpressure");
    assert!(status.projects[0].reason.contains("3 pending"));

    // Unsatisfied dependency.
    let s = sandbox("deps", true);
    write_project(&s.target);
    let mut gated = task("deploy", TaskStatus::Ready);
    gated.depends_on = Some(vec!["ghost:upstream".into()]);
    write_ledger(&s.target, vec![gated]);
    let status = idle_reasons::analyze().unwrap();
    assert_eq!(kind_for(&status), "dependencies");
    assert!(status.projects[0].reason.contains("ghost:upstream"));

    // Empty queue.
    let s = sandbox("empty", true);
    write_project(&s.target);
    write_ledger(&s.target, vec![]);
    let status = idle_reasons::analyze().unwrap();
    assert_eq!(kind_for(&status), "no-eligible-tasks");

    // Runnable work with a provider configured: ready.
    let s = sandbox("ready", true);
    write_project(&s.target);
    write_ledger(&s.target, vec![task("t", TaskStatus::Ready)]);
    let status = idle_reasons::analyze().unwrap();
    assert_eq!(kind_for(&status), "ready");
}
