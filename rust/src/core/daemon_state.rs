use std::path::{Path, PathBuf};

use super::file_lock::with_file_lock;
use super::fsutil::{read_json_file, write_json_file};
use super::now_iso;
use super::paths::{daemon_state_path, runtime_dir};
use super::types::DaemonState;

const DEFAULT_BUDGET_DATE: &str = "1970-01-01";

pub fn create_default_daemon_state() -> DaemonState {
    DaemonState {
        version: 1,
        started_at: epoch_iso(),
        pid: 0,
        active_project: None,
        paused: false,
        paused_at: None,
        total_budget_spent_usd: 0.0,
        budget_date: DEFAULT_BUDGET_DATE.to_string(),
        budget_spent_usd: 0.0,
        budget_blocked: false,
        current_run: None,
        projects: Vec::new(),
        last_digest_date: None,
    }
}

fn epoch_iso() -> String {
    "1970-01-01T00:00:00.000Z".to_string()
}

pub fn load_daemon_state(app_home_override: Option<&str>) -> Result<DaemonState, String> {
    let partial: serde_json::Value = read_json_file(&daemon_state_path(app_home_override), serde_json::Value::Null)?;
    if partial.is_null() {
        return Ok(create_default_daemon_state());
    }
    // Merge over defaults for forward/backward compatibility.
    let mut state = create_default_daemon_state();
    if let Some(v) = partial.get("startedAt").and_then(|v| v.as_str()) {
        state.started_at = v.to_string();
    }
    if let Some(v) = partial.get("pid").and_then(|v| v.as_i64()) {
        state.pid = v as i32;
    }
    state.active_project = partial
        .get("activeProject")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    state.paused = partial.get("paused").and_then(|v| v.as_bool()).unwrap_or(false);
    state.paused_at = partial
        .get("pausedAt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    state.total_budget_spent_usd = partial
        .get("totalBudgetSpentUsd")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if let Some(v) = partial.get("budgetDate").and_then(|v| v.as_str()) {
        state.budget_date = v.to_string();
    }
    state.budget_spent_usd = partial
        .get("budgetSpentUsd")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    state.budget_blocked = partial
        .get("budgetBlocked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    state.current_run = partial
        .get("currentRun")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    if let Some(projects) = partial.get("projects").and_then(|v| v.as_array()) {
        state.projects = projects
            .iter()
            .filter_map(|p| serde_json::from_value(p.clone()).ok())
            .collect();
    }
    state.last_digest_date = partial
        .get("lastDigestDate")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Ok(state)
}

pub fn save_daemon_state(state: &DaemonState, app_home_override: Option<&str>) -> Result<(), String> {
    write_json_file(&daemon_state_path(app_home_override), state)
}

fn state_lock_path(app_home_override: Option<&str>) -> PathBuf {
    runtime_dir(app_home_override).join(".daemon-state.lock")
}

/// Run a load-modify-save cycle on the daemon state under the cross-process
/// `.daemon-state.lock`. The mutator receives the FRESHLY loaded state and
/// mutates only the fields it owns — anything another process set concurrently
/// (a pause, a budget charge) survives, because nothing is overwritten from a
/// stale snapshot.
pub fn with_daemon_state(
    mutator: impl FnOnce(&mut DaemonState),
    app_home_override: Option<&str>,
) -> Result<DaemonState, String> {
    with_file_lock(&state_lock_path(app_home_override), "Daemon state", || {
        let mut state = load_daemon_state(app_home_override)?;
        mutator(&mut state);
        save_daemon_state(&state, app_home_override)?;
        Ok(state)
    })
}

pub fn pause_daemon(app_home_override: Option<&str>) -> Result<DaemonState, String> {
    let requested_at = now_iso();
    with_daemon_state(
        |state| {
            state.paused = true;
            state.paused_at = Some(requested_at.clone());
            if let Some(run) = state.current_run.as_mut() {
                run.pause_requested_at = Some(requested_at.clone());
            }
        },
        app_home_override,
    )
}

pub fn resume_daemon(app_home_override: Option<&str>) -> Result<DaemonState, String> {
    with_daemon_state(
        |state| {
            state.paused = false;
            state.paused_at = None;
            if let Some(run) = state.current_run.as_mut() {
                run.pause_requested_at = None;
            }
        },
        app_home_override,
    )
}

pub fn _unused(_p: &Path) {}
