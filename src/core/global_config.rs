use std::path::Path;

use super::fsutil::{read_json_file, write_json_file};
use super::paths::global_config_path;
use super::types::*;

pub fn default_global_config() -> GlobalConfig {
    GlobalConfig {
        version: 1,
        model: None,
        default_provider: None,
        active_project_alias: None,
        budgets: GlobalBudgets {
            daily_cost_usd: 25.0,
            estimated_cost_per_run_usd: Some(0.1),
        },
        runtime: GlobalRuntime {
            run_timeout_seconds: 1800,
            max_attempts_per_task: 3,
            no_progress_repeat_limit: 2,
            tick_interval_seconds: Some(5),
            max_pending_reviews_per_project: Some(3),
            project_selection_strategy: Some("round-robin".to_string()),
        },
        notifications: Some(GlobalNotifications {
            on_task_complete: None,
            on_task_failed: None,
            on_budget_blocked: None,
            on_all_tasks_done: None,
        }),
        notification_channels: None,
        hooks: Some(Vec::new()),
        dashboard: Some(DashboardConfig {
            port: 7399,
            enabled: false,
        }),
    }
}

pub fn load_global_config(app_home_override: Option<&str>) -> Result<GlobalConfig, String> {
    let defaults = default_global_config();
    let path = global_config_path(app_home_override);
    let partial: serde_json::Value = read_json_file(&path, serde_json::Value::Null)?;
    if partial.is_null() {
        return Ok(defaults);
    }
    // Field-by-field merge with defaults, mirroring loadGlobalConfig.
    let g = &defaults;
    let s = &partial;
    let get_str = |key: &str| -> Option<String> {
        s.get(key).and_then(|v| v.as_str()).map(|v| v.to_string())
    };
    let budgets = s.get("budgets");
    let runtime = s.get("runtime");
    let notifications = s.get("notifications");
    fn sub<'a>(obj: Option<&'a serde_json::Value>, key: &str) -> Option<&'a serde_json::Value> {
        obj.and_then(|o| o.get(key)).filter(|v| !v.is_null())
    }
    Ok(GlobalConfig {
        version: 1,
        model: get_str("model").or_else(|| g.model.clone()),
        default_provider: get_str("defaultProvider"),
        active_project_alias: get_str("activeProjectAlias"),
        budgets: GlobalBudgets {
            daily_cost_usd: sub(budgets, "dailyCostUsd")
                .and_then(|v| v.as_f64())
                .unwrap_or(g.budgets.daily_cost_usd),
            estimated_cost_per_run_usd: sub(budgets, "estimatedCostPerRunUsd")
                .and_then(|v| v.as_f64())
                .or(g.budgets.estimated_cost_per_run_usd),
        },
        runtime: GlobalRuntime {
            run_timeout_seconds: sub(runtime, "runTimeoutSeconds")
                .and_then(|v| v.as_u64())
                .unwrap_or(g.runtime.run_timeout_seconds),
            max_attempts_per_task: sub(runtime, "maxAttemptsPerTask")
                .and_then(|v| v.as_u64())
                .unwrap_or(g.runtime.max_attempts_per_task as u64)
                as u32,
            no_progress_repeat_limit: sub(runtime, "noProgressRepeatLimit")
                .and_then(|v| v.as_u64())
                .unwrap_or(g.runtime.no_progress_repeat_limit as u64)
                as u32,
            tick_interval_seconds: sub(runtime, "tickIntervalSeconds")
                .and_then(|v| v.as_u64())
                .or(g.runtime.tick_interval_seconds),
            max_pending_reviews_per_project: sub(runtime, "maxPendingReviewsPerProject")
                .and_then(|v| v.as_u64())
                .or(g.runtime.max_pending_reviews_per_project.map(|v| v as u64))
                .map(|v| v as u32),
            project_selection_strategy: sub(runtime, "projectSelectionStrategy")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
                .or_else(|| g.runtime.project_selection_strategy.clone()),
        },
        notifications: Some(GlobalNotifications {
            on_task_complete: sub(notifications, "onTaskComplete")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string()),
            on_task_failed: sub(notifications, "onTaskFailed")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string()),
            on_budget_blocked: sub(notifications, "onBudgetBlocked")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string()),
            on_all_tasks_done: sub(notifications, "onAllTasksDone")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string()),
        }),
        notification_channels: s
            .get("notificationChannels")
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        hooks: s
            .get("hooks")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or_else(|| Some(Vec::new())),
        dashboard: Some(DashboardConfig {
            port: {
                let raw_port = s
                    .get("dashboard")
                    .and_then(|d| d.get("port"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(7399);
                // TS parity: an out-of-range port fails loudly at load time
                // instead of silently truncating to a wrong port.
                u16::try_from(raw_port)
                    .map_err(|_| format!("dashboard.port {raw_port} is out of range (0-65535)"))?
            },
            enabled: s
                .get("dashboard")
                .and_then(|d| d.get("enabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }),
    })
}

pub fn save_global_config(
    config: &GlobalConfig,
    app_home_override: Option<&str>,
) -> Result<(), String> {
    write_json_file(&global_config_path(app_home_override), config)
}

/// Apply a dot-notation key to the raw JSON, matching `config set` semantics.
pub fn set_nested_json_value(
    value: &mut serde_json::Value,
    key_path: &str,
    new_value: serde_json::Value,
) {
    let parts: Vec<&str> = key_path.split('.').collect();
    let mut current = value;
    for part in &parts[..parts.len().saturating_sub(1)] {
        if !current.get(*part).map(|v| v.is_object()).unwrap_or(false) {
            current[*part] = serde_json::json!({});
        }
        current = current.get_mut(*part).unwrap();
    }
    if let Some(last) = parts.last() {
        current[*last] = new_value;
    }
}

pub fn _unused(_p: &Path) {}
