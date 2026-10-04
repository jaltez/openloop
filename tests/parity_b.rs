//! Parity port of the deleted TS unit suites: scheduler-tasks, scheduler
//! self-healing, the no-progress fingerprint residual, the resolveRunCost
//! residuals, and validation-utils.

use openloop::core::scheduler::{
    build_prompt, determine_worker_role, get_self_healing_block, is_self_healing_task,
    is_supported_self_healing_task, resolve_run_cost, should_stop_for_no_progress, NoProgressInput,
};
use openloop::core::types::*;
use openloop::core::validation_utils::{
    get_configured_validation_names, has_all_required_auto_merge_validations,
};

fn make_task(id: &str) -> ProjectTask {
    ProjectTask {
        id: id.to_string(),
        title: "Test task".to_string(),
        kind: TaskKind::Feature,
        status: TaskStatus::Ready,
        risk: TaskRisk::LowRisk,
        scope: None,
        source: TaskSource {
            source_type: "human".to_string(),
            r#ref: "test".to_string(),
        },
        spec_id: None,
        branch: None,
        owner: None,
        acceptance_criteria: vec!["Do the thing".to_string(), "Pass tests".to_string()],
        attempts: 0,
        last_failure_signature: None,
        promotion: TaskPromotion::AutoMerge,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: "2026-01-01T00:00:00.000Z".to_string(),
        updated_at: "2026-01-01T00:00:00.000Z".to_string(),
    }
}

fn make_policy() -> ProjectPolicy {
    ProjectPolicy {
        version: 1,
        scope: PolicyScope {
            allow_globs: vec![],
            deny_globs: vec![],
            high_risk_areas: vec![],
        },
        risk_classes: [
            (
                "low-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: true,
                    requires_human_review: false,
                },
            ),
            (
                "medium-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: false,
                    requires_human_review: true,
                },
            ),
            (
                "high-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: false,
                    requires_human_review: true,
                },
            ),
        ]
        .into_iter()
        .collect(),
        self_healing: SelfHealingPolicy {
            enabled: true,
            allowed_task_kinds: vec![
                "lint-fix".to_string(),
                "type-fix".to_string(),
                "localized-test-fix".to_string(),
            ],
        },
        promotion: PolicyPromotionModes {
            low_risk_mode: TaskPromotion::AutoMerge,
            medium_risk_mode: TaskPromotion::PullRequest,
            high_risk_mode: TaskPromotion::PullRequest,
        },
    }
}

fn make_config(validation: ProjectValidation) -> ProjectConfig {
    ProjectConfig {
        version: 1,
        project: ProjectMeta {
            alias: None,
            repo_root: None,
            initialized_at: None,
        },
        pi: PiConfig {
            model: None,
            prompt_files: vec![],
        },
        agent: None,
        runtime: ProjectRuntime {
            use_worktree: false,
            branch_prefix: "openloop/".to_string(),
            pr_command: None,
            worktree_setup_command: None,
        },
        validation,
        risk: ProjectRisk {
            default_unknown_area_classification: "medium-risk".to_string(),
            require_policy_for_auto_merge: true,
        },
        issue_source: None,
        hooks: None,
        schedule: None,
        schedule_state: None,
        review: None,
        verification: None,
    }
}

fn validation_config(
    lint: Option<&str>,
    test: Option<&str>,
    typecheck: Option<&str>,
) -> ProjectConfig {
    make_config(ProjectValidation {
        lint_command: lint.map(str::to_string),
        test_command: test.map(str::to_string),
        typecheck_command: typecheck.map(str::to_string),
    })
}

// ---------------------------------------------------------------------------
// build_prompt (tests/unit/scheduler-tasks.test.ts)
// ---------------------------------------------------------------------------

#[test]
fn build_prompt_includes_task_metadata_in_implement_mode() {
    let mut task = make_task("t1");
    task.title = "Add validation".to_string();
    let prompt = build_prompt(&task, "implement", WorkerRole::Implementer, None, true);

    assert!(prompt.contains("Implement the following task."));
    assert!(prompt.contains("Task ID: t1"));
    assert!(prompt.contains("Title: Add validation"));
    assert!(prompt.contains("Kind: feature"));
    assert!(prompt.contains("Assigned Role: implementer"));
    assert!(prompt.contains("Risk: low-risk"));
    assert!(prompt.contains("Source: human / test"));
}

