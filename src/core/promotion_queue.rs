use std::path::Path;

use super::fsutil::{file_exists, read_json_file, write_json_file};
use super::git::*;
use super::now_iso;
use super::promotion_artifacts::*;
use super::task_ledger::with_task_ledger;
use super::types::*;

pub struct PromotionQueueItem {
    pub artifact_path: String,
    pub artifact: PromotionArtifact,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionDetail {
    pub artifact_path: String,
    pub artifact: PromotionArtifact,
    #[serde(rename = "resultArtifactPath")]
    pub result_artifact_path: Option<String>,
    pub result_artifact: Option<PromotionResultArtifact>,
    pub task_branch: Option<String>,
    pub current_branch: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionHistoryEntry {
    pub created_at: String,
    pub kind: &'static str,
    pub artifact_path: String,
    pub task_id: String,
    pub payload: serde_json::Value,
}

pub fn list_promotion_artifacts(project_path: &Path) -> Result<Vec<PromotionQueueItem>, String> {
    let promotions_dir = project_path.join(".openloop").join("promotions");
    if !file_exists(&promotions_dir) {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&promotions_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !entry.metadata().map(|m| m.is_file()).unwrap_or(false) || !name.ends_with(".json") {
                continue;
            }
            let artifact_path = promotions_dir.join(&name);
            let path_str = artifact_path.to_string_lossy().to_string();
            // TS parity: a corrupt promotion artifact is a hard error — it
            // must not silently vanish from the queue (and from
            // review-backpressure counting).
            let artifact: PromotionArtifact =
                read_json_file(&artifact_path, serde_json::Value::Null)
                    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))?;
            items.push(PromotionQueueItem {
                artifact_path: path_str,
                artifact,
            });
        }
    }
    items.sort_by(|a, b| b.artifact.created_at.cmp(&a.artifact.created_at));
    Ok(items)
}

pub fn list_promotion_artifacts_for_task(
    project_path: &Path,
    task_id: &str,
) -> Result<Vec<PromotionQueueItem>, String> {
    Ok(list_promotion_artifacts(project_path)?
        .into_iter()
        .filter(|item| item.artifact.task_id == task_id)
        .collect())
}

pub fn update_promotion_artifact(
    project_path: &Path,
    task_id: &str,
    status: PromotionArtifactStatus,
    note: Option<&str>,
) -> Result<PromotionQueueItem, String> {
    if status == PromotionArtifactStatus::Pending {
        return Err("Cannot update a promotion artifact back to pending.".to_string());
    }

    let items = list_promotion_artifacts(project_path)?;
    let mut matched: Option<PromotionQueueItem> = items.into_iter().find(|item| {
        item.artifact.task_id == task_id && item.artifact.status == PromotionArtifactStatus::Pending
    });
    let Some(mut matched) = matched.take() else {
        return Err(format!(
            "No pending promotion artifact found for task: {task_id}"
        ));
    };

    let note_str = note.map(|s| s.to_string());
    matched.artifact.status = status;
    matched.artifact.processed_at = Some(now_iso());
    matched.artifact.note = note_str.clone();
    write_json_file(Path::new(&matched.artifact_path), &matched.artifact)?;
    let result_str = if status == PromotionArtifactStatus::Applied {
        "applied"
    } else {
        "rejected"
    };
    let result_artifact_path = write_promotion_result(
        project_path,
        &matched,
        result_str,
        None,
        note_str.as_deref(),
    )?;
    sync_task_promotion_state(
        project_path,
        task_id,
        &matched.artifact_path,
        status,
        note_str.as_deref(),
        None,
        Some(&result_artifact_path),
    )?;
    Ok(matched)
}

