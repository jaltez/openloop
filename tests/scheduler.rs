use chrono::TimeZone;
use openloop::core::types::*;

fn make_task(
    id: &str,
    status: TaskStatus,
    risk: TaskRisk,
    updated_at: &str,
    depends_on: Option<Vec<&str>>,
) -> ProjectTask {
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
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
        attempts: 0,
        last_failure_signature: None,
        promotion: TaskPromotion::PullRequest,
        promoted_at: None,
        depends_on: depends_on.map(|deps| deps.iter().map(|s| s.to_string()).collect()),
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: updated_at.into(),
        updated_at: updated_at.into(),
    }
}

fn ledger(tasks: Vec<ProjectTask>) -> TaskLedger {
    TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks,
    }
}

#[test]
fn prefers_oldest_ready_low_risk() {
    let l = ledger(vec![
        make_task(
            "b",
            TaskStatus::Ready,
            TaskRisk::LowRisk,
            "2026-01-03T00:00:00.000Z",
            None,
        ),
        make_task(
            "a",
            TaskStatus::Ready,
            TaskRisk::LowRisk,
            "2026-01-01T00:00:00.000Z",
            None,
        ),
    ]);
    let selection = openloop::core::scheduler::select_next_task(&l);
    assert_eq!(selection.task.unwrap().id, "a");
    assert_eq!(selection.mode, "implement");
}

#[test]
fn low_risk_beats_recent_medium_risk() {
    // Fresh timestamps (relative to now) so the aging rule stays out of play.
    let fresh = |hours_ago: i64| {
        chrono::Utc
            .timestamp_millis_opt(openloop::core::now_millis() - hours_ago * 3_600_000)
            .single()
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
            .unwrap()
    };
    let l = ledger(vec![
        make_task("low", TaskStatus::Ready, TaskRisk::LowRisk, &fresh(1), None),
        make_task(
            "med",
            TaskStatus::Ready,
            TaskRisk::MediumRisk,
            &fresh(2),
            None,
        ),
    ]);
    let selection = openloop::core::scheduler::select_next_task(&l);
    assert_eq!(selection.task.unwrap().id, "low");
}

#[test]
fn medium_risk_starves_until_aged() {
    let fresh = ledger(vec![make_task(
        "m",
        TaskStatus::Ready,
        TaskRisk::MediumRisk,
        "2026-06-01T00:00:00.000Z",
        None,
    )]);
    // No low-risk tasks at all: the medium-risk task is still the only candidate.
    let selection = openloop::core::scheduler::select_next_task(&fresh);
    assert_eq!(selection.task.unwrap().id, "m");
    assert!(selection.reason.contains("aged past 24h"));
}

#[test]
fn unmet_dependency_idle() {
    let l = ledger(vec![make_task(
        "blocked",
        TaskStatus::Ready,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
        Some(vec!["missing"]),
    )]);
    let selection = openloop::core::scheduler::select_next_task(&l);
    assert!(selection.task.is_none());
    assert_eq!(selection.mode, "idle");
    assert!(selection.reason.contains("unsatisfied dependsOn"));
}

#[test]
fn satisfied_dependency_runs() {
    let l = ledger(vec![
        make_task(
            "dep",
            TaskStatus::Done,
            TaskRisk::LowRisk,
            "2026-01-01T00:00:00.000Z",
            None,
        ),
        make_task(
            "child",
            TaskStatus::Ready,
            TaskRisk::LowRisk,
            "2026-01-02T00:00:00.000Z",
            Some(vec!["dep"]),
        ),
    ]);
    let selection = openloop::core::scheduler::select_next_task(&l);
    assert_eq!(selection.task.unwrap().id, "child");
}

#[test]
fn cross_project_refs_are_never_satisfied() {
    let l = ledger(vec![make_task(
        "x",
        TaskStatus::Ready,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
        Some(vec!["otherproj:dep"]),
    )]);
    let selection = openloop::core::scheduler::select_next_task(&l);
    assert!(selection.task.is_none());
    assert!(selection.reason.contains("cross-project"));
}

#[test]
fn falls_back_to_planning_when_nothing_ready() {
    let l = ledger(vec![make_task(
        "p",
        TaskStatus::Proposed,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
        None,
    )]);
    let selection = openloop::core::scheduler::select_next_task(&l);
    assert_eq!(selection.mode, "plan");
    assert_eq!(selection.task.unwrap().id, "p");
}