#[test]
fn build_prompt_prepends_agents_policy_guidance_for_non_pi_providers_only() {
    let task = make_task("t2");

    let non_pi = build_prompt(&task, "implement", WorkerRole::Implementer, None, false);
    assert!(non_pi.starts_with("Read AGENTS.md and .openloop/policy.yaml before making changes."));

    let pi = build_prompt(&task, "implement", WorkerRole::Implementer, None, true);
    assert!(!pi.contains("Read AGENTS.md and .openloop/policy.yaml"));

    // TS defaulted providerIsPi to true (Pi's system-prompt files carry the
    // conventions); the Rust signature passes it explicitly — the Pi call
    // above is exactly that default path.
}

#[test]
fn build_prompt_uses_plan_mode_header_for_plan_tasks() {
    let task = make_task("t2");
    let prompt = build_prompt(&task, "plan", WorkerRole::SddPlanner, None, true);

    assert!(prompt.contains("Plan the following task and prepare it for implementation."));
    assert!(prompt.contains("Spec Output: Write your implementation plan to .openloop/specs/t2.md"));
}

#[test]
fn build_prompt_includes_scope_paths_when_present() {
    let mut task = make_task("t3");
    task.scope = Some(TaskScope {
        paths: vec!["src/auth.ts".to_string(), "tests/auth.test.ts".to_string()],
    });
    let prompt = build_prompt(&task, "implement", WorkerRole::Implementer, None, true);

    assert!(prompt.contains("Scope Paths:"));
    assert!(prompt.contains("- src/auth.ts"));
    assert!(prompt.contains("- tests/auth.test.ts"));
}

#[test]
fn build_prompt_includes_acceptance_criteria() {
    let task = make_task("t4");
    let prompt = build_prompt(&task, "implement", WorkerRole::Implementer, None, true);

    assert!(prompt.contains("Acceptance Criteria:"));
    assert!(prompt.contains("- Do the thing"));
    assert!(prompt.contains("- Pass tests"));
}

#[test]
fn build_prompt_includes_spec_content_in_implement_mode_when_provided() {
    let task = make_task("t5");
    let prompt = build_prompt(
        &task,
        "implement",
        WorkerRole::Implementer,
        Some("## Plan\n1. Add validation\n2. Write tests"),
        true,
    );

    assert!(prompt.contains("## Implementation Spec"));
    assert!(prompt.contains("## Plan"));
    assert!(prompt.contains("1. Add validation"));
}

#[test]
fn build_prompt_does_not_include_spec_content_in_plan_mode() {
    let task = make_task("t6");
    let prompt = build_prompt(
        &task,
        "plan",
        WorkerRole::SddPlanner,
        Some("some spec content"),
        true,
    );

    assert!(!prompt.contains("## Implementation Spec"));
}

// The three repair-header tests below also pin the describe strings of the
// private `describe_self_healing_task` through its only pub seam
// (`build_prompt`): "lint issue", "type issue", "localized deterministic
// test failure".

#[test]
fn build_prompt_uses_self_healing_header_for_lint_fix_tasks() {
    let mut task = make_task("t7");
    task.kind = TaskKind::LintFix;
    let prompt = build_prompt(&task, "implement", WorkerRole::CiHealer, None, true);

    assert!(prompt.contains("Repair the following lint issue"));
    assert!(prompt.contains("Self-Healing Scope:"));
}

#[test]
fn build_prompt_uses_self_healing_header_for_type_fix_tasks() {
    let mut task = make_task("t8");
    task.kind = TaskKind::TypeFix;
    let prompt = build_prompt(&task, "implement", WorkerRole::CiHealer, None, true);

    assert!(prompt.contains("Repair the following type issue"));
}

#[test]
fn build_prompt_uses_self_healing_header_for_localized_test_fix_tasks() {
    let mut task = make_task("t9");
    task.kind = TaskKind::LocalizedTestFix;
    let prompt = build_prompt(&task, "implement", WorkerRole::CiHealer, None, true);

    assert!(prompt.contains("Repair the following localized deterministic test failure"));
}

// ---------------------------------------------------------------------------
// Self-healing gating (tests/unit/scheduler-self-healing.test.ts)
// ---------------------------------------------------------------------------