pub fn apply_promotion_artifact(
    project_path: &Path,
    task_id: &str,
    note: Option<&str>,
) -> Result<PromotionQueueItem, String> {
    ensure_promotion_workspace_clean(project_path)?;
    let items = list_promotion_artifacts(project_path)?;
    let mut matched = items
        .into_iter()
        .find(|item| {
            item.artifact.task_id == task_id
                && item.artifact.status == PromotionArtifactStatus::Pending
        })
        .ok_or_else(|| format!("No pending promotion artifact found for task: {task_id}"))?;
    let task = super::task_ledger::get_task(project_path, task_id)
        .map_err(|_| format!("No task found for promotion artifact: {task_id}"))?;

    if matched.artifact.action == PromotionAction::QueueReview {
        let branch_name = prepare_review_branch(project_path, task_id)?;
        let pr_url = attempt_pr_creation(
            project_path,
            &task,
            &branch_name,
            matched.artifact.base_branch.as_deref(),
        );
        matched.artifact.status = PromotionArtifactStatus::Applied;
        matched.artifact.processed_at = Some(now_iso());
        let applied_note = match (&note, &pr_url) {
            (Some(n), _) => n.to_string(),
            (None, Some(url)) => format!("Created PR: {url}"),
            (None, None) => format!("Prepared review branch {branch_name}"),
        };
        matched.artifact.note = Some(applied_note.clone());
        write_json_file(Path::new(&matched.artifact_path), &matched.artifact)?;
        let result_artifact_path = write_promotion_result_with_pr(
            project_path,
            &matched,
            "applied",
            Some(&branch_name),
            Some(&applied_note),
            pr_url.as_deref(),
        )?;
        sync_task_promotion_state(
            project_path,
            task_id,
            &matched.artifact_path,
            PromotionArtifactStatus::Applied,
            Some(&applied_note),
            Some(&branch_name),
            Some(&result_artifact_path),
        )?;
        return Ok(matched);
    }

    if matched.artifact.action != PromotionAction::QueueAutoMerge {
        return Err(format!(
            "Promotion action not yet supported for local apply: {}",
            matched.artifact.action.as_str()
        ));
    }

    assert_auto_merge_ready(project_path, &task)?;
    let branch_name = apply_auto_merge_promotion(
        project_path,
        task_id,
        matched.artifact.base_branch.as_deref(),
    )?;
    matched.artifact.status = PromotionArtifactStatus::Applied;
    matched.artifact.processed_at = Some(now_iso());
    let applied_note = note.map(|s| s.to_string()).unwrap_or_else(|| {
        format!(
            "Merged {branch_name} into {}",
            matched
                .artifact
                .base_branch
                .clone()
                .unwrap_or_else(|| "current branch".into())
        )
    });
    matched.artifact.note = Some(applied_note.clone());
    write_json_file(Path::new(&matched.artifact_path), &matched.artifact)?;
    let result_artifact_path = write_promotion_result_with_pr(
        project_path,
        &matched,
        "applied",
        Some(&branch_name),
        Some(&applied_note),
        None,
    )?;
    sync_task_promotion_state(
        project_path,
        task_id,
        &matched.artifact_path,
        PromotionArtifactStatus::Applied,
        Some(&applied_note),
        Some(&branch_name),
        Some(&result_artifact_path),
    )?;
    Ok(matched)
}

pub struct DryRunOutcome {
    pub would_apply: bool,
    pub action: String,
    pub reason: String,
}

pub fn dry_run_promotion_apply(
    project_path: &Path,
    task_id: &str,
) -> Result<DryRunOutcome, String> {
    let git_state = get_git_working_tree_state(project_path)?;
    if git_state.dirty {
        return Ok(DryRunOutcome {
            would_apply: false,
            action: "none".to_string(),
            reason: "Git working tree is dirty.".to_string(),
        });
    }
    let items = list_promotion_artifacts(project_path)?;
    let matched = items.into_iter().find(|item| {
        item.artifact.task_id == task_id && item.artifact.status == PromotionArtifactStatus::Pending
    });
    let Some(matched) = matched else {
        return Ok(DryRunOutcome {
            would_apply: false,
            action: "none".to_string(),
            reason: format!("No pending promotion artifact found for task: {task_id}"),
        });
    };
    let task = match super::task_ledger::get_task(project_path, task_id) {
        Ok(t) => t,
        Err(_) => {
            return Ok(DryRunOutcome {
                would_apply: false,
                action: "none".to_string(),
                reason: format!("No task found for promotion artifact: {task_id}"),
            })
        }
    };

    if matched.artifact.action == PromotionAction::QueueReview {
        let config = super::project_config::load_project_config(project_path)?;
        let branch_name = format!("{}{}", config.runtime.branch_prefix, task_id);
        return Ok(DryRunOutcome {
            would_apply: true,
            action: "queue-review".to_string(),
            reason: format!("Would prepare review branch {branch_name}"),
        });
    }

    if matched.artifact.action == PromotionAction::QueueAutoMerge {
        return match assert_auto_merge_ready(project_path, &task) {
            Ok(()) => Ok(DryRunOutcome {
                would_apply: true,
                action: "queue-auto-merge".to_string(),
                reason: format!(
                    "Would auto-merge task branch into {}",
                    matched
                        .artifact
                        .base_branch
                        .clone()
                        .unwrap_or_else(|| "current branch".into())
                ),
            }),
            Err(e) => Ok(DryRunOutcome {
                would_apply: false,
                action: "queue-auto-merge".to_string(),
                reason: e,
            }),
        };
    }

    Ok(DryRunOutcome {
        would_apply: false,
        action: matched.artifact.action.as_str().to_string(),
        reason: format!(
            "Promotion action not yet supported: {}",
            matched.artifact.action.as_str()
        ),
    })
}

