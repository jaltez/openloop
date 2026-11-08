use std::path::Path;

use super::now_iso;
use super::types::*;

/// One PR refresh outcome, for the daemon loop and the CLI to report.
#[derive(Debug, Clone)]
pub struct PrRefreshOutcome {
    pub project_alias: String,
    pub task_id: String,
    pub pr_url: String,
    pub state: Option<String>,
    pub failed_checks: Vec<String>,
    pub merged: bool,
    pub closed: bool,
}

/// Refresh the latest PR result artifact for one task via `gh pr view`,
/// feeding the live state back into the ledger: MERGED promotes the task,
/// CLOSED rejects the promotion artifact, failing checks append a note and
/// fire the `promotion-ci-failed` hooks.
pub fn refresh_task_pr(
    config: &GlobalConfig,
    project_alias: &str,
    project_path: &Path,
    task_id: &str,
    gh_timeout_ms: u64,
) -> Result<PrRefreshOutcome, String> {
    let results =
        super::promotion_artifacts::list_promotion_result_artifacts(project_path, Some(task_id))?;
    let with_pr = results
        .iter()
        .find(|(_, artifact)| artifact.pr_url.is_some());
    let Some((_, with_pr)) = with_pr else {
        return Err(format!(
            "No PR promotion result found for task {task_id} in {project_alias}."
        ));
    };
    let pr_url = with_pr.pr_url.clone().unwrap();
    refresh_pr(
        config,
        project_alias,
        project_path,
        task_id,
        &pr_url,
        with_pr,
        gh_timeout_ms,
    )
}

/// Poll every pending PR promotion under the given projects (latest result
/// artifact per task, skipping ones already known MERGED/CLOSED). Returns
/// the refreshed outcomes plus per-PR error strings for the caller to log —
/// a missing `gh` or a flaky API must never fail the daemon tick.
pub fn refresh_pending_prs(
    config: &GlobalConfig,
    projects: &[LinkedProject],
    gh_timeout_ms: u64,
) -> (Vec<PrRefreshOutcome>, Vec<String>) {
    let mut outcomes = Vec::new();
    let mut errors = Vec::new();
    for project in projects {
        if !project.initialized {
            continue;
        }
        let project_path = Path::new(&project.path);
        let Ok(results) =
            super::promotion_artifacts::list_promotion_result_artifacts(project_path, None)
        else {
            continue;
        };
        // Newest-first: only the latest result per task carries the live PR.
        let mut seen_tasks = std::collections::HashSet::new();
        for artifact in results.into_iter().map(|(_, artifact)| artifact) {
            if !seen_tasks.insert(artifact.task_id.clone()) {
                continue;
            }
            let Some(pr_url) = artifact.pr_url.clone() else {
                continue;
            };
            if matches!(artifact.state.as_deref(), Some("MERGED") | Some("CLOSED")) {
                continue;
            }
            match refresh_pr(
                config,
                &project.alias,
                project_path,
                &artifact.task_id,
                &pr_url,
                &artifact,
                gh_timeout_ms,
            ) {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => errors.push(format!("{}: {}", project.alias, error)),
            }
        }
    }
    (outcomes, errors)
}

