use std::path::Path;

use super::fsutil::{ensure_dir, file_exists, write_json_file};
use super::types::*;

pub fn write_promotion_artifact(
    project_path: &Path,
    artifact: &PromotionArtifact,
) -> Result<String, String> {
    let promotions_dir = project_path.join(".openloop").join("promotions");
    ensure_dir(&promotions_dir).map_err(|e| e.to_string())?;
    let timestamp = artifact.created_at.replace([':', '.'], "-");
    let file_path = promotions_dir.join(format!("{timestamp}-{}.json", artifact.task_id));
    write_json_file(&file_path, artifact)?;
    Ok(file_path.to_string_lossy().to_string())
}

pub fn write_promotion_result_artifact(
    project_path: &Path,
    artifact: &PromotionResultArtifact,
) -> Result<String, String> {
    let results_dir = project_path.join(".openloop").join("promotion-results");
    ensure_dir(&results_dir).map_err(|e| e.to_string())?;
    let timestamp = artifact.created_at.replace([':', '.'], "-");
    let file_path = results_dir.join(format!("{timestamp}-{}.json", artifact.task_id));
    write_json_file(&file_path, artifact)?;
    Ok(file_path.to_string_lossy().to_string())
}

pub fn read_promotion_result_artifact(
    file_path: &str,
) -> Result<Option<PromotionResultArtifact>, String> {
    let path = Path::new(file_path);
    if !file_exists(path) {
        return Ok(None);
    }
    // TS parity: a corrupt artifact is a hard error, never silently skipped
    // (it would otherwise hide data from history/refresh/detail flows).
    super::fsutil::read_json_file(path, serde_json::Value::Null)
        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
        .map(Some)
}

pub fn list_promotion_result_artifacts(
    project_path: &Path,
    task_id: Option<&str>,
) -> Result<Vec<(String, PromotionResultArtifact)>, String> {
    let results_dir = project_path.join(".openloop").join("promotion-results");
    if !file_exists(&results_dir) {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let entries = std::fs::read_dir(&results_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !entry.metadata().map(|m| m.is_file()).unwrap_or(false) || !name.ends_with(".json") {
            continue;
        }
        let artifact_path = results_dir.join(&name);
        let path_str = artifact_path.to_string_lossy().to_string();
        let Some(artifact) = read_promotion_result_artifact(&path_str)? else {
            continue;
        };
        if let Some(task_id) = task_id {
            if artifact.task_id != task_id {
                continue;
            }
        }
        items.push((path_str, artifact));
    }
    items.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at));
    Ok(items)
}
