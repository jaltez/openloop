use crate::core as c;

/// `promotion refresh`: pull live PR state via `gh pr view` and feed it back
/// into the ledger. The behavior lives in core (the daemon poll loop shares
/// it); this wrapper targets one task and prints the outcome. Errors
/// propagate as Err — main() turns them into a non-zero exit.
pub fn refresh(project_alias: &str, task_id: &str) -> Result<(), String> {
    let project = c::project_registry::get_project(project_alias, None)?;
    let config = c::global_config::load_global_config(None)?;
    let outcome = c::pr_refresh::refresh_task_pr(
        &config,
        &project.alias,
        std::path::Path::new(&project.path),
        task_id,
        30_000,
    )?;

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "taskId": outcome.task_id,
            "prUrl": outcome.pr_url,
            "state": outcome.state,
            "failedChecks": outcome.failed_checks,
        }))
        .unwrap_or_default()
    );
    Ok(())
}