#[test]
fn is_self_healing_task_matches_repair_kinds() {
    assert!(is_self_healing_task(TaskKind::LintFix));
    assert!(is_self_healing_task(TaskKind::TypeFix));
    assert!(is_self_healing_task(TaskKind::LocalizedTestFix));
    assert!(is_self_healing_task(TaskKind::CiHeal));
    assert!(!is_self_healing_task(TaskKind::Feature));
    assert!(!is_self_healing_task(TaskKind::Bugfix));
}

#[test]
fn is_supported_self_healing_task_drops_ci_heal() {
    assert!(is_supported_self_healing_task(TaskKind::LintFix));
    assert!(is_supported_self_healing_task(TaskKind::TypeFix));
    assert!(is_supported_self_healing_task(TaskKind::LocalizedTestFix));
    // ci-heal is a self-healing kind, but it is not in the V1 supported list.
    assert!(!is_supported_self_healing_task(TaskKind::CiHeal));
}

#[test]
fn self_healing_block_is_absent_for_non_self_healing_tasks() {
    let mut task = make_task("t1");
    task.kind = TaskKind::Feature;
    assert!(get_self_healing_block(&task, &make_policy()).is_none());
}

#[test]
fn self_healing_block_is_returned_when_self_healing_is_disabled() {
    let mut task = make_task("t1");
    task.kind = TaskKind::LintFix;
    let mut policy = make_policy();
    policy.self_healing = SelfHealingPolicy {
        enabled: false,
        allowed_task_kinds: vec![],
    };

    let block = get_self_healing_block(&task, &policy).expect("block for disabled policy");
    assert_eq!(block.failure_signature, "self-healing-disabled");
    assert!(block.reason.contains("disabled"));
}

#[test]
fn self_healing_block_is_returned_for_unsupported_ci_heal_kind() {
    let mut task = make_task("t1");
    task.kind = TaskKind::CiHeal;

    let block = get_self_healing_block(&task, &make_policy()).expect("block for ci-heal");
    assert_eq!(block.failure_signature, "self-healing-kind-not-supported");
    assert!(block.reason.contains("outside the V1 scope"));
}

#[test]
fn self_healing_block_is_returned_when_kind_is_not_allowed_by_policy() {
    let mut task = make_task("t1");
    task.kind = TaskKind::LintFix;
    let mut policy = make_policy();
    policy.self_healing.allowed_task_kinds = vec!["type-fix".to_string()];

    let block = get_self_healing_block(&task, &policy).expect("block for non-allowlisted kind");
    assert_eq!(block.failure_signature, "self-healing-kind-not-allowed");
}

#[test]
fn self_healing_block_is_absent_when_enabled_and_kind_allowed() {
    let mut task = make_task("t1");
    task.kind = TaskKind::LintFix;
    assert!(get_self_healing_block(&task, &make_policy()).is_none());
}

#[test]
fn worker_role_is_repo_improver_for_discovery_and_scope_proposal() {
    let mut task = make_task("t1");

    task.kind = TaskKind::Discovery;
    assert_eq!(
        determine_worker_role(&task, "plan"),
        WorkerRole::RepoImprover
    );

    task.kind = TaskKind::ScopeProposal;
    assert_eq!(
        determine_worker_role(&task, "plan"),
        WorkerRole::RepoImprover
    );
}

#[test]
fn worker_role_is_ci_healer_for_supported_repair_kinds() {
    let mut task = make_task("t1");

    task.kind = TaskKind::LintFix;
    assert_eq!(
        determine_worker_role(&task, "implement"),
        WorkerRole::CiHealer
    );

    task.kind = TaskKind::TypeFix;
    assert_eq!(
        determine_worker_role(&task, "implement"),
        WorkerRole::CiHealer
    );
}

#[test]
fn worker_role_follows_mode_for_feature_tasks() {
    let task = make_task("t1");
    assert_eq!(determine_worker_role(&task, "plan"), WorkerRole::SddPlanner);
    assert_eq!(
        determine_worker_role(&task, "implement"),
        WorkerRole::Implementer
    );
}

// ---------------------------------------------------------------------------
// No-progress residual (tests/unit/scheduler-no-progress.test.ts)
// ---------------------------------------------------------------------------