#[test]
fn promotion_decisions() {
    use openloop::core::scheduler::{
        decide_promotion, decide_promotion_action, resolve_effective_promotion_mode,
    };
    use openloop::core::validation_utils::get_configured_validation_names;

    let mut config = openloop::core::project_config::default_project_config();
    config.validation.lint_command = Some("lint".into());
    let policy = openloop::core::project_policy::default_project_policy();

    let mut done_low = make_task(
        "t",
        TaskStatus::Done,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
        None,
    );
    done_low.promotion = TaskPromotion::AutoMerge;

    let validation = vec![openloop::core::types::ValidationSummary {
        name: openloop::core::types::ValidationName::Lint,
        command: "lint".into(),
        exit_code: 0,
    }];

    // Failing validation always blocks.
    let failed = vec![openloop::core::types::ValidationSummary {
        name: openloop::core::types::ValidationName::Lint,
        command: "lint".into(),
        exit_code: 1,
    }];
    assert_eq!(
        decide_promotion(&done_low, &failed, TaskPromotion::AutoMerge, &config),
        PromotionDecision::Blocked
    );
    assert_eq!(
        decide_promotion_action(PromotionDecision::Blocked),
        PromotionAction::Block
    );

    // Not done -> none.
    let mut not_done = done_low.clone();
    not_done.status = TaskStatus::InProgress;
    assert_eq!(
        decide_promotion(&not_done, &validation, TaskPromotion::AutoMerge, &config),
        PromotionDecision::None
    );

    // Done + low risk + all configured validations passing + auto-merge mode -> eligible.
    assert_eq!(
        decide_promotion(&done_low, &validation, TaskPromotion::AutoMerge, &config),
        PromotionDecision::AutoMergeEligible
    );
    assert_eq!(
        decide_promotion_action(PromotionDecision::AutoMergeEligible),
        PromotionAction::QueueAutoMerge
    );

    // No validations configured forces manual review even for auto-merge mode.
    let mut no_val = config.clone();
    no_val.validation.lint_command = None;
    assert!(get_configured_validation_names(&no_val).is_empty());
    assert_eq!(
        decide_promotion(&done_low, &validation, TaskPromotion::AutoMerge, &no_val),
        PromotionDecision::ManualReview
    );

    // Manual-only promotion mode -> manual review.
    let mut manual = done_low.clone();
    manual.promotion = TaskPromotion::ManualOnly;
    let mode = resolve_effective_promotion_mode(&manual, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::ManualOnly);
    assert_eq!(
        decide_promotion(&manual, &validation, mode, &config),
        PromotionDecision::ManualReview
    );
    assert_eq!(
        decide_promotion_action(PromotionDecision::ManualReview),
        PromotionAction::QueueReview
    );
}

#[test]
fn no_progress_counts_repeats_and_blocks_at_limit() {
    use openloop::core::scheduler::{should_stop_for_no_progress, NoProgressInput};

    let mut task = make_task(
        "t",
        TaskStatus::Ready,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
        None,
    );
    task.last_failure_signature = Some("validation-lint".into());

    fn step(task: &mut ProjectTask) -> bool {
        let sig = task.last_failure_signature.clone();
        let input = NoProgressInput {
            previous_failure_signature: sig.as_deref(),
            previous_promotion_decision: Some(PromotionDecision::Blocked),
            current_promotion_decision: PromotionDecision::Blocked,
            before_fingerprint: Some("same".into()),
            after_fingerprint: Some("same".into()),
            no_progress_repeat_limit: 3,
        };
        should_stop_for_no_progress(task, &input)
    }

    // Run 1 and 2: counts increment, no block.
    assert!(!step(&mut task));
    assert!(!step(&mut task));
    // Run 3: failure + promotion-blocked + diff-unchanged all reach 3 -> blocked.
    assert!(step(&mut task));
    let notes = task.notes.unwrap();
    assert!(notes.contains(&"openloop:no-progress:diff-unchanged:3".to_string()));
    assert!(notes.contains(&"openloop:no-progress:failure-validation-lint:3".to_string()));
    assert!(notes.contains(&"openloop:no-progress:promotion-blocked:3".to_string()));
}

#[test]
fn run_cost_prefers_measured_positive() {
    use openloop::core::scheduler::resolve_run_cost;

    let measured = AgentRunResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        usage: Some(AgentUsage {
            input_tokens: Some(10.0),
            output_tokens: None,
            total_tokens: None,
            cost_usd: Some(0.25),
        }),
    };
    assert_eq!(
        resolve_run_cost(&measured, 0.1),
        (0.25, CostSource::Measured)
    );

    let unmeasured = AgentRunResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        usage: None,
    };
    assert_eq!(
        resolve_run_cost(&unmeasured, 0.1),
        (0.1, CostSource::Estimated)
    );

    let zero_cost = AgentRunResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        usage: Some(AgentUsage {
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cost_usd: Some(0.0),
        }),
    };
    assert_eq!(
        resolve_run_cost(&zero_cost, 0.1),
        (0.1, CostSource::Estimated)
    );
}

// ---------------------------------------------------------------------------
// Cross-project dependencies (alias:task-id refs resolved via the registry)
// ---------------------------------------------------------------------------

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static XENV_LOCK: Mutex<()> = Mutex::new(());

