use super::daemon_state::load_daemon_state;
use super::global_config::load_global_config;
use super::project_registry::list_projects;
use super::task_ledger::{load_task_ledger, summarize_queue};
use super::types::LinkedProject;

#[derive(Debug, Clone)]
pub struct ProjectQueueState {
    pub project: LinkedProject,
    pub queue_size: usize,
    pub blocked_tasks: usize,
}

pub fn load_project_queue_states(
    app_home_override: Option<&str>,
) -> Result<Vec<ProjectQueueState>, String> {
    let projects = list_projects(app_home_override)?;
    let mut states = Vec::new();
    for project in projects {
        let ledger = load_task_ledeger_lossy(&project.path);
        let (queue_size, blocked_tasks) = summarize_queue(&ledger);
        states.push(ProjectQueueState {
            project,
            queue_size,
            blocked_tasks,
        });
    }
    Ok(states)
}

fn load_task_ledeger_lossy(project_path: &str) -> super::types::TaskLedger {
    load_task_ledger(std::path::Path::new(project_path)).unwrap_or(super::types::TaskLedger {
        version: 1,
        updated_at: String::new(),
        tasks: Vec::new(),
    })
}

pub fn select_next_project(
    app_home_override: Option<&str>,
) -> Result<Option<LinkedProject>, String> {
    let states = load_project_queue_states(app_home_override)?;
    let config = load_global_config(app_home_override)?;
    let eligible: Vec<&ProjectQueueState> = states
        .iter()
        .filter(|state| state.project.initialized && state.queue_size > 0)
        .collect();

    if eligible.is_empty() {
        return Ok(None);
    }

    // Explicit activeProjectAlias always wins regardless of strategy.
    if let Some(active_alias) = &config.active_project_alias {
        if let Some(active) = eligible
            .iter()
            .find(|state| &state.project.alias == active_alias)
        {
            return Ok(Some(active.project.clone()));
        }
    }

    let strategy = config
        .runtime
        .project_selection_strategy
        .clone()
        .unwrap_or_else(|| "round-robin".to_string());

    if strategy == "priority" {
        // Project with the most ready tasks first.
        let mut sorted: Vec<&ProjectQueueState> = eligible.clone();
        sorted.sort_by(|left, right| {
            right
                .queue_size
                .cmp(&left.queue_size)
                .then_with(|| left.project.alias.cmp(&right.project.alias))
        });
        return Ok(sorted.first().map(|s| s.project.clone()));
    }

    if strategy == "focus" {
        // Stay with the first project that has work until its queue is empty.
        let mut sorted: Vec<&ProjectQueueState> = eligible.clone();
        sorted.sort_by(|left, right| left.project.alias.cmp(&right.project.alias));
        return Ok(sorted.first().map(|s| s.project.clone()));
    }

    // Default: "round-robin" — the eligible project that has waited longest
    // (oldest lastIterationAt) runs next; ties break alphabetically by alias.
    let daemon_state = load_daemon_state(app_home_override).ok();
    let mut sorted: Vec<&ProjectQueueState> = eligible.clone();
    sorted.sort_by(|left, right| {
        let get = |state: &ProjectQueueState| -> Option<String> {
            daemon_state
                .as_ref()
                .and_then(|ds| ds.projects.iter().find(|p| p.alias == state.project.alias))
                .and_then(|p| p.last_iteration_at.clone())
        };
        let (left_at, right_at) = (get(left), get(right));
        match (left_at, right_at) {
            (None, None) => left.project.alias.cmp(&right.project.alias),
            (None, Some(_)) => std::cmp::Ordering::Less, // never ran — goes first
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(l), Some(r)) => l
                .cmp(&r)
                .then_with(|| left.project.alias.cmp(&right.project.alias)),
        }
    });
    Ok(sorted.first().map(|s| s.project.clone()))
}