/// Run `gh pr view` for one PR and apply the state transitions. Shared by the
/// CLI (`promotion refresh`) and the daemon poll loop.
fn refresh_pr(
    config: &GlobalConfig,
    project_alias: &str,
    project_path: &Path,
    task_id: &str,
    pr_url: &str,
    latest: &PromotionResultArtifact,
    gh_timeout_ms: u64,
) -> Result<PrRefreshOutcome, String> {
    let raw = super::providers::spawn_and_wait(
        "gh",
        &[
            "pr".to_string(),
            "view".to_string(),
            pr_url.to_string(),
            "--json".to_string(),
            "state,statusCheckRollup".to_string(),
        ],
        project_path,
        super::providers::SpawnOptions {
            timeout_ms: Some(gh_timeout_ms),
            project_alias: None,
            env_prompt: None,
            env_model: None,
        },
    )?;
    if raw.exit_code != 0 {
        let stderr = raw.stderr.trim();
        return Err(if stderr.is_empty() {
            format!("gh pr view failed with exit {}", raw.exit_code)
        } else {
            stderr.to_string()
        });
    }
    let view: serde_json::Value = serde_json::from_str(&raw.stdout).map_err(|_| {
        format!(
            "gh pr view returned unparseable output: {}",
            raw.stdout.chars().take(200).collect::<String>()
        )
    })?;

    let state = view
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut checks: Vec<PromotionCheckRollupEntry> = Vec::new();
    if let Some(rollup) = view.get("statusCheckRollup").and_then(|v| v.as_array()) {
        for entry in rollup {
            // gh mixes CheckRun entries (name + conclusion) with legacy
            // StatusContext entries (context + state). Normalize both into
            // one shape: a StatusContext's state acts as the conclusion.
            let name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| entry.get("context").and_then(|v| v.as_str()))
                .map(|s| s.to_string());
            checks.push(PromotionCheckRollupEntry {
                name,
                status: entry
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                conclusion: entry
                    .get("conclusion")
                    .and_then(|v| v.as_str())
                    .or_else(|| entry.get("state").and_then(|v| v.as_str()))
                    .map(|s| s.to_string()),
            });
        }
    }

    let merged = state.as_deref() == Some("MERGED");
    let closed = state.as_deref() == Some("CLOSED");

    // Persist a refreshed result artifact only when the observed state
    // changed (the daemon polls on a cadence; open PRs must not accumulate
    // a file per poll).
    if latest.state.as_deref() != state.as_deref() || merged || closed {
        let mut refreshed = latest.clone();
        refreshed.created_at = now_iso();
        refreshed.result = "refreshed".to_string();
        refreshed.note = Some(format!(
            "Refreshed from {pr_url}: state={}, checks={}",
            state.clone().unwrap_or_else(|| "unknown".into()),
            checks.len()
        ));
        refreshed.state = state.clone();
        refreshed.checks = Some(checks.clone());
        super::promotion_artifacts::write_promotion_result_artifact(project_path, &refreshed)?;
    }

    const FAILING_CONCLUSIONS: [&str; 4] = ["FAILURE", "ERROR", "TIMED_OUT", "ACTION_REQUIRED"];
    let failed_checks: Vec<String> = checks
        .iter()
        .filter(|check| {
            check
                .conclusion
                .as_deref()
                .map(|c| FAILING_CONCLUSIONS.contains(&c))
                .unwrap_or(false)
        })
        .map(|check| {
            check
                .name
                .clone()
                .unwrap_or_else(|| "unknown-check".to_string())
        })
        .collect();

    if merged {
        super::task_ledger::with_task_ledger(project_path, |ledger| {
            if let Some(task) = ledger
                .tasks
                .iter_mut()
                .find(|candidate| candidate.id == task_id)
            {
                task.status = TaskStatus::Promoted;
                task.promoted_at = Some(now_iso());
                let mut notes = task.notes.clone().unwrap_or_default();
                notes.push(format!("PR merged: {pr_url}"));
                task.notes = Some(notes);
                task.updated_at = now_iso();
            }
            Ok(())
        })?;
        // A still-pending promotion artifact for this task is now moot —
        // retire it so review backpressure does not count a merged PR.
        let _ = super::promotion_queue::update_promotion_artifact(
            project_path,
            task_id,
            PromotionArtifactStatus::Applied,
            Some(&format!("PR merged: {pr_url}")),
        );
    } else if closed {
        let rejection = super::promotion_queue::update_promotion_artifact(
            project_path,
            task_id,
            PromotionArtifactStatus::Rejected,
            Some(&format!("PR closed: {pr_url}")),
        );
        if rejection.is_err() {
            super::task_ledger::with_task_ledger(project_path, |ledger| {
                if let Some(task) = ledger
                    .tasks
                    .iter_mut()
                    .find(|candidate| candidate.id == task_id)
                {
                    let mut notes = task.notes.clone().unwrap_or_default();
                    notes.push(format!("PR closed without merging: {pr_url}"));
                    task.notes = Some(notes);
                    task.updated_at = now_iso();
                }
                Ok(())
            })?;
        }
    }

    if !failed_checks.is_empty() {
        let note = format!("PR checks failed: {}", failed_checks.join(", "));
        super::task_ledger::with_task_ledger(project_path, |ledger| {
            if let Some(task) = ledger
                .tasks
                .iter_mut()
                .find(|candidate| candidate.id == task_id)
            {
                let mut notes = task.notes.clone().unwrap_or_default();
                notes.push(note.clone());
                task.notes = Some(notes);
                task.updated_at = now_iso();
            }
            Ok(())
        })?;
        let mut payload = super::worker::build_lifecycle_payload(
            "promotion-ci-failed",
            project_alias,
            task_id,
            &note,
            "idle",
        );
        payload.failed_checks = Some(failed_checks.clone());
        let _ = super::hooks::run_lifecycle_hooks(
            config,
            None,
            &payload,
            Some(&super::paths::daemon_log_path(None).to_string_lossy()),
        );
    }

    Ok(PrRefreshOutcome {
        project_alias: project_alias.to_string(),
        task_id: task_id.to_string(),
        pr_url: pr_url.to_string(),
        state,
        failed_checks,
        merged,
        closed,
    })
}