pub fn get_promotion_detail(project_path: &Path, task_id: &str) -> Result<PromotionDetail, String> {
    let items = list_promotion_artifacts_for_task(project_path, task_id)?;
    let matched = items
        .into_iter()
        .next()
        .ok_or_else(|| format!("No promotion artifact found for task: {task_id}"))?;

    let task = super::task_ledger::load_task_ledger(project_path)?
        .tasks
        .into_iter()
        .find(|candidate| candidate.id == task_id);
    let git_state = get_git_working_tree_state(project_path)?;
    let result_artifact_path = task
        .as_ref()
        .and_then(|t| t.last_run.as_ref())
        .and_then(|lr| lr.promotion_result_artifact_path.clone());
    let result_artifact = result_artifact_path
        .as_deref()
        .and_then(read_promotion_result_artifact);
    Ok(PromotionDetail {
        artifact_path: matched.artifact_path,
        artifact: matched.artifact,
        result_artifact_path,
        result_artifact,
        task_branch: task.as_ref().and_then(|t| t.branch.clone()),
        current_branch: git_state.current_branch,
        dirty: git_state.dirty,
    })
}

pub fn get_promotion_history(
    project_path: &Path,
    task_id: &str,
) -> Result<Vec<PromotionHistoryEntry>, String> {
    let mut history = Vec::new();

    for item in list_promotion_artifacts(project_path)? {
        if item.artifact.task_id == task_id {
            history.push(PromotionHistoryEntry {
                created_at: item.artifact.created_at.clone(),
                kind: "request",
                artifact_path: item.artifact_path,
                task_id: task_id.to_string(),
                payload: serde_json::to_value(&item.artifact).unwrap_or(serde_json::Value::Null),
            });
        }
    }

    for (artifact_path, artifact) in list_promotion_result_artifacts(project_path, Some(task_id)) {
        history.push(PromotionHistoryEntry {
            created_at: artifact.created_at.clone(),
            kind: "result",
            artifact_path,
            task_id: task_id.to_string(),
            payload: serde_json::to_value(&artifact).unwrap_or(serde_json::Value::Null),
        });
    }

    history.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(history)
}

pub fn downgrade_pending_auto_merge_promotion_to_review(
    project_path: &Path,
    task_id: &str,
    note: &str,
) -> Result<bool, String> {
    let items = list_promotion_artifacts(project_path)?;
    let matched = items.into_iter().find(|item| {
        item.artifact.task_id == task_id
            && item.artifact.status == PromotionArtifactStatus::Pending
            && item.artifact.action == PromotionAction::QueueAutoMerge
    });
    let Some(mut matched) = matched else {
        return Ok(false);
    };

    matched.artifact.decision = PromotionDecision::ManualReview;
    matched.artifact.action = PromotionAction::QueueReview;
    matched.artifact.note = Some(note.to_string());
    let artifact_path = matched.artifact_path.clone();
    write_json_file(Path::new(&artifact_path), &matched.artifact)?;

    with_task_ledger(project_path, |ledger| {
        if let Some(task) = ledger.tasks.iter_mut().find(|t| t.id == task_id) {
            let matches_artifact = task
                .last_run
                .as_ref()
                .map(|lr| lr.promotion_artifact_path.as_deref() == Some(artifact_path.as_str()))
                .unwrap_or(false);
            if matches_artifact {
                if let Some(lr) = task.last_run.as_mut() {
                    lr.promotion_decision = PromotionDecision::ManualReview;
                    lr.promotion_action = PromotionAction::QueueReview;
                }
                let mut notes = task.notes.clone().unwrap_or_default();
                notes.push(format!("Promotion artifact downgraded to review: {note}"));
                task.notes = Some(notes);
                task.updated_at = now_iso();
            }
        }
        Ok(())
    })?;

    Ok(true)
}

