use std::path::Path;

use serde::{Deserialize, Serialize};

use super::fsutil::{ensure_dir, file_exists, read_json_file, write_json_file};
use super::git::DiffStatEntry;
use super::now_iso;
use super::types::*;

/// Human-approval packet: everything an approver needs to judge a pending
/// promotion (diff stat, validation, review findings, cost, provenance) in one
/// deterministic JSON artifact at `.openloop/approvals/<taskId>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalPacket {
    pub version: i32,
    pub created_at: String,
    pub task_id: String,
    pub title: String,
    pub risk: TaskRisk,
    pub scope: Option<TaskScope>,
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub branch: Option<String>,
    pub diff_stat: Option<Vec<DiffStatEntry>>,
    pub validation: Vec<ValidationSummary>,
    pub review_findings: Vec<ReviewFinding>,
    pub cost_usd: f64,
    pub cost_source: CostSource,
    pub attempts: u32,
    #[serde(default)]
    pub run_summary_path: Option<String>,
    #[serde(default)]
    pub spec_path: Option<String>,
}

pub struct ApprovalPacketData {
    pub diff_stat: Option<Vec<DiffStatEntry>>,
    pub validation: Vec<ValidationSummary>,
    pub review_findings: Vec<ReviewFinding>,
    pub cost_usd: f64,
    pub cost_source: CostSource,
    pub run_summary_path: Option<String>,
    pub spec_path: Option<String>,
}

pub fn approval_packet_path(control_plane_path: &Path, task_id: &str) -> std::path::PathBuf {
    control_plane_path
        .join(".openloop")
        .join("approvals")
        .join(format!("{task_id}.json"))
}

pub fn write_approval_packet(
    control_plane_path: &Path,
    task: &ProjectTask,
    data: &ApprovalPacketData,
) -> Result<String, String> {
    let file_path = approval_packet_path(control_plane_path, &task.id);
    if let Some(parent) = file_path.parent() {
        ensure_dir(parent).map_err(|e| e.to_string())?;
    }
    let packet = ApprovalPacket {
        version: 1,
        created_at: now_iso(),
        task_id: task.id.clone(),
        title: task.title.clone(),
        risk: task.risk,
        scope: task.scope.clone(),
        acceptance_criteria: task.acceptance_criteria.clone(),
        branch: task.branch.clone(),
        diff_stat: data.diff_stat.clone(),
        validation: data.validation.clone(),
        review_findings: data.review_findings.clone(),
        cost_usd: data.cost_usd,
        cost_source: data.cost_source,
        attempts: task.attempts,
        run_summary_path: data.run_summary_path.clone(),
        spec_path: data.spec_path.clone(),
    };
    write_json_file(&file_path, &packet)?;
    Ok(file_path.to_string_lossy().to_string())
}

pub fn read_approval_packet(
    control_plane_path: &Path,
    task_id: &str,
) -> Result<Option<ApprovalPacket>, String> {
    let file_path = approval_packet_path(control_plane_path, task_id);
    if !file_exists(&file_path) {
        return Ok(None);
    }
    // TS parity: corrupt approval JSON is a hard error, never a silent miss.
    read_json_file(&file_path, serde_json::Value::Null)
        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
        .map(Some)
}

pub fn list_approval_packets(
    control_plane_path: &Path,
) -> Result<Vec<(String, ApprovalPacket)>, String> {
    let approvals_dir = control_plane_path.join(".openloop").join("approvals");
    if !file_exists(&approvals_dir) {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let entries = std::fs::read_dir(&approvals_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !entry.metadata().map(|m| m.is_file()).unwrap_or(false) || !name.ends_with(".json") {
            continue;
        }
        let packet_path = approvals_dir.join(&name);
        let packet: ApprovalPacket = read_json_file(&packet_path, serde_json::Value::Null)
            .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))?;
        items.push((packet_path.to_string_lossy().to_string(), packet));
    }
    items.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at));
    Ok(items)
}
