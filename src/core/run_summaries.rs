use std::path::Path;

use super::fsutil::ensure_dir;
use super::now_iso;
use super::types::SchedulerResult;

fn opt_str(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "none".to_string())
}

fn fmt_f64(v: f64) -> String {
    // JS numbers print without trailing zeros; f64 Display matches closely.
    format!("{v}")
}

pub fn write_run_summary(
    project_path: &Path,
    summary: &SchedulerResult,
    file_path: Option<&Path>,
) -> Result<String, String> {
    let runs_dir = project_path.join(".openloop").join("runs");
    ensure_dir(&runs_dir).map_err(|e| e.to_string())?;
    let file_path = match file_path {
        Some(p) => p.to_path_buf(),
        None => {
            let timestamp = now_iso().replace([':', '.'], "-");
            let task_id = summary
                .task_id
                .clone()
                .unwrap_or_else(|| "idle".to_string());
            runs_dir.join(format!("{timestamp}-{task_id}.md"))
        }
    };

    let validation = &summary.validation;
    let validation_lines: Vec<String> = if !validation.is_empty() {
        validation
            .iter()
            .map(|item| {
                format!(
                    "- {}: {} => {}",
                    item.name.as_str(),
                    item.command,
                    item.exit_code
                )
            })
            .collect()
    } else {
        vec!["- none".to_string()]
    };

    let mut content = String::new();
    content.push_str("# Run Summary\n\n");
    content.push_str(&format!("- project: {}\n", summary.project_alias));
    content.push_str(&format!("- task: {}\n", opt_str(&summary.task_id)));
    content.push_str(&format!("- mode: {}\n", summary.mode));
    content.push_str(&format!(
        "- role: {}\n",
        summary
            .role
            .map(|r| r.as_str().to_string())
            .unwrap_or_else(|| "none".into())
    ));
    content.push_str(&format!("- reason: {}\n", summary.reason));
    content.push_str(&format!("- model: {}\n", opt_str(&summary.model)));
    content.push_str(&format!(
        "- exitCode: {}\n",
        summary
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "none".into())
    ));
    content.push_str(&format!(
        "- taskStatus: {}\n",
        summary
            .task_status
            .map(|s| s.as_str().to_string())
            .unwrap_or_else(|| "none".into())
    ));
    content.push_str(&format!(
        "- promotedAt: {}\n",
        opt_str(&summary.promoted_at)
    ));
    content.push_str(&format!(
        "- promotionDecision: {}\n",
        summary.promotion_decision.as_str()
    ));
    content.push_str(&format!(
        "- promotionAction: {}\n",
        summary.promotion_action.as_str()
    ));
    content.push_str(&format!(
        "- promotionArtifactPath: {}\n",
        opt_str(&summary.promotion_artifact_path)
    ));
    content.push_str(&format!(
        "- promotionResultArtifactPath: {}\n",
        opt_str(&summary.promotion_result_artifact_path)
    ));
    content.push_str(&format!("- stoppedBy: {}\n", summary.stopped_by.as_str()));
    content.push_str(&format!(
        "- attemptNumber: {}\n",
        summary
            .attempt_number
            .map(|n| n.to_string())
            .unwrap_or_else(|| "none".into())
    ));
    content.push_str(&format!(
        "- dirtyTreeDetected: {}\n",
        summary.dirty_tree_detected
    ));
    content.push_str(&format!(
        "- budgetSnapshotUsd: {}\n",
        summary
            .budget_snapshot_usd
            .map(fmt_f64)
            .unwrap_or_else(|| "none".into())
    ));
    content.push_str(&format!("- validationCount: {}\n", validation.len()));
    content.push_str(&format!(
        "- approvalPacketPath: {}\n",
        opt_str(&summary.approval_packet_path)
    ));
    content.push_str(&format!(
        "- costUsd: {}\n",
        summary
            .cost_usd
            .map(fmt_f64)
            .unwrap_or_else(|| "none".into())
    ));
    content.push_str(&format!(
        "- costSource: {}\n",
        summary
            .cost_source
            .map(|c| c.as_str().to_string())
            .unwrap_or_else(|| "none".into())
    ));
    content.push_str(&format!("- createdAt: {}\n", now_iso()));
    content.push_str("\n## Validation\n");
    for line in &validation_lines {
        content.push_str(line);
        content.push('\n');
    }
    content.push('\n');
    if let Some(findings) = &summary.review_findings {
        if !findings.is_empty() {
            content.push_str("## Review\n");
            for f in findings {
                let file_suffix = f
                    .file
                    .as_deref()
                    .map(|file| format!(" ({file})"))
                    .unwrap_or_default();
                content.push_str(&format!(
                    "- [{}] {}: {}{}\n",
                    f.severity.as_str(),
                    f.rule,
                    f.message,
                    file_suffix
                ));
            }
        }
    }
    content.push_str("\n## Prompt\n");
    content.push_str(summary.prompt.as_deref().unwrap_or("_none_"));

    let payload = format!("{content}\n");
    super::fsutil::write_atomic(&file_path, payload.as_bytes())?;
    Ok(file_path.to_string_lossy().to_string())
}
