//! Ports the promotion-mode matrix from the deleted TS suites:
//! `tests/unit/scheduler-promotion.test.ts` (resolveEffectivePromotionMode)
//! and the promotion-mode cases of `tests/integration/validation-flow.test.ts`.
//!
//! These tests assert directly against
//! `openloop::core::scheduler::resolve_effective_promotion_mode`, which is a
//! public function returning `Result<TaskPromotion, String>`.

use std::collections::BTreeMap;

use openloop::core::scheduler::resolve_effective_promotion_mode;
use openloop::core::types::{
    PolicyPromotionModes, PolicyScope, ProjectPolicy, ProjectTask, RiskClassPolicy,
    SelfHealingPolicy, TaskKind, TaskPromotion, TaskRisk, TaskSource, TaskStatus,
};

fn make_task(id: &str, risk: TaskRisk, promotion: TaskPromotion) -> ProjectTask {
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status: TaskStatus::Done,
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
        promotion,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    }
}

fn make_risk_classes(low_auto_merge_allowed: bool) -> BTreeMap<String, RiskClassPolicy> {
    BTreeMap::from([
        (
            "low-risk".to_string(),
            RiskClassPolicy {
                auto_merge_allowed: low_auto_merge_allowed,
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
    ])
}

/// Mirrors the TS `makeProjectPolicy` defaults: low-risk auto-merge,
/// medium/high pull-request, low-risk class allows auto-merge.
fn make_policy() -> ProjectPolicy {
    ProjectPolicy {
        version: 1,
        scope: PolicyScope {
            allow_globs: vec!["src/**".into()],
            deny_globs: vec![],
            high_risk_areas: vec![],
        },
        risk_classes: make_risk_classes(true),
        self_healing: SelfHealingPolicy {
            enabled: true,
            allowed_task_kinds: vec!["lint-fix".into(), "type-fix".into()],
        },
        promotion: PolicyPromotionModes {
            low_risk_mode: TaskPromotion::AutoMerge,
            medium_risk_mode: TaskPromotion::PullRequest,
            high_risk_mode: TaskPromotion::PullRequest,
        },
    }
}

#[test]
fn policy_low_risk_mode_pull_request_forces_pull_request() {
    // TS validation-flow: "runProjectIteration downgrades promotion when
    // policy disallows auto-merge" — lowRiskMode: pull-request downgrades a
    // low-risk auto-merge task to pull-request regardless of the
    // require-policy flag.
    let task = make_task("t1", TaskRisk::LowRisk, TaskPromotion::AutoMerge);
    let mut policy = make_policy();
    policy.promotion.low_risk_mode = TaskPromotion::PullRequest;

    let mode = resolve_effective_promotion_mode(&task, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::PullRequest);

    let mode = resolve_effective_promotion_mode(&task, &policy, false).unwrap();
    assert_eq!(mode, TaskPromotion::PullRequest);
}

#[test]
fn require_policy_disallowing_auto_merge_forces_pull_request() {
    // TS unit: "returns pull-request when requirePolicyForAutoMerge is true
    // and autoMergeAllowed is false".
    let task = make_task("t1", TaskRisk::LowRisk, TaskPromotion::AutoMerge);
    let mut policy = make_policy();
    policy.risk_classes = make_risk_classes(false);

    let mode = resolve_effective_promotion_mode(&task, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::PullRequest);

    // Without requirePolicyForAutoMerge the risk class veto is not consulted
    // (TS parity: the guard is `requirePolicyForAutoMerge && ...`).
    let mode = resolve_effective_promotion_mode(&task, &policy, false).unwrap();
    assert_eq!(mode, TaskPromotion::AutoMerge);
}

#[test]
fn medium_and_high_risk_map_to_pull_request() {
    // TS unit: "returns pull-request for medium-risk tasks" /
    // "returns pull-request for high-risk tasks".
    let policy = make_policy();

    let medium = make_task("t1", TaskRisk::MediumRisk, TaskPromotion::AutoMerge);
    let mode = resolve_effective_promotion_mode(&medium, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::PullRequest);

    let high = make_task("t2", TaskRisk::HighRisk, TaskPromotion::AutoMerge);
    let mode = resolve_effective_promotion_mode(&high, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::PullRequest);
}

#[test]
fn low_risk_with_auto_merge_allowed_yields_auto_merge() {
    // TS unit: "returns auto-merge for low-risk task when policy allows it"
    // (requirePolicyForAutoMerge = true).
    let task = make_task("t1", TaskRisk::LowRisk, TaskPromotion::AutoMerge);
    let policy = make_policy();

    let mode = resolve_effective_promotion_mode(&task, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::AutoMerge);

    let mode = resolve_effective_promotion_mode(&task, &policy, false).unwrap();
    assert_eq!(mode, TaskPromotion::AutoMerge);
}

#[test]
fn manual_only_wins_over_everything() {
    let mut policy = make_policy();

    // TS unit: "returns manual-only when task promotion is manual-only" —
    // even against a fully permissive auto-merge policy.
    let task = make_task("t1", TaskRisk::LowRisk, TaskPromotion::ManualOnly);
    let mode = resolve_effective_promotion_mode(&task, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::ManualOnly);

    // Task manual-only outranks a policy that would force pull-request.
    policy.promotion.low_risk_mode = TaskPromotion::PullRequest;
    let mode = resolve_effective_promotion_mode(&task, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::ManualOnly);

    // TS unit: "returns manual-only when policy overrides to manual-only" —
    // a manual-only policy mode outranks the task's auto-merge request.
    policy.promotion.low_risk_mode = TaskPromotion::ManualOnly;
    let task = make_task("t2", TaskRisk::LowRisk, TaskPromotion::AutoMerge);
    let mode = resolve_effective_promotion_mode(&task, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::ManualOnly);
}

#[test]
fn missing_risk_class_with_require_policy_is_hard_error() {
    // TS parity: indexing a missing risk class throws (a hard error in Rust
    // via Err) instead of silently weakening promotion gating.
    let task = make_task("t1", TaskRisk::LowRisk, TaskPromotion::AutoMerge);
    let mut policy = make_policy();
    policy.risk_classes.remove("low-risk");

    let err = resolve_effective_promotion_mode(&task, &policy, true).unwrap_err();
    assert!(
        err.contains("low-risk"),
        "error should name the missing risk class: {err}"
    );

    // Medium/high risk short-circuit to pull-request before the risk-class
    // lookup, so a missing medium-risk class is not an error.
    let medium = make_task("t2", TaskRisk::MediumRisk, TaskPromotion::AutoMerge);
    policy.risk_classes.remove("medium-risk");
    let mode = resolve_effective_promotion_mode(&medium, &policy, true).unwrap();
    assert_eq!(mode, TaskPromotion::PullRequest);
}
