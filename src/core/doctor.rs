use std::path::Path;

use super::fsutil::file_exists;
use super::paths::{
    daemon_log_path, daemon_state_path, global_config_path, projects_registry_path, runtime_dir,
};
use super::providers::{binary_exists, PROVIDER_NAMES};

#[derive(Debug, Clone, serde::Serialize)]
pub struct CheckResult {
    pub label: String,
    pub status: &'static str, // "ok" | "warn" | "fail"
    pub detail: String,
}

/// Recursive byte size of a directory tree (best-effort; unreadable entries
/// count as zero).
fn dir_size_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            total += dir_size_bytes(&entry.path());
        } else {
            total += metadata.len();
        }
    }
    total
}

fn check_binary(name: &str) -> CheckResult {
    let found = std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !found {
        return CheckResult {
            label: name.to_string(),
            status: "fail",
            detail: format!("'{name}' not found on PATH"),
        };
    }
    let bin_path = String::from_utf8_lossy(
        &std::process::Command::new("which")
            .arg(name)
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default(),
    )
    .trim()
    .to_string();
    let version = std::process::Command::new(name)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();
    CheckResult {
        label: name.to_string(),
        status: "ok",
        detail: if version.is_empty() {
            format!("found at {bin_path}")
        } else {
            format!("{version} ({bin_path})")
        },
    }
}

fn check_directory_writable(label: &str, dir_path: &std::path::Path) -> CheckResult {
    if dir_path.exists() {
        let writable = std::fs::OpenOptions::new()
            .append(true)
            .open(dir_path.join(".openloop-write-probe"))
            .map(|_| true)
            .unwrap_or(false);
        let _ = std::fs::remove_file(dir_path.join(".openloop-write-probe"));
        if writable {
            return CheckResult {
                label: label.to_string(),
                status: "ok",
                detail: format!("{} (writable)", dir_path.display()),
            };
        }
        return CheckResult {
            label: label.to_string(),
            status: "warn",
            detail: format!("{} exists but is not writable", dir_path.display()),
        };
    }
    CheckResult {
        label: label.to_string(),
        status: "warn",
        detail: format!(
            "{} does not exist yet (will be created on first use)",
            dir_path.display()
        ),
    }
}

fn check_json_file(label: &str, file_path: &std::path::Path) -> CheckResult {
    if !file_exists(file_path) {
        return CheckResult {
            label: label.to_string(),
            status: "warn",
            detail: format!(
                "{} not found (will be created on first use)",
                file_path.display()
            ),
        };
    }
    match std::fs::read_to_string(file_path) {
        Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(_) => CheckResult {
                label: label.to_string(),
                status: "ok",
                detail: format!("{} (valid JSON)", file_path.display()),
            },
            Err(_) => CheckResult {
                label: label.to_string(),
                status: "fail",
                detail: format!("{} contains invalid JSON", file_path.display()),
            },
        },
        Err(_) => CheckResult {
            label: label.to_string(),
            status: "fail",
            detail: format!("{} cannot be read", file_path.display()),
        },
    }
}

fn check_project_validation(alias: &str, config: &super::types::ProjectConfig) -> Vec<CheckResult> {
    let commands = [
        ("lintCommand", &config.validation.lint_command),
        ("testCommand", &config.validation.test_command),
        ("typecheckCommand", &config.validation.typecheck_command),
    ];
    commands
        .iter()
        .map(|(name, command)| match command {
            Some(command) => CheckResult {
                label: format!("{alias}: {name}"),
                status: "ok",
                detail: command.clone(),
            },
            None => CheckResult {
                label: format!("{alias}: {name}"),
                status: "warn",
                detail: "not configured".to_string(),
            },
        })
        .collect()
}

