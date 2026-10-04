// Integration tests for project selection: the activeProjectAlias override
// and the round-robin fallback (oldest lastIterationAt runs next, ties break
// alphabetically, never-started projects go first).
//
// Every helper passes an explicit app-home override, so OPENLOOP_HOME is
// never touched and no cross-test env lock is required.

use std::fs;
use std::path::{Path, PathBuf};

use openloop::core::daemon_state::{
    create_default_daemon_state, load_daemon_state, save_daemon_state,
};
use openloop::core::global_config::{default_global_config, save_global_config};
use openloop::core::project_registry::{add_project, mark_project_initialized};
use openloop::core::project_selection::select_next_project;
use openloop::core::types::*;

const NOW: &str = "2026-03-09T09:00:00.000Z";

struct Sandbox {
    home: PathBuf,
    projects: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
        let _ = fs::remove_dir_all(&self.projects);
    }
}

fn sandbox(name: &str) -> Sandbox {
    let suffix = format!("{name}-{}", std::process::id());
    let home = std::env::temp_dir().join(format!("ol-select-home-{suffix}"));
    let projects = std::env::temp_dir().join(format!("ol-select-projects-{suffix}"));
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&projects);
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&projects).unwrap();
    Sandbox { home, projects }
}

/// Inline fixture: an initialized project with `ready_tasks` queued tasks and
/// its daemon-state iteration timestamp (`None` = never ran, so it gets no
/// daemon-state entry, matching the TS suite).
struct ProjectFixture {
    alias: &'static str,
    ready_tasks: usize,
    last_iteration_at: Option<&'static str>,
}

fn fx(
    alias: &'static str,
    ready_tasks: usize,
    last_iteration_at: Option<&'static str>,
) -> ProjectFixture {
    ProjectFixture {
        alias,
        ready_tasks,
        last_iteration_at,
    }
}

