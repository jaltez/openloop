use std::path::Path;
use std::process::{Command, Stdio};

use crate::core as c;
use c::types::*;

/// `promotion refresh`: pull live PR state via `gh pr view` and feed it back
/// into the ledger. Daemon-side polling is intentionally out of scope for this
/// release — refresh is invoked on demand.
pub fn refresh(project_alias: &str, task_id: &str) -> Result<(), String> {
    let project = c::project_registry::get_project(project_alias, None)?;
    let project_path = Path::new(&project.path);
    let results = c::promotion_artifacts::list_promotion_result_artifacts(project_path, Some(task_id));
    let with_pr = results.iter().find(|(_, artifact)| artifact.pr_url.is_some());
    let Some((_, with_pr)) = with_pr else {
        eprintln!("No PR promotion result found for task {task_id} in {}.", project.alias);
        std::process::exit(1);
    };
    let pr_url = with_pr.pr_url.clone().unwrap();

    let gh_output = Command::new("gh")
        .args(["pr", "view", &pr_url, "--json", "state,statusCheckRollup"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let gh_exit = gh_output.status.code().unwrap_or(1);
    if gh_exit != 0 {
        let stderr = String::from_utf8_lossy(&gh_output.stderr).trim().to_string();
        eprintln!("{}", if stderr.is_empty() { format!("gh pr view failed with exit {gh_exit}") } else { stderr });
        std::process::exit(1);
    }
    let stdout = String::from_utf8_lossy(&gh_output.stdout).to_string();

    let view: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => {
            eprintln!(
                "gh pr view returned unparseable output: {}",
                stdout.chars().take(200).collect::<String>()
            );
            std::process::exit(1);
        }
    };

    let state = view
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut checks: Vec<PromotionCheckRollupEntry> = Vec::new();
    if let Some(rollup) = view.get("statusCheckRollup").and_then(|v| v.as_array()) {
        for entry in rollup {
            // gh mixes CheckRun entries (name + conclusion) with legacy
            // StatusContext entries (context + state). Normalize both into one
            // shape: a StatusContext's state acts as the conclusion.
            let name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| entry.get("context").and_then(|v| v.as_str()))
                .map(|s| s.to_string());
            checks.push(PromotionCheckRollupEntry {
                name,
                status: entry.get("status").and_then(|v| v.as_str()).map(|s| s.to_string()),
                conclusion: entry
                    .get("conclusion")
                    .and_then(|v| v.as_str())
                    .or_else(|| entry.get("state").and_then(|v| v.as_str()))
                    .map(|s| s.to_string()),
            });
        }
    }

    let mut refreshed = with_pr.clone();
    refreshed.created_at = c::now_iso();
    refreshed.result = "refreshed".to_string();
    refreshed.note = Some(format!(
        "Refreshed from {pr_url}: state={}, checks={}",
        state.clone().unwrap_or_else(|| "unknown".into()),
        checks.len()
    ));
    refreshed.state = state.clone();
    refreshed.checks = Some(checks.clone());
    c::promotion_artifacts::write_promotion_result_artifact(project_path, &refreshed)?;

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
        .map(|check| check.name.clone().unwrap_or_else(|| "unknown-check".to_string()))
        .collect();

    if state.as_deref() == Some("MERGED") {
        let _ = c::task_ledger::with_task_ledger(project_path, |ledger| {
            if let Some(task) = ledger.tasks.iter_mut().find(|candidate| candidate.id == task_id) {
                task.status = TaskStatus::Promoted;
                task.promoted_at = Some(c::now_iso());
                let mut notes = task.notes.clone().unwrap_or_default();
                notes.push(format!("PR merged: {pr_url}"));
                task.notes = Some(notes);
                task.updated_at = c::now_iso();
            }
            Ok(())
        });
    } else if state.as_deref() == Some("CLOSED") {
        let rejection = c::promotion_queue::update_promotion_artifact(
            project_path,
            task_id,
            PromotionArtifactStatus::Rejected,
            Some(&format!("PR closed: {pr_url}")),
        );
        if rejection.is_err() {
            let _ = c::task_ledger::with_task_ledger(project_path, |ledger| {
                if let Some(task) = ledger.tasks.iter_mut().find(|candidate| candidate.id == task_id) {
                    let mut notes = task.notes.clone().unwrap_or_default();
                    notes.push(format!("PR closed without merging: {pr_url}"));
                    task.notes = Some(notes);
                    task.updated_at = c::now_iso();
                }
                Ok(())
            });
        }
    }

    if !failed_checks.is_empty() {
        let note = format!("PR checks failed: {}", failed_checks.join(", "));
        let _ = c::task_ledger::with_task_ledger(project_path, |ledger| {
            if let Some(task) = ledger.tasks.iter_mut().find(|candidate| candidate.id == task_id) {
                let mut notes = task.notes.clone().unwrap_or_default();
                notes.push(note.clone());
                task.notes = Some(notes);
                task.updated_at = c::now_iso();
            }
            Ok(())
        });
        if let Ok(config) = c::global_config::load_global_config(None) {
            let payload = c::worker::build_lifecycle_payload(
                "promotion-ci-failed",
                &project.alias,
                task_id,
                &note,
                "idle",
            );
            let mut payload = payload;
            payload.failed_checks = Some(failed_checks.clone());
            let _ = c::hooks::run_lifecycle_hooks(
                &config,
                None,
                &payload,
                Some(&c::paths::daemon_log_path(None).to_string_lossy()),
            );
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "taskId": task_id,
            "prUrl": pr_url,
            "state": state,
            "failedChecks": failed_checks,
        }))
        .unwrap_or_default()
    );
    Ok(())
}