pub fn run_doctor_checks(app_home_override: Option<&str>) -> Result<Vec<CheckResult>, String> {
    let mut results = Vec::new();

    // 1. Check prerequisite binaries
    results.push(check_binary("git"));
    results.push(check_binary("node"));

    // 1b. Check configured default provider
    let mut default_provider_name = "pi".to_string();
    if let Ok(temp_config) = super::global_config::load_global_config(app_home_override) {
        if let Some(p) = &temp_config.default_provider {
            default_provider_name = p.clone();
        }
    }
    if let Some(name) = super::providers::ProviderName::parse(&default_provider_name) {
        if binary_exists(name.as_str()) {
            results.push(CheckResult {
                label: format!("Provider ({})", name.as_str()),
                status: "ok",
                detail: format!("{} binary found", name.label()),
            });
        } else {
            results.push(CheckResult {
                label: format!("Provider ({})", name.as_str()),
                status: "fail",
                detail: format!("'{}' binary not found on PATH", name.as_str()),
            });
        }
    } else {
        results.push(CheckResult {
            label: format!("Provider ({default_provider_name})"),
            status: "fail",
            detail: format!("Unknown provider '{default_provider_name}'"),
        });
    }

    // 1c. List available providers
    let available_providers: Vec<&str> = PROVIDER_NAMES
        .iter()
        .filter(|p| binary_exists(p.as_str()))
        .map(|p| p.as_str())
        .collect();
    if !available_providers.is_empty() {
        results.push(CheckResult {
            label: "Available providers".to_string(),
            status: "ok",
            detail: available_providers.join(", "),
        });
    } else {
        results.push(CheckResult {
            label: "Available providers".to_string(),
            status: "warn",
            detail: "No agent provider binaries found on PATH".to_string(),
        });
    }

    // 2. Check global config
    results.push(check_json_file(
        "Global config",
        &global_config_path(app_home_override),
    ));
    results.push(check_json_file(
        "Project registry",
        &projects_registry_path(app_home_override),
    ));

    // 3. Check global config values
    match super::global_config::load_global_config(app_home_override) {
        Ok(config) => {
            if config.budgets.daily_cost_usd <= 0.0 {
                results.push(CheckResult {
                    label: "Budget ceiling".to_string(),
                    status: "warn",
                    detail: format!(
                        "dailyCostUsd is {} — daemon will be budget-blocked immediately",
                        config.budgets.daily_cost_usd
                    ),
                });
            } else {
                results.push(CheckResult {
                    label: "Budget ceiling".to_string(),
                    status: "ok",
                    detail: format!("${}/day", config.budgets.daily_cost_usd),
                });
            }
        }
        Err(err) => {
            results.push(CheckResult {
                label: "Global config parse".to_string(),
                status: "fail",
                detail: err,
            });
        }
    }

    // 4. Check runtime directory writable
    results.push(check_directory_writable(
        "Runtime directory",
        &runtime_dir(app_home_override),
    ));

    // 5. Check daemon state and log
    results.push(check_json_file(
        "Daemon state",
        &daemon_state_path(app_home_override),
    ));
    let log_path = daemon_log_path(app_home_override);
    if file_exists(&log_path) {
        results.push(CheckResult {
            label: "Daemon log".to_string(),
            status: "ok",
            detail: log_path.to_string_lossy().to_string(),
        });
    } else {
        results.push(CheckResult {
            label: "Daemon log".to_string(),
            status: "warn",
            detail: format!("{} not found (created on daemon start)", log_path.display()),
        });
    }

    // 5b. Notification delivery health: last outcome per channel plus the
    // pending retry queue.
    if let Ok(state) = super::daemon_state::load_daemon_state(app_home_override) {
        let failing: Vec<String> = state
            .notification_status
            .unwrap_or_default()
            .iter()
            .filter(|s| !s.ok)
            .map(|s| {
                format!(
                    "{} ({})",
                    s.channel_key,
                    s.error.as_deref().unwrap_or("unknown error")
                )
            })
            .collect();
        let queued = state.notification_queue.unwrap_or_default().len();
        if failing.is_empty() && queued == 0 {
            results.push(CheckResult {
                label: "Notifications".to_string(),
                status: "ok",
                detail: "all configured channels delivered".to_string(),
            });
        } else {
            let mut detail = String::new();
            if !failing.is_empty() {
                detail.push_str(&format!("failing: {}", failing.join("; ")));
            }
            if queued > 0 {
                if !detail.is_empty() {
                    detail.push_str("; ");
                }
                detail.push_str(&format!("{queued} delivery(ies) awaiting retry"));
            }
            results.push(CheckResult {
                label: "Notifications".to_string(),
                status: "warn",
                detail,
            });
        }
    }

    // 6. Check each linked project
    match super::project_registry::list_projects(app_home_override) {
        Ok(projects) => {
            if projects.is_empty() {
                results.push(CheckResult {
                    label: "Linked projects".to_string(),
                    status: "warn",
                    detail: "No projects linked. Use 'openloop project add' to register one."
                        .to_string(),
                });
            } else {
                results.push(CheckResult {
                    label: "Linked projects".to_string(),
                    status: "ok",
                    detail: format!("{} project(s) registered", projects.len()),
                });
                for project in projects {
                    let project_path = Path::new(&project.path);
                    if !project_path.exists() {
                        results.push(CheckResult {
                            label: format!("Project \"{}\"", project.alias),
                            status: "fail",
                            detail: format!("Path does not exist: {}", project.path),
                        });
                        continue;
                    }
                    if !project.initialized {
                        results.push(CheckResult {
                            label: format!("Project \"{}\"", project.alias),
                            status: "warn",
                            detail: "Not initialized. Run 'openloop project init <alias>'."
                                .to_string(),
                        });
                        continue;
                    }
                    match super::project_config::load_project_config(project_path) {
                        Ok(project_config) => {
                            let project_agent = project_config
                                .agent
                                .as_ref()
                                .and_then(|a| a.agent_type.as_deref())
                                .unwrap_or(&default_provider_name)
                                .to_string();
                            if project_agent != "custom" {
                                if let Some(agent) =
                                    super::providers::ProviderName::parse(&project_agent)
                                {
                                    if !binary_exists(agent.as_str()) {
                                        results.push(CheckResult {
                                            label: format!("{}: agent provider", project.alias),
                                            status: "warn",
                                            detail: format!(
                                                "'{project_agent}' binary not found on PATH"
                                            ),
                                        });
                                    }
                                }
                            }
                            results
                                .extend(check_project_validation(&project.alias, &project_config));
                            // Control-plane disk usage (runs/, promotions/,
                            // promotion-results/, ...).
                            let control_plane = project_path.join(".openloop");
                            if control_plane.is_dir() {
                                let bytes = dir_size_bytes(&control_plane);
                                let mib = bytes as f64 / (1024.0 * 1024.0);
                                let status = if bytes > 512 * 1024 * 1024 {
                                    "warn"
                                } else {
                                    "ok"
                                };
                                results.push(CheckResult {
                                    label: format!("{}: control-plane size", project.alias),
                                    status,
                                    detail: format!(
                                        "{mib:.1} MiB in {} (retention prunes runs/ and \
                                         promotion-results/ per policy)",
                                        control_plane.display()
                                    ),
                                });
                            }
                        }
                        Err(err) => {
                            results.push(CheckResult {
                                label: format!("Project \"{}\" config", project.alias),
                                status: "fail",
                                detail: err,
                            });
                        }
                    }
                }
            }
        }
        Err(err) => {
            results.push(CheckResult {
                label: "Project registry".to_string(),
                status: "fail",
                detail: err,
            });
        }
    }

    Ok(results)
}

pub fn format_doctor_results(results: &[CheckResult]) -> String {
    results
        .iter()
        .map(|r| {
            let icon = match r.status {
                "ok" => "✅",
                "warn" => "⚠️",
                _ => "❌",
            };
            format!("{icon} {}: {}", r.label, r.detail)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