fn prepare_review_branch(project_path: &Path, task_id: &str) -> Result<String, String> {
    let config = super::project_config::load_project_config(project_path)?;
    let branch_name = format!("{}{}", config.runtime.branch_prefix, task_id);
    checkout_handoff_branch(project_path, &branch_name)?;
    Ok(branch_name)
}

fn apply_auto_merge_promotion(
    project_path: &Path,
    task_id: &str,
    base_branch: Option<&str>,
) -> Result<String, String> {
    let config = super::project_config::load_project_config(project_path)?;
    let branch_name = format!("{}{}", config.runtime.branch_prefix, task_id);
    let initial_state = get_git_working_tree_state(project_path)?;
    let merge_base_branch = base_branch
        .map(|s| s.to_string())
        .or_else(|| initial_state.current_branch.clone());
    let Some(merge_base_branch) = merge_base_branch else {
        return Err("Cannot determine base branch for auto-merge promotion.".to_string());
    };

    checkout_handoff_branch(project_path, &branch_name)?;
    ensure_clean_git_repo(project_path)?;

    let base_head_before_merge = get_branch_head(project_path, &merge_base_branch)?;
    let merge_base = get_merge_base(project_path, &merge_base_branch, &branch_name)?;
    if merge_base != base_head_before_merge {
        return Err(format!(
            "Base branch has drifted since task branch split from {merge_base_branch}."
        ));
    }

    checkout_branch(project_path, &merge_base_branch)?;
    merge_fast_forward(project_path, &branch_name)?;
    Ok(branch_name)
}

fn assert_auto_merge_ready(project_path: &Path, task: &ProjectTask) -> Result<(), String> {
    let Some(last_run) = task.last_run.as_ref() else {
        return Err("Task has no last run summary for auto-merge promotion.".to_string());
    };
    if task.risk != TaskRisk::LowRisk {
        return Err("Auto-merge is restricted to low-risk tasks.".to_string());
    }
    if last_run.outcome != RunOutcome::Completed {
        return Err(format!(
            "Task is not in a completed state for auto-merge: {}",
            last_run.outcome.as_str()
        ));
    }
    if last_run.promotion_decision != PromotionDecision::AutoMergeEligible {
        return Err(format!(
            "Task is not eligible for auto-merge: {}",
            last_run.promotion_decision.as_str()
        ));
    }

    let project_config = super::project_config::load_project_config(project_path)?;
    let configured = super::validation_utils::get_configured_validation_names(&project_config);
    if configured.is_empty() {
        return Err("Auto-merge requires at least one configured validation command.".to_string());
    }

    if last_run.validation.is_empty() {
        return Err("Auto-merge requires at least one successful validation result.".to_string());
    }
    if last_run.validation.iter().any(|item| item.exit_code != 0) {
        return Err("Auto-merge requires all persisted validations to pass.".to_string());
    }
    for required_name in &configured {
        if !last_run
            .validation
            .iter()
            .any(|item| &item.name == required_name && item.exit_code == 0)
        {
            return Err(format!(
                "Auto-merge requires configured validation '{}' to pass.",
                required_name.as_str()
            ));
        }
    }
    Ok(())
}