#[test]
fn no_progress_never_counts_when_fingerprints_differ() {
    let mut task = make_task("t1");
    let input = NoProgressInput {
        previous_failure_signature: None,
        previous_promotion_decision: None,
        current_promotion_decision: PromotionDecision::AutoMergeEligible,
        before_fingerprint: Some("abc".to_string()),
        after_fingerprint: Some("def".to_string()),
        no_progress_repeat_limit: 1,
    };

    // Differing fingerprints never observe diff-unchanged — regardless of how
    // often the check runs or how low the repeat limit is.
    assert!(!should_stop_for_no_progress(&mut task, &input));
    assert!(!should_stop_for_no_progress(&mut task, &input));
    let notes = task.notes.unwrap_or_default();
    assert!(!notes.iter().any(|note| note.contains("diff-unchanged")));
}

// ---------------------------------------------------------------------------
// resolve_run_cost residuals (tests/unit/scheduler-cost.test.ts)
// ---------------------------------------------------------------------------

fn run_with_usage(usage: Option<AgentUsage>) -> AgentRunResult {
    AgentRunResult {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        usage,
    }
}

#[test]
fn run_cost_falls_back_to_estimate_for_non_finite_measured_cost() {
    let result = run_with_usage(Some(AgentUsage {
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        cost_usd: Some(f64::NAN),
    }));
    assert_eq!(resolve_run_cost(&result, 0.1), (0.1, CostSource::Estimated));
}

#[test]
fn run_cost_falls_back_to_estimate_for_tokens_without_cost() {
    let result = run_with_usage(Some(AgentUsage {
        input_tokens: Some(10.0),
        output_tokens: Some(5.0),
        total_tokens: None,
        cost_usd: None,
    }));
    assert_eq!(resolve_run_cost(&result, 0.1), (0.1, CostSource::Estimated));
}

// ---------------------------------------------------------------------------
// validation-utils (tests/unit/validation-utils.test.ts)
// ---------------------------------------------------------------------------

#[test]
fn configured_validation_names_is_empty_when_no_commands_configured() {
    assert!(get_configured_validation_names(&validation_config(None, None, None)).is_empty());
}

#[test]
fn configured_validation_names_returns_lint_when_only_lint_configured() {
    let config = validation_config(Some("eslint ."), None, None);
    assert_eq!(
        get_configured_validation_names(&config),
        vec![ValidationName::Lint]
    );
}

#[test]
fn configured_validation_names_orders_lint_test_typecheck_when_all_configured() {
    let config = validation_config(Some("eslint ."), Some("vitest run"), Some("tsc --noEmit"));
    assert_eq!(
        get_configured_validation_names(&config),
        vec![
            ValidationName::Lint,
            ValidationName::Test,
            ValidationName::Typecheck,
        ]
    );
}

#[test]
fn auto_merge_validations_false_when_no_validations_configured() {
    assert!(!has_all_required_auto_merge_validations(
        &[],
        &validation_config(None, None, None)
    ));
}

#[test]
fn auto_merge_validations_true_when_all_configured_validations_pass() {
    let config = validation_config(Some("eslint ."), Some("vitest run"), None);
    let validation = vec![
        ValidationSummary {
            name: ValidationName::Lint,
            command: "eslint .".to_string(),
            exit_code: 0,
        },
        ValidationSummary {
            name: ValidationName::Test,
            command: "vitest run".to_string(),
            exit_code: 0,
        },
    ];
    assert!(has_all_required_auto_merge_validations(
        &validation,
        &config
    ));
}

#[test]
fn auto_merge_validations_false_when_a_configured_validation_fails() {
    let config = validation_config(Some("eslint ."), Some("vitest run"), None);
    let validation = vec![
        ValidationSummary {
            name: ValidationName::Lint,
            command: "eslint .".to_string(),
            exit_code: 0,
        },
        ValidationSummary {
            name: ValidationName::Test,
            command: "vitest run".to_string(),
            exit_code: 1,
        },
    ];
    assert!(!has_all_required_auto_merge_validations(
        &validation,
        &config
    ));
}

#[test]
fn auto_merge_validations_false_when_a_configured_validation_is_missing_from_results() {
    let config = validation_config(Some("eslint ."), Some("vitest run"), None);
    let validation = vec![ValidationSummary {
        name: ValidationName::Lint,
        command: "eslint .".to_string(),
        exit_code: 0,
    }];
    assert!(!has_all_required_auto_merge_validations(
        &validation,
        &config
    ));
}
