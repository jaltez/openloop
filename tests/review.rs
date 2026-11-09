use openloop::core::review::{check_scope_drift, check_secrets, extract_changed_files};
use openloop::core::types::*;

fn task_with_scope(paths: &[&str]) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: "t1".into(),
        title: "T".into(),
        kind: TaskKind::Feature,
        status: TaskStatus::Ready,
        risk: TaskRisk::LowRisk,
        scope: (!paths.is_empty()).then_some(TaskScope {
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

fn default_policy() -> ProjectPolicy {
    openloop::core::project_policy::default_project_policy()
}

#[test]
fn detects_aws_keys_in_added_lines_only() {
    let diff = "+AKIAIOSFODNN7EXAMPLE\n-context AKIAIOSFODNN7EXAMPLE\n";
    let findings = check_secrets(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, "secret-detection:aws-access-key-id");
    assert_eq!(findings[0].severity, ReviewFindingSeverity::Block);
}

#[test]
fn detects_github_tokens() {
    let token = "ghp_".to_string() + &"A".repeat(40);
    let findings = check_secrets(&format!("+{token}\n"));
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, "secret-detection:github-token");
}

#[test]
fn detects_private_key_blocks() {
    for variant in ["RSA ", "EC ", "OPENSSH ", "DSA ", ""] {
        let diff = format!("+-----BEGIN {variant}PRIVATE KEY-----\n+data\n");
        let findings = check_secrets(&diff);
        assert!(
            findings
                .iter()
                .any(|f| f.rule == "secret-detection:private-key-block"),
            "failed for variant '{variant}'"
        );
    }
}

#[test]
fn detects_generic_secret_assignment() {
    let diff = format!("+api_key = \"{}\"\n", "A".repeat(40));
    let findings = check_secrets(&diff);
    assert!(findings.iter().any(|f| f.rule.contains("generic-secret")));
}

#[test]
fn clean_diff_produces_no_secret_findings() {
    let findings = check_secrets("+const x = 1;\n+console.log('hi');\n");
    assert!(findings.is_empty());
}

#[test]
fn scope_drift_flags_files_outside_scope() {
    let task = task_with_scope(&["src/**"]);
    let findings = check_scope_drift(
        &["src/a.ts".into(), "docs/b.md".into()],
        &task,
        &default_policy(),
    );
    let drift: Vec<&ReviewFinding> = findings
        .iter()
        .filter(|f| f.rule == "scope-drift")
        .collect();
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].file.as_deref(), Some("docs/b.md"));
    assert_eq!(drift[0].severity, ReviewFindingSeverity::Warn);
}

#[test]
fn scope_drift_flags_denied_globs_as_block() {
    let mut policy = default_policy();
    policy.scope.deny_globs = vec!["secrets/**".into()];
    let task = task_with_scope(&[]);
    let findings = check_scope_drift(&["secrets/env".into()], &task, &policy);
    assert!(findings
        .iter()
        .any(|f| f.rule == "deny-glob-violation" && f.severity == ReviewFindingSeverity::Block));
}

#[test]
fn extract_changed_files_parses_headers_deduped() {
    let patch = "diff --git a/src/x.rs b/src/x.rs\n--- a\n+++ b\ndiff --git a/y b/y\n";
    assert_eq!(
        extract_changed_files(patch),
        vec!["src/x.rs".to_string(), "y".to_string()]
    );
}

#[test]
fn detects_slack_google_openai_and_jwt_tokens() {
    let cases: Vec<(&str, &str)> = vec![
        (
            "+token = \"xoxb-111AAA222BBB333CCC444DDD\"\n",
            "secret-detection:slack-token",
        ),
        (
            "+key = \"AIzaSyA1234567890abcdefghijklmnopqrstuv\"\n",
            "secret-detection:google-api-key",
        ),
        (
            "+api_key = \"sk-Ab1Cd2Ef3Gh4Ij5Kl6Mn7Op8Qr\"\n",
            "secret-detection:openai-api-key",
        ),
        (
            "+auth = \"eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5Nr_XgL0n3I9PlFUP0THsR8U\"\n",
            "secret-detection:jwt",
        ),
    ];
    for (diff, rule) in cases {
        let findings = check_secrets(diff);
        assert!(
            findings.iter().any(|f| f.rule == rule),
            "{rule} must be detected: {:?}",
            findings.iter().map(|f| f.rule.clone()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn entropy_detector_flags_random_tokens_as_warn_only() {
    // High-entropy run with no known prefix: heuristic coverage.
    let token = "Zj3kQ9vP2nR8wX1mL5sT7bY4cH6dG0fA";
    let findings = check_secrets(&format!("+{token}\n"));
    let entropy = findings
        .iter()
        .find(|f| f.rule == "secret-detection:high-entropy-token")
        .expect("high-entropy token must be flagged");
    assert_eq!(entropy.severity, ReviewFindingSeverity::Warn);

    // Ordinary prose does not trip it.
    let clean = check_secrets("+this is a normal sentence with regular words and no secrets\n");
    assert!(
        !clean.iter().any(|f| f.rule.contains("entropy")),
        "prose must not be flagged: {clean:?}"
    );
}
