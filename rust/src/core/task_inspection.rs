use std::path::Path;

use super::types::*;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInspectionHistoryEntry {
    pub created_at: String,
    pub kind: &'static str,
    pub artifact_path: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInspection {
    pub task: ProjectTask,
    #[serde(rename = "latestPromotionRequest")]
    pub latest_promotion_request: Option<serde_json::Value>,
    #[serde(rename = "latestPromotionResult")]
    pub latest_promotion_result: Option<serde_json::Value>,
    pub promotion_history: Vec<TaskInspectionHistoryEntry>,
}

pub fn get_task_inspection(project_path: &Path, task_id: &str) -> Result<TaskInspection, String> {
    let task = super::task_ledger::get_task(project_path, task_id)?;
    let promotion_requests = super::promotion_queue::list_promotion_artifacts_for_task(project_path, task_id)?;
    let promotion_results = super::promotion_artifacts::list_promotion_result_artifacts(project_path, Some(task_id));

    let mut promotion_history: Vec<TaskInspectionHistoryEntry> = Vec::new();
    for item in &promotion_requests {
        promotion_history.push(TaskInspectionHistoryEntry {
            created_at: item.artifact.created_at.clone(),
            kind: "request",
            artifact_path: item.artifact_path.clone(),
            payload: serde_json::to_value(&item.artifact).unwrap_or(serde_json::Value::Null),
        });
    }
    for (artifact_path, artifact) in &promotion_results {
        promotion_history.push(TaskInspectionHistoryEntry {
            created_at: artifact.created_at.clone(),
            kind: "result",
            artifact_path: artifact_path.clone(),
            payload: serde_json::to_value(artifact).unwrap_or(serde_json::Value::Null),
        });
    }
    promotion_history.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    let latest_request = promotion_requests.first().map(|item| {
        serde_json::json!({
            "artifactPath": item.artifact_path,
            "artifact": item.artifact,
        })
    });
    let latest_result = promotion_results.first().map(|(artifact_path, artifact)| {
        serde_json::json!({
            "artifactPath": artifact_path,
            "artifact": artifact,
        })
    });

    Ok(TaskInspection {
        task,
        latest_promotion_request: latest_request,
        latest_promotion_result: latest_result,
        promotion_history,
    })
}
