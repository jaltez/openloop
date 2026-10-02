use openloop::core::project_policy::{
    evaluate_task_scope_policy, matches_any_glob, normalize_scope_path,
};
use openloop::core::types::*;

fn task(risk: TaskRisk, scope: Option<&[&str]>) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: "t".into(),
        title: "T".into(),
        kind: TaskKind::Feature,
        status: TaskStatus::Ready,
        risk,
        scope: scope.map(|paths| TaskScope {
            paths: paths.iter().map(|s| s.to_string()).collect(),
        }),
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
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

#[test]
fn single_star_does_not_cross_slashes() {
    // '*' compiles to [^/]*: it must not match path separators.
    assert!(!matches_any_glob("src/deep/a.rs", &["src/*.rs".into()]));
    assert!(matches_any_glob("src/a.rs", &["src/*.rs".into()]));
}

#[test]
fn double_star_crosses_slashes() {
    assert!(matches_any_glob("src/deep/a.rs", &["src/**".into()]));
    assert!(matches_any_glob("src/x/y/z.rs", &["**/*.rs".into()]));
}

#[test]
fn question_mark_matches_one_non_slash() {
    assert!(matches_any_glob("a.rs", &["?.rs".into()]));
    assert!(!matches_any_glob("ab.rs", &["?.rs".into()]));
    assert!(!matches_any_glob("a/b.rs", &["?.rs".into()]));
}

#[test]
fn normalize_strips_leading_dot_slash_and_backslashes() {
    assert_eq!(normalize_scope_path("./src/a.rs"), "src/a.rs");
    assert_eq!(normalize_scope_path("src\\a.rs"), "src/a.rs");
}

#[test]
fn no_scope_with_rules_escalates_risk() {
    let policy = {
        let mut p = openloop::core::project_policy::default_project_policy();
        p.scope.allow_globs = vec!["src/**".into()];
        p
    };
    let decision = evaluate_task_scope_policy(&task(TaskRisk::LowRisk, None), &policy, "high-risk");
    assert!(!decision.blocked);
    assert_eq!(decision.adjusted_risk, TaskRisk::HighRisk);
    assert!(decision
        .note
        .as_deref()
        .unwrap_or("")
        .contains("conservatively"));
}

#[test]
fn denied_paths_block_with_signature() {
    let mut policy = openloop::core::project_policy::default_project_policy();
    policy.scope.deny_globs = vec!["**/secrets/**".into()];
    let decision = evaluate_task_scope_policy(
        &task(TaskRisk::LowRisk, Some(&["src/secrets/x"])),
        &policy,
        "low-risk",
    );
    assert!(decision.blocked);
    assert_eq!(
        decision.failure_signature.as_deref(),
        Some("policy-scope-denied")
    );
}

#[test]
fn outside_allowlist_blocks() {
    let mut policy = openloop::core::project_policy::default_project_policy();
    policy.scope.allow_globs = vec!["src/**".into()];
    let decision = evaluate_task_scope_policy(
        &task(TaskRisk::LowRisk, Some(&["docs/x"])),
        &policy,
        "low-risk",
    );
    assert!(decision.blocked);
    assert_eq!(
        decision.failure_signature.as_deref(),
        Some("policy-scope-not-allowlisted")
    );
}

#[test]
fn high_risk_area_escalates_without_blocking() {
    let mut policy = openloop::core::project_policy::default_project_policy();
    policy.scope.high_risk_areas = vec!["infra/**".into()];
    let decision = evaluate_task_scope_policy(
        &task(TaskRisk::LowRisk, Some(&["infra/terradeform"])),
        &policy,
        "low-risk",
    );
    assert!(!decision.blocked);
    assert_eq!(decision.adjusted_risk, TaskRisk::HighRisk);
}

#[test]
fn no_rules_and_no_scope_is_passthrough() {
    let decision = evaluate_task_scope_policy(
        &task(TaskRisk::MediumRisk, None),
        &openloop::core::project_policy::default_project_policy(),
        "high-risk",
    );
    assert!(!decision.blocked);
    assert_eq!(decision.adjusted_risk, TaskRisk::MediumRisk);
    assert!(decision.note.is_none());
}