struct XRepoSandbox {
    home: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
    saved_home: Option<String>,
}

impl Drop for XRepoSandbox {
    fn drop(&mut self) {
        match self.saved_home.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        let _ = fs::remove_dir_all(&self.home);
        let _ = fs::remove_dir_all(&self.alpha);
        let _ = fs::remove_dir_all(&self.beta);
    }
}

fn write_registry_and_ledgers(
    home: &std::path::Path,
    alpha: &std::path::Path,
    beta: &std::path::Path,
    alpha_tasks: Vec<ProjectTask>,
    beta_tasks: Vec<ProjectTask>,
) {
    let linked = |alias: &str, path: &std::path::Path| LinkedProject {
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
            projects: vec![linked("alpha", alpha), linked("beta", beta)],
        })
        .unwrap(),
    )
    .unwrap();
    write_ledger(alpha, alpha_tasks);
    write_ledger(beta, beta_tasks);
}

fn write_ledger(dir: &std::path::Path, tasks: Vec<ProjectTask>) {
    let l = TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks,
    };
    fs::create_dir_all(dir.join(".openloop")).unwrap();
    fs::write(
        dir.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&l).unwrap(),
    )
    .unwrap();
}

fn xrepo_sandbox(name: &str) -> XRepoSandbox {
    let pid = std::process::id();
    let home = std::env::temp_dir().join(format!("ol-xrepo-home-{name}-{pid}"));
    let alpha = std::env::temp_dir().join(format!("ol-xrepo-alpha-{name}-{pid}"));
    let beta = std::env::temp_dir().join(format!("ol-xrepo-beta-{name}-{pid}"));
    for dir in [&home, &alpha, &beta] {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
    }
    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);
    XRepoSandbox {
        home,
        alpha,
        beta,
        saved_home,
    }
}

fn select_with_registry(l: &TaskLedger) -> SchedulerSelection {
    let resolver = openloop::core::scheduler::make_cross_project_resolver();
    openloop::core::scheduler::select_next_task_with_resolver(l, &resolver)
}

fn cross_dep_task(dep: &str) -> ProjectTask {
    make_task(
        "deploy",
        TaskStatus::Ready,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
        Some(vec![dep]),
    )
}

fn beta_api_task(status: TaskStatus) -> ProjectTask {
    make_task(
        "api",
        status,
        TaskRisk::LowRisk,
        "2026-01-01T00:00:00.000Z",
        None,
    )
}

#[test]
fn cross_project_dep_blocks_until_referenced_task_is_promoted() {
    let _env = XENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let s = xrepo_sandbox("gated");
    let deploy = cross_dep_task("beta:api");

    // Dependency still in flight: nothing eligible.
    write_registry_and_ledgers(
        &s.home,
        &s.alpha,
        &s.beta,
        vec![deploy.clone()],
        vec![beta_api_task(TaskStatus::InProgress)],
    );
    let alpha_ledger = openloop::core::task_ledger::load_task_ledger(&s.alpha).unwrap();
    let selection = select_with_registry(&alpha_ledger);
    assert!(
        selection.task.is_none(),
        "must stay blocked while beta:api is in progress"
    );
    assert!(
        selection.reason.contains("cross-project"),
        "reason: {}",
        selection.reason
    );

    // Dependency promoted: the task becomes eligible.
    write_ledger(&s.beta, vec![beta_api_task(TaskStatus::Promoted)]);
    let alpha_ledger = openloop::core::task_ledger::load_task_ledger(&s.alpha).unwrap();
    let selection = select_with_registry(&alpha_ledger);
    assert_eq!(selection.task.expect("eligible once promoted").id, "deploy");

    // Done and Cancelled also satisfy the dependency.
    for status in [TaskStatus::Done, TaskStatus::Cancelled] {
        write_ledger(&s.beta, vec![beta_api_task(status)]);
        let alpha_ledger = openloop::core::task_ledger::load_task_ledger(&s.alpha).unwrap();
        let selection = select_with_registry(&alpha_ledger);
        assert!(
            selection.task.is_some(),
            "{status:?} must satisfy a cross-project dependency"
        );
    }
}

#[test]
fn unknown_cross_project_ref_stays_blocked() {
    let _env = XENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let s = xrepo_sandbox("unknown");
    let deploy = cross_dep_task("ghost:missing");

    write_registry_and_ledgers(&s.home, &s.alpha, &s.beta, vec![deploy], vec![]);
    let alpha_ledger = openloop::core::task_ledger::load_task_ledger(&s.alpha).unwrap();
    let selection = select_with_registry(&alpha_ledger);
    assert!(
        selection.task.is_none(),
        "unknown alias must leave the task blocked"
    );
    assert!(
        selection.reason.contains("ghost:missing"),
        "reason should name the unresolved ref: {}",
        selection.reason
    );
}
