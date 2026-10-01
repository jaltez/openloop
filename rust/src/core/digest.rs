use chrono::TimeZone;
use std::path::Path;

use super::approval_packets::{read_approval_packet, ApprovalPacket};
use super::now_iso;
use super::types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DigestConfidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetDigestReviewQueueEntry {
    pub task_id: String,
    pub title: String,
    pub risk: String,
    pub confidence: DigestConfidence,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetDigestProject {
    pub alias: String,
    pub completed: usize,
    pub failed: usize,
    pub blocked: usize,
    pub promoted: usize,
    pub spend_usd: SpendSplit,
    pub review_queue: Vec<FleetDigestReviewQueueEntry>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SpendSplit {
    pub measured: f64,
    pub estimated: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetDigest {
    pub generated_at: String,
    pub since: String,
    pub projects: Vec<FleetDigestProject>,
}

/// Deterministic confidence for a pending promotion:
/// - high: all configured validations passed AND no block-severity review
///   findings AND first attempt AND low risk.
/// - medium: validations passed and no blocking findings.
/// - low: anything else (including unverifiable provenance).
pub fn compute_confidence(
    artifact: &PromotionArtifact,
    packet: Option<&ApprovalPacket>,
    configured_validation_names: &[ValidationName],
) -> DigestConfidence {
    let ran: Vec<&ValidationName> = artifact
        .validation
        .iter()
        .filter(|item| item.exit_code == 0)
        .map(|item| &item.name)
        .collect();
    let validations_passed = !configured_validation_names.is_empty()
        && configured_validation_names
            .iter()
            .all(|name| ran.contains(&name));

    let has_blocking_finding = packet
        .map(|p| p.review_findings.iter().any(|f| f.severity == ReviewFindingSeverity::Block))
        .unwrap_or(true);
    let attempts = packet.map(|p| p.attempts);
    let risk = packet.map(|p| p.risk);

    if validations_passed && !has_blocking_finding {
        if attempts == Some(1) && risk == Some(TaskRisk::LowRisk) {
            return DigestConfidence::High;
        }
        return DigestConfidence::Medium;
    }
    DigestConfidence::Low
}

struct RunSummaryCost {
    cost_usd: f64,
    cost_source: CostSource,
    completed_at: String,
}

/// Parse cost lines from a run summary written by write_runSummary. The format
/// is owned by this codebase (`- costUsd: <n>` / `- costSource: <s>` /
/// `- createdAt: <iso>`).
fn parse_run_summary_cost(content: &str) -> Option<RunSummaryCost> {
    let mut cost_usd = None;
    let mut cost_source = None;
    let mut completed_at = None;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("- costUsd: ") {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
                cost_usd = rest.parse::<f64>().ok();
            }
        } else if let Some(rest) = line.strip_prefix("- costSource: ") {
            if rest == "measured" || rest == "estimated" {
                cost_source = Some(if rest == "measured" {
                    CostSource::Measured
                } else {
                    CostSource::Estimated
                });
            }
        } else if let Some(rest) = line.strip_prefix("- createdAt: ") {
            completed_at = Some(rest.to_string());
        }
    }
    Some(RunSummaryCost {
        cost_usd: cost_usd?,
        cost_source: cost_source?,
        completed_at: completed_at.unwrap_or_else(super::epoch_iso),
    })
}

pub fn build_digest(since_ms: Option<i64>, app_home_override: Option<&str>) -> Result<FleetDigest, String> {
    let since_ms = since_ms.unwrap_or(24 * 60 * 60 * 1000);
    let since = chrono::Utc
        .timestamp_millis_opt(super::now_millis() - since_ms)
        .single()
        .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default();
    let projects = super::project_registry::list_projects(app_home_override)?;

    let mut entries = Vec::new();
    for project in projects {
        match build_project_digest(&project.alias, Path::new(&project.path), &since) {
            Ok(entry) => entries.push(entry),
            Err(_) => {
                // A single unreadable project must not break the fleet digest.
                entries.push(FleetDigestProject {
                    alias: project.alias.clone(),
                    completed: 0,
                    failed: 0,
                    blocked: 0,
                    promoted: 0,
                    spend_usd: SpendSplit::default(),
                    review_queue: Vec::new(),
                });
            }
        }
    }
    entries.sort_by(|a, b| a.alias.cmp(&b.alias));

    Ok(FleetDigest {
        generated_at: now_iso(),
        since,
        projects: entries,
    })
}

fn build_project_digest(alias: &str, project_path: &Path, since: &str) -> Result<FleetDigestProject, String> {
    let ledger = super::task_ledger::load_task_ledger(project_path)?;
    let project_config = super::project_config::load_project_config(project_path).ok();
    let configured_validation_names = project_config
        .as_ref()
        .map(super::validation_utils::get_configured_validation_names)
        .unwrap_or_default();

    let completed = ledger
        .tasks
        .iter()
        .filter(|t| {
            (t.status == TaskStatus::Done || t.status == TaskStatus::Promoted)
                && t.last_run
                    .as_ref()
                    .map(|lr| lr.completed_at.as_str() >= since)
                    .unwrap_or_else(|| t.updated_at.as_str() >= since)
        })
        .count();
    let failed = ledger
        .tasks
        .iter()
        .filter(|t| {
            t.status == TaskStatus::Failed
                && t.last_run
                    .as_ref()
                    .map(|lr| lr.completed_at.as_str() >= since)
                    .unwrap_or_else(|| t.updated_at.as_str() >= since)
        })
        .count();
    let blocked = ledger
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Blocked && t.updated_at.as_str() >= since)
        .count();
    let promoted = ledger
        .tasks
        .iter()
        .filter(|t| {
            t.status == TaskStatus::Promoted
                && t.promoted_at
                    .as_ref()
                    .map(|p| p.as_str() >= since)
                    .unwrap_or_else(|| t.updated_at.as_str() >= since)
        })
        .count();

    let mut spend_usd = SpendSplit::default();
    let runs_dir = project_path.join(".openloop").join("runs");
    if let Ok(entries) = std::fs::read_dir(&runs_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".md") {
                continue;
            }
            let Some(content) = std::fs::read_to_string(entry.path()).ok() else {
                continue;
            };
            let Some(cost) = parse_run_summary_cost(&content) else {
                continue;
            };
            if cost.completed_at.as_str() < since {
                continue;
            }
            let bucket = match cost.cost_source {
                CostSource::Measured => &mut spend_usd.measured,
                CostSource::Estimated => &mut spend_usd.estimated,
            };
            *bucket = ((*bucket + cost.cost_usd) * 1_000_000.0).round() / 1_000_000.0;
        }
    }

    let mut review_queue = Vec::new();
    let promotions = super::promotion_queue::list_promotion_artifacts(project_path)?;
    for item in &promotions {
        if item.artifact.status != PromotionArtifactStatus::Pending {
            continue;
        }
        let packet = read_approval_packet(project_path, &item.artifact.task_id);
        let task = ledger.tasks.iter().find(|candidate| candidate.id == item.artifact.task_id);
        review_queue.push(FleetDigestReviewQueueEntry {
            task_id: item.artifact.task_id.clone(),
            title: packet
                .as_ref()
                .map(|p| p.title.clone())
                .or_else(|| task.map(|t| t.title.clone()))
                .unwrap_or_else(|| item.artifact.task_id.clone()),
            risk: packet
                .as_ref()
                .map(|p| p.risk.as_str().to_string())
                .or_else(|| task.map(|t| t.risk.as_str().to_string()))
                .unwrap_or_else(|| "unknown".to_string()),
            confidence: compute_confidence(&item.artifact, packet.as_ref(), &configured_validation_names),
        });
    }

    Ok(FleetDigestProject {
        alias: alias.to_string(),
        completed,
        failed,
        blocked,
        promoted,
        spend_usd,
        review_queue,
    })
}