fn make_task(id: &str) -> ProjectTask {
    ProjectTask {
        id: id.into(),
        title: format!("Task {id}"),
        kind: TaskKind::Feature,
        status: TaskStatus::Ready,
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
        promotion: TaskPromotion::AutoMerge,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

fn seed_project(home: &Path, projects_root: &Path, fixture: &ProjectFixture) {
    let dir = projects_root.join(fixture.alias);
    fs::create_dir_all(dir.join(".openloop")).unwrap();
    let tasks = (0..fixture.ready_tasks)
        .map(|i| make_task(&format!("{}{}", fixture.alias, i + 1)))
        .collect();
    fs::write(
        dir.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&TaskLedger {
            version: 1,
            updated_at: NOW.into(),
            tasks,
        })
        .unwrap(),
    )
    .unwrap();
    let home_str = home.to_string_lossy().to_string();
    let dir_str = dir.to_string_lossy().to_string();
    add_project(fixture.alias, &dir_str, Some(&home_str)).unwrap();
    mark_project_initialized(fixture.alias, Some(&home_str)).unwrap();
}

fn write_config(home: &Path, active_alias: Option<&str>, strategy: Option<&str>) {
    let mut config = default_global_config();
    config.active_project_alias = active_alias.map(str::to_string);
    config.runtime.project_selection_strategy = strategy.map(str::to_string);
    let home_str = home.to_string_lossy().to_string();
    save_global_config(&config, Some(&home_str)).unwrap();
}

fn daemon_entry(
    alias: &str,
    queue_size: usize,
    last_iteration_at: Option<&str>,
) -> DaemonProjectState {
    DaemonProjectState {
        alias: alias.into(),
        queue_size,
        paused: false,
        last_iteration_at: last_iteration_at.map(str::to_string),
        last_result: Some("idle".into()),
        blocked_tasks: 0,
    }
}

fn write_daemon_state(home: &Path, entries: Vec<DaemonProjectState>) {
    let mut state = create_default_daemon_state();
    state.started_at = NOW.into();
    state.pid = 1;
    state.budget_date = "2026-03-09".into();
    state.projects = entries;
    let home_str = home.to_string_lossy().to_string();
    save_daemon_state(&state, Some(&home_str)).unwrap();
}

fn selected_alias(home: &Path) -> Option<String> {
    let home_str = home.to_string_lossy().to_string();
    select_next_project(Some(&home_str))
        .unwrap()
        .map(|project| project.alias)
}

fn select_from_fixture(
    name: &str,
    active_alias: Option<&str>,
    strategy: Option<&str>,
    fixtures: &[ProjectFixture],
) -> Option<String> {
    let sandbox = sandbox(name);
    for fixture in fixtures {
        seed_project(&sandbox.home, &sandbox.projects, fixture);
    }
    write_config(&sandbox.home, active_alias, strategy);
    let entries = fixtures
        .iter()
        .filter(|fixture| fixture.last_iteration_at.is_some())
        .map(|fixture| {
            daemon_entry(
                fixture.alias,
                fixture.ready_tasks,
                fixture.last_iteration_at,
            )
        })
        .collect();
    write_daemon_state(&sandbox.home, entries);
    selected_alias(&sandbox.home)
}

#[test]
fn active_project_alias_wins_over_eligible_projects_regardless_of_strategy() {
    // "a" is the natural winner under every fallback strategy (alphabetically
    // first, oldest lastIterationAt, biggest queue); the explicit
    // activeProjectAlias must override them all and select "b".
    let strategies = [None, Some("round-robin"), Some("priority"), Some("focus")];
    for (index, strategy) in strategies.into_iter().enumerate() {
        let selected = select_from_fixture(
            &format!("active-{index}"),
            Some("b"),
            strategy,
            &[
                fx("a", 3, Some("2026-03-09T08:00:00.000Z")),
                fx("b", 1, Some("2026-03-09T10:00:00.000Z")),
            ],
        );
        assert_eq!(
            selected.as_deref(),
            Some("b"),
            "strategy {strategy:?} must defer to activeProjectAlias"
        );
    }
}

#[test]
fn round_robin_picks_the_eligible_project_with_the_oldest_last_iteration_at() {
    // b waited longer (09:00 vs a's 10:00) — round-robin must rotate to b
    // instead of alphabetical a.
    let selected = select_from_fixture(
        "oldest-iteration",
        None,
        None,
        &[
            fx("a", 1, Some("2026-03-09T10:00:00.000Z")),
            fx("b", 1, Some("2026-03-09T09:00:00.000Z")),
        ],
    );
    assert_eq!(selected.as_deref(), Some("b"));
}

#[test]
fn round_robin_runs_never_started_projects_first_and_breaks_ties_alphabetically() {
    let cases: &[(&str, &[ProjectFixture], &str)] = &[
        (
            "project without a daemon-state entry (never ran) goes first",
            &[
                fx("a", 1, Some("2026-03-09T09:00:00.000Z")),
                fx("b", 1, Some("2026-03-09T09:00:00.000Z")),
                fx("c", 1, None),
            ],
            "c",
        ),
        (
            "tie at the oldest timestamp breaks alphabetically by alias",
            &[
                fx("a", 1, Some("2026-03-09T09:00:00.000Z")),
                fx("b", 1, Some("2026-03-09T09:00:00.000Z")),
                fx("c", 1, Some("2026-03-09T11:00:00.000Z")),
            ],
            "a",
        ),
    ];
    for (index, (label, fixtures, expected)) in cases.iter().enumerate() {
        let selected = select_from_fixture(&format!("tie-{index}"), None, None, fixtures);
        assert_eq!(selected.as_deref(), Some(*expected), "{label}");
    }
}

#[test]
fn round_robin_reselects_after_a_never_ran_project_registers_an_iteration() {
    let sandbox = sandbox("reselect");
    for alias in ["a", "b", "c"] {
        seed_project(&sandbox.home, &sandbox.projects, &fx(alias, 1, Some(NOW)));
    }
    write_config(&sandbox.home, None, None);

    // c has no daemon-state entry (never ran) — it goes first.
    write_daemon_state(
        &sandbox.home,
        vec![
            daemon_entry("a", 1, Some(NOW)),
            daemon_entry("b", 1, Some(NOW)),
        ],
    );
    assert_eq!(selected_alias(&sandbox.home).as_deref(), Some("c"));

    // Register c with a newer iteration: a and b now tie at the oldest
    // timestamp — alphabetical picks a.
    let home_str = sandbox.home.to_string_lossy().to_string();
    let mut state = load_daemon_state(Some(&home_str)).unwrap();
    state
        .projects
        .push(daemon_entry("c", 1, Some("2026-03-09T11:00:00.000Z")));
    save_daemon_state(&state, Some(&home_str)).unwrap();
    assert_eq!(selected_alias(&sandbox.home).as_deref(), Some("a"));
}
