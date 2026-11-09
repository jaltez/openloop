use std::path::Path;

use super::types::*;

/// Why one project is (or is not) running work right now.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectIdleStatus {
    pub alias: String,
    /// "ready" | "paused" | "budget-blocked" | "not-initialized" |
    /// "review-backpressure" | "dependencies" | "no-eligible-tasks" |
    /// "provider-missing" | "ledger-error"
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FleetIdleStatus {
    /// Daemon-wide gate (paused / budget-blocked), when set.
    pub daemon: Option<ProjectIdleStatus>,
    pub projects: Vec<ProjectIdleStatus>,
}

/// Explain, right now, why the daemon is not running work in each project.
/// Applies the same gates as the tick, in the same order: daemon pause and
/// budget, initialization, review backpressure, task selection (including
/// cross-repo dependency gating), and provider availability. `ready` means
/// the project has runnable work and no gate is holding it back.
pub fn analyze() -> Result<FleetIdleStatus, String> {
    let config = super::global_config::load_global_config(None)?;
    let state = super::daemon_state::load_daemon_state(None).unwrap_or_default();
    let projects = super::project_registry::list_projects(None)?;

    let daemon = if state.paused {
        Some(ProjectIdleStatus {
            alias: "(daemon)".to_string(),
            kind: "paused".to_string(),
            reason: "daemon is paused — 'openloop service resume'".to_string(),
        })
    } else if state.budget_blocked || state.budget_spent_usd >= config.budgets.daily_cost_usd {
        Some(ProjectIdleStatus {
            alias: "(daemon)".to_string(),
            kind: "budget-blocked".to_string(),
            reason: format!(
                "daily budget exhausted at ${:.4} of ${:.4}",
                state.budget_spent_usd, config.budgets.daily_cost_usd
            ),
        })
    } else {
        None
    };

    let statuses = projects
        .iter()
        .map(|project| per_project(project, &config, daemon.as_ref()))
        .collect();

    Ok(FleetIdleStatus {
        daemon,
        projects: statuses,
    })
}

fn per_project(
    project: &LinkedProject,
    config: &GlobalConfig,
    daemon_gate: Option<&ProjectIdleStatus>,
) -> ProjectIdleStatus {
    if let Some(gate) = daemon_gate {
        return ProjectIdleStatus {
            alias: project.alias.clone(),
            kind: gate.kind.clone(),
            reason: gate.reason.clone(),
        };
    }
    if !project.initialized {
        return ProjectIdleStatus {
            alias: project.alias.clone(),
            kind: "not-initialized".to_string(),
            reason: format!("run 'openloop project init {}'", project.alias),
        };
    }
    let project_path = Path::new(&project.path);

    // Review backpressure — same limit the tick enforces.
    let max_pending = config.runtime.max_pending_reviews_per_project.unwrap_or(3);
    if max_pending > 0 {
        let pending = super::promotion_queue::list_promotion_artifacts(project_path)
            .map(|items| {
                items
                    .into_iter()
                    .filter(|item| item.artifact.status == PromotionArtifactStatus::Pending)
                    .count()
            })
            .unwrap_or(0);
        if pending >= max_pending as usize {
            return ProjectIdleStatus {
                alias: project.alias.clone(),
                kind: "review-backpressure".to_string(),
                reason: format!(
                    "{pending} pending promotion(s) at the review limit ({max_pending}) — \
                     'openloop promotion list -p {alias}'",
                    alias = project.alias
                ),
            };
        }
    }

    // Task selection, exactly as the tick would do it (dependency gating
    // included); the selection's own reason is the explanation.
    let ledger = match super::task_ledger::load_task_ledger(project_path) {
        Ok(ledger) => ledger,
        Err(error) => {
            return ProjectIdleStatus {
                alias: project.alias.clone(),
                kind: "ledger-error".to_string(),
                reason: error,
            }
        }
    };
    let resolver = super::scheduler::make_cross_project_resolver();
    let selection = super::scheduler::select_next_task_with_resolver(&ledger, &resolver);
    if selection.task.is_none() {
        let kind = if selection.reason.contains("dependsOn") {
            "dependencies"
        } else {
            "no-eligible-tasks"
        };
        return ProjectIdleStatus {
            alias: project.alias.clone(),
            kind: kind.to_string(),
            reason: selection.reason,
        };
    }

    // Work exists: can the configured provider actually run it?
    let project_config = super::project_config::load_project_config(project_path).ok();
    let agent = project_config
        .as_ref()
        .and_then(|c| c.agent.as_ref())
        .and_then(|a| a.agent_type.clone())
        .or_else(|| config.default_provider.clone())
        .unwrap_or_else(|| "pi".to_string());
    if agent != "custom" && !super::providers::binary_exists(&agent) {
        return ProjectIdleStatus {
            alias: project.alias.clone(),
            kind: "provider-missing".to_string(),
            reason: format!("'{agent}' binary not found on PATH"),
        };
    }

    ProjectIdleStatus {
        alias: project.alias.clone(),
        kind: "ready".to_string(),
        reason: selection.reason,
    }
}
