use std::collections::BTreeMap;

use super::fsutil::{read_json_file, write_json_file};
use super::types::*;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TaskListSummary {
    pub total: usize,
    pub by_status: BTreeMap<String, usize>,
    pub by_risk: BTreeMap<String, usize>,
}

pub fn load_task_ledger(project_path: &std::path::Path) -> Result<TaskLedger, String> {
    let ledger_path = project_path.join(".openloop").join("tasks.json");
    read_json_file(
        &ledger_path,
        TaskLedger {
            version: 1,
            updated_at: super::epoch_iso(),
            tasks: Vec::new(),
        },
    )
}

pub fn save_task_ledger(project_path: &std::path::Path, ledger: &mut TaskLedger) -> Result<(), String> {
    let ledger_path = project_path.join(".openloop").join("tasks.json");
    ledger.updated_at = super::now_iso();
    write_json_file(&ledger_path, ledger)
}

fn tasks_lock_path(project_path: &std::path::Path) -> std::path::PathBuf {
    project_path.join(".openloop").join(".tasks.lock")
}

/// Run a load-modify-save cycle on the task ledger under the cross-process
/// `.tasks.lock`. The mutator receives the freshly loaded ledger and mutates
/// it in place (or returns a value); the ledger is persisted on completion.
pub fn with_task_ledger<T>(
    project_path: &std::path::Path,
    mutator: impl FnOnce(&mut TaskLedger) -> Result<T, String>,
) -> Result<T, String> {
    super::file_lock::with_file_lock(&tasks_lock_path(project_path), "Task ledger", || {
        let mut ledger = load_task_ledger(project_path)?;
        let result = mutator(&mut ledger)?;
        save_task_ledger(project_path, &mut ledger)?;
        Ok(result)
    })
}

pub fn add_task(project_path: &std::path::Path, task: &mut ProjectTask) -> Result<(), String> {
    with_task_ledger(project_path, |ledger| {
        if let Some(deps) = task.depends_on.as_ref() {
            for r in deps {
                if r.contains(':') {
                    continue; // cross-project refs are accepted and reserved
                }
                if !ledger.tasks.iter().any(|existing| existing.id == *r) {
                    return Err(format!("Unknown task id in dependsOn: {r}"));
                }
            }
        }
        if ledger.tasks.iter().any(|existing| existing.id == task.id) {
            task.id = format!("{}-{}", task.id, super::now_millis());
        }
        ledger.tasks.push(task.clone());
        Ok(())
    })
}

/// Replace or append a single task under the ledger lock. The scheduler holds a
/// long-lived in-memory task object across an agent run; persisting it through
/// an upsert (instead of saving the whole stale in-memory ledger) preserves
/// concurrent edits to other tasks made by the CLI/TUI in the meantime.
pub fn upsert_task(project_path: &std::path::Path, task: &ProjectTask) -> Result<(), String> {
    with_task_ledger(project_path, |ledger| {
        match ledger.tasks.iter().position(|existing| existing.id == task.id) {
            None => ledger.tasks.push(task.clone()),
            Some(index) => ledger.tasks[index] = task.clone(),
        }
        Ok(())
    })
}

pub fn get_task(project_path: &std::path::Path, task_id: &str) -> Result<ProjectTask, String> {
    let ledger = load_task_ledger(project_path)?;
    ledger
        .tasks
        .into_iter()
        .find(|candidate| candidate.id == task_id)
        .ok_or_else(|| format!("Unknown task id: {task_id}"))
}

#[derive(Debug, Clone, Default)]
pub struct TaskListFilters {
    pub status: Option<TaskStatus>,
    pub risk: Option<TaskRisk>,
}

pub fn list_tasks(project_path: &std::path::Path, filters: &TaskListFilters) -> Result<Vec<ProjectTask>, String> {
    let ledger = load_task_ledger(project_path)?;
    let mut tasks: Vec<ProjectTask> = ledger
        .tasks
        .into_iter()
        .filter(|t| filters.status.map(|s| t.status == s).unwrap_or(true))
        .filter(|t| filters.risk.map(|r| t.risk == r).unwrap_or(true))
        .collect();
    tasks.sort_by(|left, right| {
        let updated_order = right.updated_at.cmp(&left.updated_at);
        if updated_order != std::cmp::Ordering::Equal {
            return updated_order;
        }
        left.id.cmp(&right.id)
    });
    Ok(tasks)
}

pub fn summarize_tasks(tasks: &[ProjectTask]) -> TaskListSummary {
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_risk: BTreeMap<String, usize> = BTreeMap::new();
    for task in tasks {
        *by_status.entry(task.status.as_str().to_string()).or_insert(0) += 1;
        *by_risk.entry(task.risk.as_str().to_string()).or_insert(0) += 1;
    }
    TaskListSummary {
        total: tasks.len(),
        by_status,
        by_risk,
    }
}

pub fn summarize_queue(ledger: &TaskLedger) -> (usize, usize) {
    let queue_size = ledger
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Proposed
                    | TaskStatus::Planned
                    | TaskStatus::Ready
                    | TaskStatus::AwaitingApproval
                    | TaskStatus::InProgress
            )
        })
        .count();
    let blocked_tasks = ledger.tasks.iter().filter(|t| t.status == TaskStatus::Blocked).count();
    (queue_size, blocked_tasks)
}

#[derive(Debug, Clone, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub status: Option<TaskStatus>,
    pub risk: Option<TaskRisk>,
    pub kind: Option<TaskKind>,
    pub scope: Option<Option<TaskScope>>,
}

pub fn update_task(project_path: &std::path::Path, task_id: &str, patch: &TaskPatch) -> Result<ProjectTask, String> {
    with_task_ledger(project_path, |ledger| {
        let task = ledger
            .tasks
            .iter_mut()
            .find(|candidate| candidate.id == task_id)
            .ok_or_else(|| format!("Unknown task id: {task_id}"))?;
        if let Some(title) = &patch.title {
            task.title = title.clone();
        }
        if let Some(status) = patch.status {
            task.status = status;
        }
        if let Some(risk) = patch.risk {
            task.risk = risk;
        }
        if let Some(kind) = patch.kind {
            task.kind = kind;
        }
        if let Some(scope) = &patch.scope {
            task.scope = scope.clone();
        }
        task.updated_at = super::now_iso();
        Ok(task.clone())
    })
}

pub fn remove_task(project_path: &std::path::Path, task_id: &str) -> Result<(), String> {
    with_task_ledger(project_path, |ledger| {
        let index = ledger
            .tasks
            .iter()
            .position(|candidate| candidate.id == task_id)
            .ok_or_else(|| format!("Unknown task id: {task_id}"))?;
        ledger.tasks.remove(index);
        Ok(())
    })
}
