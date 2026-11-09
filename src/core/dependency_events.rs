use std::collections::HashSet;
use std::path::Path;

use super::now_iso;
use super::types::*;

/// A cross-repo dependency that just became satisfied, plus the task that
/// was waiting on it.
#[derive(Debug, Clone)]
pub struct DependencySatisfied {
    /// Project alias of the WAITING task.
    pub dependent_project: String,
    pub dependent_task_id: String,
    /// The `alias:task-id` ref that became satisfied.
    pub dependency_ref: String,
    /// The upstream task's satisfying status.
    pub dependency_status: TaskStatus,
}

/// Seed the pending set at tick start (before the PR poll): every
/// currently-unsatisfied cross-repo ref becomes pending; long-satisfied
/// dependencies are NOT recorded and so never fire retroactively. The set
/// is persisted even when empty, so "seeded" is distinguishable from
/// "never ran" on later ticks.
pub fn seed_pending_set(projects: &[LinkedProject]) {
    let resolver = super::scheduler::make_cross_project_resolver();
    let mut pending: Vec<String> = Vec::new();
    for project in projects {
        if !project.initialized {
            continue;
        }
        let Ok(ledger) = super::task_ledger::load_task_ledger(Path::new(&project.path)) else {
            continue;
        };
        for task in &ledger.tasks {
            if task.status != TaskStatus::Ready {
                continue;
            }
            for dep in task.depends_on.iter().flatten() {
                let Some((alias, task_id)) = dep.split_once(':') else {
                    continue;
                };
                let satisfying = matches!(
                    resolver(alias, task_id),
                    Some(TaskStatus::Promoted | TaskStatus::Done | TaskStatus::Cancelled)
                );
                if !satisfying && !pending.contains(dep) {
                    pending.push(dep.clone());
                }
            }
        }
    }
    let _ = super::daemon_state::with_daemon_state(
        |state| {
            state.pending_cross_deps = Some(pending.clone());
        },
        None,
    );
}

/// Scan every initialized project's ledger for Ready tasks waiting on
/// cross-repo dependency refs and detect satisfaction transitions against
/// the daemon state's pending set (`pendingCrossDeps`). Newly satisfied
/// dependencies get a note on the waiting task; the pending set is persisted
/// BEFORE any side effects (at-most-once). When no set has ever been stored
/// (a tick that ran before seeding), satisfying refs seed silently instead
/// of firing retroactively. Returns the transitions for the worker to
/// event/notify.
pub fn detect_and_mark(projects: &[LinkedProject]) -> Vec<DependencySatisfied> {
    let resolver = super::scheduler::make_cross_project_resolver();

    // (dependent alias, dependent path, task id, dep ref) for every waiter.
    let mut waiters: Vec<(String, std::path::PathBuf, String, String)> = Vec::new();
    for project in projects {
        if !project.initialized {
            continue;
        }
        let Ok(ledger) = super::task_ledger::load_task_ledger(Path::new(&project.path)) else {
            continue;
        };
        for task in &ledger.tasks {
            if task.status != TaskStatus::Ready {
                continue;
            }
            for dep in task.depends_on.iter().flatten() {
                if dep.contains(':') {
                    waiters.push((
                        project.alias.clone(),
                        std::path::PathBuf::from(&project.path),
                        task.id.clone(),
                        dep.clone(),
                    ));
                }
            }
        }
    }

    let state = super::daemon_state::load_daemon_state(None).ok();
    let stored_set_absent = state
        .as_ref()
        .map(|s| s.pending_cross_deps.is_none())
        .unwrap_or(true);
    let known_pending: HashSet<String> = state
        .and_then(|s| s.pending_cross_deps)
        .unwrap_or_default()
        .into_iter()
        .collect();

    let mut next_pending: HashSet<String> = HashSet::new();
    let mut satisfied: Vec<DependencySatisfied> = Vec::new();
    for (dependent_alias, dependent_path, task_id, dep_ref) in waiters {
        let status = dep_ref
            .split_once(':')
            .and_then(|(alias, task)| resolver(alias, task));
        let satisfying = matches!(
            status,
            Some(TaskStatus::Promoted | TaskStatus::Done | TaskStatus::Cancelled)
        );
        if !satisfying {
            next_pending.insert(dep_ref);
            continue;
        }
        // Only a pending → satisfied transition is an event; on the first
        // run (no stored set) satisfying refs seed silently.
        if !stored_set_absent && known_pending.contains(&dep_ref) {
            let transition = DependencySatisfied {
                dependent_project: dependent_alias.clone(),
                dependent_task_id: task_id.clone(),
                dependency_ref: dep_ref.clone(),
                dependency_status: status.unwrap(),
            };
            note_waiting_task(&dependent_path, &transition);
            satisfied.push(transition);
        }
    }

    // Persist the pending set before any side effects (at-most-once). An
    // empty set persists as [] (seeded) rather than absent (never ran).
    let persisted: Vec<String> = next_pending.into_iter().collect();
    let _ = super::daemon_state::with_daemon_state(
        |state| {
            state.pending_cross_deps = Some(persisted.clone());
        },
        None,
    );

    satisfied
}

fn note_waiting_task(dependent_path: &Path, transition: &DependencySatisfied) {
    let _ = super::task_ledger::with_task_ledger(dependent_path, |ledger| {
        if let Some(task) = ledger
            .tasks
            .iter_mut()
            .find(|t| t.id == transition.dependent_task_id)
        {
            let mut notes = task.notes.clone().unwrap_or_default();
            notes.push(format!(
                "Dependency {} satisfied ({}); task is eligible to run.",
                transition.dependency_ref,
                transition.dependency_status.as_str()
            ));
            task.notes = Some(notes);
            task.updated_at = now_iso();
        }
        Ok(())
    });
}
