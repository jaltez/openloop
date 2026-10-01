use openloop::core::digest::{compute_confidence, DigestConfidence};
use openloop::core::types::*;

fn artifact(validations: &[(&str, i32)]) -> PromotionArtifact {
    PromotionArtifact {
        version: 1,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        project_alias: "p".into(),
        task_id: "t".into(),
        base_branch: None,
        decision: PromotionDecision::ManualReview,
        action: PromotionAction::QueueReview,
        effective_promotion_mode: TaskPromotion::PullRequest,
        validation: validations
            .iter()
            .map(|(name, code)| ValidationSummary {
                name: ValidationName::from_str(name).unwrap(),
                command: name.to_string(),
                exit_code: *code,
            })
            .collect(),
        pi_exit_code: Some(0),
        outcome: RunOutcome::Completed,
        approval_packet_path: None,
        status: PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    }
}

fn packet(risk: TaskRisk, attempts: u32, blocking: bool) -> openloop::core::approval_packets::ApprovalPacket {
    openloop::core::approval_packets::ApprovalPacket {
        version: 1,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        task_id: "t".into(),
        title: "T".into(),
        risk,
        scope: None,
        acceptance_criteria: vec![],
        branch: None,
        diff_stat: None,
        validation: vec![],
        review_findings: if blocking {
            vec![ReviewFinding {
                rule: "x".into(),
                severity: ReviewFindingSeverity::Block,
                message: "m".into(),
                file: None,
            }]
        } else {
            vec![]
        },
        cost_usd: 0.1,
        cost_source: CostSource::Estimated,
        attempts,
        run_summary_path: None,
        spec_path: None,
    }
}

fn configured(names: &[&str]) -> Vec<ValidationName> {
    names.iter().map(|n| ValidationName::from_str(n).unwrap()).collect()
}

#[test]
fn high_confidence_requires_validations_first_attempt_low_risk() {
    let art = artifact(&[("lint", 0), ("test", 0)]);
    let pk = packet(TaskRisk::LowRisk, 1, false);
    assert_eq!(
        compute_confidence(&art, Some(&pk), &configured(&["lint", "test"])),
        DigestConfidence::High
    );
}

#[test]
fn second_attempt_or_higher_risk_caps_at_medium() {
    let art = artifact(&[("lint", 0)]);
    let configured = configured(&["lint"]);

    let pk = packet(TaskRisk::LowRisk, 2, false);
    assert_eq!(compute_confidence(&art, Some(&pk), &configured), DigestConfidence::Medium);

    let pk = packet(TaskRisk::MediumRisk, 1, false);
    assert_eq!(compute_confidence(&art, Some(&pk), &configured), DigestConfidence::Medium);
}

#[test]
fn blocking_finding_drops_to_low() {
    let art = artifact(&[("lint", 0)]);
    let pk = packet(TaskRisk::LowRisk, 1, true);
    assert_eq!(
        compute_confidence(&art, Some(&pk), &configured(&["lint"])),
        DigestConfidence::Low
    );
}

#[test]
fn missing_validation_or_missing_packet_is_low() {
    let art = artifact(&[("lint", 0)]);
    // test configured but not run -> low.
    assert_eq!(
        compute_confidence(&art, None, &configured(&["lint", "test"])),
        DigestConfidence::Low
    );
    // No packet (unverifiable provenance) -> low.
    let art_only = artifact(&[("lint", 0)]);
    assert_eq!(
        compute_confidence(&art_only, None, &configured(&["lint"])),
        DigestConfidence::Low
    );
}

#[test]
fn failing_validation_is_low() {
    let art = artifact(&[("lint", 1)]);
    let pk = packet(TaskRisk::LowRisk, 1, false);
    assert_eq!(
        compute_confidence(&art, Some(&pk), &configured(&["lint"])),
        DigestConfidence::Low
    );
}