fn ensure_promotion_workspace_clean(project_path: &Path) -> Result<(), String> {
    let state = get_git_working_tree_state(project_path)?;
    if state.dirty {
        return Err("Cannot apply promotion on a dirty git tree.".to_string());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sync_task_promotion_state(
    project_path: &Path,
    task_id: &str,
    artifact_path: &str,
    status: PromotionArtifactStatus,
    note: Option<&str>,
    branch_name: Option<&str>,
    result_artifact_path: Option<&str>,
) -> Result<(), String> {
    with_task_ledger(project_path, |ledger| {
        let Some(task) = ledger
            .tasks
            .iter_mut()
            .find(|candidate| candidate.id == task_id)
        else {
            return Ok(());
        };
        let matches_artifact = task
            .last_run
            .as_ref()
            .map(|lr| lr.promotion_artifact_path.as_deref() == Some(artifact_path))
            .unwrap_or(false);
        if !matches_artifact {
            return Ok(());
        }

        let Some(lr) = task.last_run.as_mut() else {
            return Ok(());
        };
        lr.promotion_artifact_state = status;
        if let Some(rap) = result_artifact_path {
            lr.promotion_result_artifact_path = Some(rap.to_string());
        }
        if let Some(branch) = branch_name {
            task.branch = Some(branch.to_string());
        }
        if status == PromotionArtifactStatus::Applied
            && lr.promotion_action == PromotionAction::QueueAutoMerge
        {
            task.status = TaskStatus::Promoted;
            task.promoted_at = Some(now_iso());
        }
        let mut notes = task.notes.clone().unwrap_or_default();
        let note_suffix = note
            .filter(|n| !n.is_empty())
            .map(|n| format!(": {n}"))
            .unwrap_or_else(|| ".".to_string());
        notes.push(format!(
            "Promotion artifact {}{}",
            status.as_str(),
            note_suffix
        ));
        task.notes = Some(notes);
        task.updated_at = now_iso();
        Ok(())
    })
}

fn write_promotion_result(
    project_path: &Path,
    item: &PromotionQueueItem,
    result: &str,
    branch: Option<&str>,
    note: Option<&str>,
) -> Result<String, String> {
    write_promotion_result_with_pr(project_path, item, result, branch, note, None)
}

fn write_promotion_result_with_pr(
    project_path: &Path,
    item: &PromotionQueueItem,
    result: &str,
    branch: Option<&str>,
    note: Option<&str>,
    pr_url: Option<&str>,
) -> Result<String, String> {
    write_promotion_result_artifact(
        project_path,
        &PromotionResultArtifact {
            version: 1,
            created_at: now_iso(),
            project_alias: item.artifact.project_alias.clone(),
            task_id: item.artifact.task_id.clone(),
            source_promotion_artifact_path: item.artifact_path.clone(),
            source_promotion_action: item.artifact.action,
            source_promotion_decision: item.artifact.decision,
            result: result.to_string(),
            branch: branch.map(|s| s.to_string()),
            base_branch: item.artifact.base_branch.clone(),
            note: note.map(|s| s.to_string()),
            pr_url: pr_url.map(|s| s.to_string()),
            state: None,
            checks: None,
        },
    )
}

// W6: Attempt to create a PR using the configured or default command.
fn attempt_pr_creation(
    project_path: &Path,
    task: &ProjectTask,
    branch_name: &str,
    base_branch: Option<&str>,
) -> Option<String> {
    let result = (|| -> Result<Option<String>, String> {
        let config = super::project_config::load_project_config(project_path)?;
        let pr_cmd = config
            .runtime
            .pr_command
            .clone()
            .unwrap_or_else(|| "gh pr create".to_string());
        let mut args: Vec<String> = vec!["--head".into(), branch_name.to_string()];
        if let Some(base) = base_branch {
            args.push("--base".into());
            args.push(base.to_string());
        }
        args.push("--title".into());
        args.push(task.title.clone());
        args.push("--body".into());
        args.push(format!("Automated PR for OpenLoop task {}", task.id));
        let quoted: Vec<String> = args
            .iter()
            .map(|a| serde_json::to_string(a).unwrap_or_default())
            .collect();
        let shell_command = format!("{pr_cmd} {}", quoted.join(" "));
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&shell_command)
            .current_dir(project_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| e.to_string())?;
        // Mirror TS execFile semantics: a non-zero exit rejects (caught as
        // None) — never adopt failing stdout as a PR URL.
        if !output.status.success() {
            return Ok(None);
        }
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let url = stdout
            .trim()
            .lines()
            .last()
            .map(|l| l.trim().to_string())
            .unwrap_or_default();
        if url.is_empty() {
            Ok(None)
        } else {
            Ok(Some(url))
        }
    })();
    // PR creation is best-effort; failure should not block promotion.
    result.unwrap_or(None)
}
