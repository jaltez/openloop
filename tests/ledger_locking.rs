// Concurrency tests for the cross-process ledger and daemon-state locks:
// the serialization that keeps CLI, TUI, and daemon from losing updates.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use openloop::core::daemon_state::{pause_daemon, resume_daemon, with_daemon_state};
use openloop::core::task_ledger::{
    add_task, list_tasks, load_task_ledger, remove_task, update_task, with_task_ledger, TaskPatch,
};
use openloop::core::types::*;

fn sandbox(name: &str) -> Sandbox {
    let dir = std::env::temp_dir().join(format!("ol-lock-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".openloop")).unwrap();
    Sandbox(dir)
}

struct Sandbox(PathBuf);

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

use std::ops::{Deref, DerefMut};
impl Deref for Sandbox {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}
impl DerefMut for Sandbox {
    fn deref_mut(&mut self) -> &mut PathBuf {
        &mut self.0
    }
}

fn make_task(id: &str, status: TaskStatus) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status,
        risk: TaskRisk::LowRisk,
        scope: None,
        source: TaskSource {
            source_type: "human".into(),
            r#ref: "test".into(),
        },
        spec_id: None,
        branch: None,
        owner: None,
        acceptance_criteria: vec![],
        attempts: 0,
        last_failure_signature: None,
        promotion: TaskPromotion::PullRequest,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

#[test]
fn concurrent_adds_all_land_no_lost_updates() {
    let dir = sandbox("concurrent-adds");
    let dir = Arc::new(dir);
    let mut handles = Vec::new();
    for i in 0..8 {
        let dir = Arc::clone(&dir);
        handles.push(std::thread::spawn(move || {
            let mut task = make_task(&format!("t{i}"), TaskStatus::Proposed);
            add_task(&dir, &mut task).unwrap();
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let ledger = load_task_ledger(&dir).unwrap();
    assert_eq!(ledger.tasks.len(), 8, "every serialized add must persist");
    let ids: Vec<&str> = ledger.tasks.iter().map(|t| t.id.as_str()).collect();
    for i in 0..8 {
        assert!(ids.contains(&format!("t{i}").as_str()), "missing t{i}");
    }
}

#[test]
fn concurrent_mutators_keep_other_fields() {
    let dir = sandbox("concurrent-patch");
    // One task; N threads each append a note through with_task_ledger.
    let mut task = make_task("shared", TaskStatus::Ready);
    task.notes = Some(Vec::new());
    add_task(&dir, &mut task).unwrap();

    let dir = Arc::new(dir);
    let mut handles = Vec::new();
    for i in 0..8 {
        let dir = Arc::clone(&dir);
        handles.push(std::thread::spawn(move || {
            with_task_ledger(&dir, |ledger| {
                let task = ledger.tasks.iter_mut().find(|t| t.id == "shared").unwrap();
                let mut notes = task.notes.take().unwrap_or_default();
                notes.push(format!("note-{i}"));
                task.notes = Some(notes);
                Ok(())
            })
            .unwrap();
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }

    let ledger = load_task_ledger(&dir).unwrap();
    let task = ledger.tasks.iter().find(|t| t.id == "shared").unwrap();
    let notes = task.notes.as_deref().unwrap();
    assert_eq!(
        notes.len(),
        8,
        "every note append survives — no lost updates"
    );
    for i in 0..8 {
        assert!(notes.contains(&format!("note-{i}")), "missing note-{i}");
    }
}

#[test]
fn add_task_validates_depends_on_and_suffixes_duplicates() {
    let dir = sandbox("add-validation");
    let mut a = make_task("a", TaskStatus::Proposed);
    add_task(&dir, &mut a).unwrap();

    // Unknown local ref rejected.
    let mut bad = make_task("bad", TaskStatus::Proposed);
    bad.depends_on = Some(vec!["missing".into()]);
    let err = add_task(&dir, &mut bad).unwrap_err();
    assert!(
        err.contains("Unknown task id in dependsOn"),
        "unexpected: {err}"
    );

    // Cross-project refs accepted.
    let mut cross = make_task("cross", TaskStatus::Proposed);
    cross.depends_on = Some(vec!["otherproj:1".into()]);
    add_task(&dir, &mut cross).unwrap();

    // Satisfied dependency accepted.
    let mut ok = make_task("ok", TaskStatus::Proposed);
    ok.depends_on = Some(vec!["a".into()]);
    add_task(&dir, &mut ok).unwrap();

    // Duplicate id gets a timestamp suffix instead of clobbering.
    let mut dup = make_task("a", TaskStatus::Proposed);
    add_task(&dir, &mut dup).unwrap();
    assert!(
        dup.id.starts_with("a-"),
        "duplicate id must be suffixed: {}",
        dup.id
    );
    assert_eq!(load_task_ledger(&dir).unwrap().tasks.len(), 4);
}

#[test]
fn update_remove_and_filter_round_trip() {
    let dir = sandbox("crud");
    let mut t = make_task("t1", TaskStatus::Proposed);
    add_task(&dir, &mut t).unwrap();

    let updated = update_task(
        &dir,
        "t1",
        &TaskPatch {
            title: Some("renamed".into()),
            status: Some(TaskStatus::Ready),
            risk: Some(TaskRisk::HighRisk),
            kind: None,
            scope: None,
        },
    )
    .unwrap();
    assert_eq!(updated.title, "renamed");
    assert_eq!(updated.status, TaskStatus::Ready);
    assert_eq!(updated.risk, TaskRisk::HighRisk);

    let filtered = list_tasks(
        &dir,
        &openloop::core::task_ledger::TaskListFilters {
            status: Some(TaskStatus::Ready),
            risk: None,
        },
    )
    .unwrap();
    assert_eq!(filtered.len(), 1);

    let none = list_tasks(
        &dir,
        &openloop::core::task_ledger::TaskListFilters {
            status: Some(TaskStatus::Blocked),
            risk: None,
        },
    )
    .unwrap();
    assert!(none.is_empty());

    remove_task(&dir, "t1").unwrap();
    assert!(load_task_ledger(&dir).unwrap().tasks.is_empty());
}

// ---------------------------------------------------------------------------
// Daemon state
// ---------------------------------------------------------------------------

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct HomeGuard {
    home: PathBuf,
    saved: Option<String>,
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match self.saved.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn isolated_home(name: &str) -> HomeGuard {
    // Callers already hold ENV_LOCK.
    let home = std::env::temp_dir().join(format!("ol-state-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    let saved = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);
    HomeGuard { home, saved }
}

#[test]
fn pause_resume_round_trip_and_fresh_state_merges() {
    let _env = ENV_LOCK.lock().unwrap();
    let _guard = isolated_home("pause-resume");

    let paused = pause_daemon(None).unwrap();
    assert!(paused.paused);
    assert!(paused.paused_at.is_some());

    let resumed = resume_daemon(None).unwrap();
    assert!(!resumed.paused);
    assert!(resumed.paused_at.is_none());

    // Fresh-state merge: two sequential mutators do not clobber each other.
    with_daemon_state(
        |state| {
            state.budget_spent_usd = 1.0;
        },
        None,
    )
    .unwrap();
    with_daemon_state(
        |state| {
            state.total_budget_spent_usd += 1.0;
        },
        None,
    )
    .unwrap();
    let state = with_daemon_state(|_| {}, None).unwrap();
    assert_eq!(
        state.budget_spent_usd, 1.0,
        "first write survives the second mutator"
    );
    assert_eq!(state.total_budget_spent_usd, 1.0);
}

#[test]
fn concurrent_daemon_state_writes_all_land() {
    let _env = ENV_LOCK.lock().unwrap();
    let _guard = isolated_home("concurrent-state");

    let home = std::env::var("OPENLOOP_HOME").unwrap();
    let home = Arc::new(home);
    let mut handles = Vec::new();
    for _ in 0..8 {
        let home = Arc::clone(&home);
        handles.push(std::thread::spawn(move || {
            // Each thread charges a delta through the lock.
            std::env::set_var("OPENLOOP_HOME", home.as_str());
            with_daemon_state(
                |state| {
                    state.budget_spent_usd =
                        ((state.budget_spent_usd + 0.5) * 10_000.0).round() / 10_000.0;
                },
                None,
            )
            .unwrap();
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }

    let state = with_daemon_state(|_| {}, None).unwrap();
    assert_eq!(
        state.budget_spent_usd, 4.0,
        "8 x 0.5 deltas must all persist"
    );
}

#[test]
fn ledger_lock_file_is_cleaned_up() {
    let dir = sandbox("lock-hygiene");
    let mut t = make_task("t", TaskStatus::Proposed);
    add_task(&dir, &mut t).unwrap();
    assert!(
        !dir.join(".openloop").join(".tasks.lock").exists(),
        "lock file must be released after the cycle"
    );
}

#[test]
fn corrupt_ledger_fails_loudly() {
    let dir: &Path = &sandbox("corrupt");
    fs::write(dir.join(".openloop").join("tasks.json"), "{corrupt").unwrap();
    let err = load_task_ledger(dir).unwrap_err();
    assert!(err.contains("Corrupt JSON"), "unexpected: {err}");
}
