use chrono::TimeZone;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use super::daemon_state::{load_daemon_state, pause_daemon, with_daemon_state};
use super::hooks::LifecycleHookPayload;
use super::local_date_stamp;
use super::event_log::{append_event, now_event};
use super::fsutil::{ensure_dir, rotate_log_file};
use super::now_iso;
use super::now_millis;
use super::paths::{daemon_log_path, daemon_pid_path, runtime_dir};
use super::types::*;

const MAX_CONSECUTIVE_ERRORS: u32 = 5;

pub static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn handle_signal(_sig: i32) {
    SHUTDOWN.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn append_log(line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(daemon_log_path(None))
    {
        let _ = writeln!(f, "[{line}]");
    }
}

fn append_log_fmt(payload: &str) {
    append_log(&format!("{} {payload}", now_iso()));
}

pub fn start_worker_loop(foreground: bool) -> Result<(), String> {
    ensure_dir(&runtime_dir(None)).map_err(|e| e.to_string())?;
    rotate_log_file(&daemon_log_path(None), 10 * 1024 *1024, 3);
    let started_at = now_iso();
    let _ = std::fs::write(daemon_pid_path(None), format!("{}\n", std::process::id()));

    let mut started_event = now_event("daemon_started");
    started_event.pid = Some(std::process::id() as i32);
    let _ = append_event(&started_event, None);

    recover_stuck_tasks()?;

    // A3: Start dashboard server if enabled
    let init_config = super::global_config::load_global_config(None).unwrap_or_else(|_| super::global_config::default_global_config());
    let mut dashboard: Option<super::dashboard::DashboardHandle> = None;
    if init_config.dashboard.as_ref().map(|d| d.enabled).unwrap_or(false) {
        let port = init_config.dashboard.as_ref().map(|d| d.port).unwrap_or(7399);
        match super::dashboard::start_dashboard(port) {
            Ok(handle) => {
                dashboard = Some(handle);
                append_log_fmt(&format!("dashboard started on port {port}"));
            }
            Err(_) => append_log_fmt("dashboard failed to start"),
        }
    }

    // Signal handling: SIGTERM/SIGINT flip the shutdown flag.
    unsafe {
        libc::signal(libc::SIGTERM, handle_signal as usize);
        libc::signal(libc::SIGINT, handle_signal as usize);
    }

    let mut consecutive_errors: u32 = 0;

    while !SHUTDOWN.load(std::sync::atomic::Ordering::SeqCst) {
        let tick_result = run_worker_tick(&started_at);
        match tick_result {
            Ok(state) => {
                consecutive_errors = 0;
                append_log_fmt("tick");
                if foreground {
                    let ts = &now_iso()[11..19.min(now_iso().len())];
                    let proj = state.active_project.clone().unwrap_or_else(|| "idle".to_string());
                    let summary = match &state.current_run {
                        Some(run) => format!("{} | {} | mode:{}", proj, run.task_id.clone().unwrap_or_else(|| "--".into()), run.mode),
                        None => format!("{proj} | no run"),
                    };
                    eprintln!("[{ts}] {summary}");
                }
            }
            Err(error) => {
                consecutive_errors += 1;
                append_log_fmt(&format!("tick error ({consecutive_errors}/{MAX_CONSECUTIVE_ERRORS}): {error}"));
                if foreground {
                    let first_line = error.lines().next().unwrap_or(&error);
                    eprintln!("[{}] tick error: {first_line}", &now_iso()[11..19.min(now_iso().len())]);
                }
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    append_log_fmt("max consecutive errors reached; entering degraded (paused) state");
                    let _ = pause_daemon(None);
                    consecutive_errors = 0;
                }
            }
        }
        // W9: Use configurable tick interval from global config.
        let tick_config = super::global_config::load_global_config(None).ok();
        let tick_secs = tick_config
            .as_ref()
            .and_then(|c| c.runtime.tick_interval_seconds)
            .unwrap_or(5);
        let deadline = Instant_plus(now_millis(), (tick_secs * 1000) as i64);
        while now_millis() < deadline && !SHUTDOWN.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // Shutdown: kill running agent process groups so they aren't orphaned.
    super::active_runs::kill_active_runs();
    if let Some(handle) = dashboard {
        handle.stop();
    }
    let _ = std::fs::remove_file(daemon_pid_path(None));
    append_log_fmt("shutdown");
    Ok(())
}

fn Instant_plus(millis: i64, add: i64) -> i64 {
    millis + add
}

pub fn run_worker_tick(started_at: &str) -> Result<DaemonState, String> {
    let config = super::global_config::load_global_config(None)?;
    let normalized_state = normalize_daemon_budget_state(load_daemon_state(None)?, started_at);
    let projects = super::project_registry::list_projects(None)?;
    let queue_states = super::project_selection::load_project_queue_states(None)?;

    // Every persist takes the state lock, loads FRESH state, and touches only
    // the fields this tick owns.
    let projects_snapshot = projects.clone();
    let queue_states_snapshot = queue_states.clone();
    let persist_tick_state = |active_project_alias: Option<&str>,
                              last_iteration_at: Option<&str>,
                              iteration_result: Option<&str>,
                              update: &dyn Fn(&mut DaemonState)|
     -> Result<DaemonState, String> {
        let projects = projects_snapshot.clone();
        let queue_states = queue_states_snapshot.clone();
        with_daemon_state(|state| {
            state.started_at = started_at.to_string();
            state.pid = std::process::id() as i32;
            state.projects = projects
                .iter()
                .map(|project| {
                    let queue = queue_states
                        .iter()
                        .find(|candidate| candidate.project.alias == project.alias);
                    let previous = state
                        .projects
                        .iter()
                        .find(|candidate| candidate.alias == project.alias);
                    DaemonProjectState {
                        alias: project.alias.clone(),
                        queue_size: queue.map(|q| q.queue_size).unwrap_or(0),
                        paused: state.paused,
                        last_iteration_at: if Some(project.alias.as_str()) == active_project_alias
                            && last_iteration_at.is_some()
                        {
                            last_iteration_at.map(|s| s.to_string())
                        } else {
                            previous.and_then(|p| p.last_iteration_at.clone())
                        },
                        last_result: if Some(project.alias.as_str()) == active_project_alias {
                            iteration_result
                                .map(|s| s.to_string())
                                .or(Some("idle".to_string()))
                        } else {
                            previous
                                .and_then(|p| p.last_result.clone())
                                .or(Some("idle".to_string()))
                        },
                        blocked_tasks: queue.map(|q| q.blocked_tasks).unwrap_or(0),
                    }
                })
                .collect();
            update(state);
        }, None)
    };

    if normalized_state.paused {
        return persist_tick_state(None, Some(&now_iso()), Some("paused"), &|_| {});
    }

    // Daily digest: fire once per local calendar day on the first tick. The
    // date is persisted BEFORE the hooks run (at-most-once).
    let today = local_date_stamp();
    if normalized_state.last_digest_date.as_deref() != Some(today.as_str()) {
        let _ = with_daemon_state(|state| {
            state.last_digest_date = Some(today.clone());
        }, None);
        if let Ok(digest) = super::digest::build_digest(Some(24 * 60 * 60 * 1000), None) {
            let pending: usize = digest.projects.iter().map(|p| p.review_queue.len()).sum();
            let message = format!("Daily digest: {} project(s), {pending} pending review(s).", digest.projects.len());
            let payload = build_lifecycle_payload("daily-digest", "", "", &message, "idle");
            let payload = LifecycleHookPayload {
                digest: Some(serde_json::to_value(&digest).unwrap_or(serde_json::Value::Null)),
                ..payload
            };
            let _ = super::hooks::run_lifecycle_hooks(
                &config,
                None,
                &payload,
                Some(&daemon_log_path(None).to_string_lossy()),
            );
        }
    }

    if normalized_state.budget_spent_usd >= config.budgets.daily_cost_usd {
        let blocked_state = persist_tick_state(None, Some(&now_iso()), Some("budget-blocked"), &|state| {
            state.budget_blocked = true;
            state.active_project = None;
            state.current_run = None;
        });
        let message = format!("Daily budget exhausted at ${:.4}.", normalized_state.budget_spent_usd);
        let payload = build_lifecycle_payload("budget-blocked", "", "", &message, "idle");
        let payload = LifecycleHookPayload {
            budget_snapshot_usd: Some(normalized_state.budget_spent_usd),
            ..payload
        };
        let _ = super::hooks::run_lifecycle_hooks(
            &config,
            None,
            &payload,
            Some(&daemon_log_path(None).to_string_lossy()),
        );
        let _ = super::notifications::fire_notifications(&config, &payload);
        if let Some(shell_command) = config
            .notifications
            .as_ref()
            .and_then(|n| n.on_budget_blocked.clone())
        {
            std::thread::spawn(move || {
                fire_shell_command(&shell_command, "budget-blocked", "", "");
            });
        }
        return blocked_state;
    }

    // Daemon-native schedules: evaluate every initialized project's schedules
    // each tick. Schedules only add tasks — they never preempt active work.
    for project in &projects {
        if !project.initialized {
            continue;
        }
        if let Err(error) = evaluate_schedules(project) {
            append_log_fmt(&format!(
                "schedule evaluation failed in {}: {}",
                project.alias, error
            ));
        }
    }

    let active_project = super::project_selection::select_next_project(None)?;

    // Review backpressure: a project whose pending promotions pile up must be
    // reviewed before more agent runs land on top of them.
    if let Some(active_project) = &active_project {
        let max_pending_reviews = config.runtime.max_pending_reviews_per_project.unwrap_or(3);
        if max_pending_reviews > 0 {
            let pending_count = super::promotion_queue::list_promotion_artifacts(Path::new(&active_project.path))?
                .into_iter()
                .filter(|item| item.artifact.status == PromotionArtifactStatus::Pending)
                .count();
            if pending_count >= max_pending_reviews as usize {
                let backpressure_state = persist_tick_state(
                    Some(&active_project.alias),
                    Some(&now_iso()),
                    Some("review-backpressure"),
                    &|state| {
                        state.active_project = Some(active_project.alias.clone());
                        state.current_run = None;
                    },
                );
                append_log_fmt(&format!(
                    "review-backpressure: {pending_count} pending promotion(s) >= {max_pending_reviews} in {}",
                    active_project.alias
                ));
                let message = format!(
                    "Skipping {}: {pending_count} pending promotions awaiting review (limit {max_pending_reviews}).",
                    active_project.alias
                );
                let payload = build_lifecycle_payload(
                    "review-backpressure",
                    &active_project.alias,
                    "",
                    &message,
                    "idle",
                );
                let payload = LifecycleHookPayload {
                    pending_promotions: Some(pending_count),
                    ..payload
                };
                let _ = super::hooks::run_lifecycle_hooks(
                    &config,
                    None,
                    &payload,
                    Some(&daemon_log_path(None).to_string_lossy()),
                );
                return backpressure_state;
            }
        }
    }

    let mut current_run: Option<DaemonCurrentRunState> = None;

    if let Some(active_project) = &active_project {
        let ledger = super::task_ledger::load_task_ledger(Path::new(&active_project.path))?;
        let selection = super::scheduler::select_next_task(&ledger);
        if let Some(task) = selection.task {
            if selection.mode != "idle" {
                let started_at_run = now_iso();
                let deadline_at = format!(
                    "{}",
                    chrono::Utc
                        .timestamp_millis_opt(now_millis() + (config.runtime.run_timeout_seconds as i64) * 1000)
                        .single()
                        .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
                        .unwrap_or_default()
                );
                current_run = Some(DaemonCurrentRunState {
                    project_alias: active_project.alias.clone(),
                    task_id: Some(task.id.clone()),
                    mode: selection.mode.clone(),
                    role: super::scheduler::determine_worker_role(&task, &selection.mode)
                        .as_str()
                        .to_string(),
                    started_at: started_at_run,
                    deadline_at,
                    attempt_number: task.attempts + 1,
                    pause_requested_at: normalized_state.paused_at.clone(),
                });
            }
        }
    }

    persist_tick_state(
        active_project.as_ref().map(|p| p.alias.as_str()),
        None,
        None,
        &|state| {
            state.active_project = active_project.as_ref().map(|p| p.alias.clone());
            state.current_run = current_run.clone();
        },
    )?;

    let mut iteration_result: Option<String> = None;
    let mut charge_usd = 0.0f64;

    if let Some(active_project) = &active_project {
        let run_result: Result<SchedulerResult, String> = (|| {
            let mut hooks = super::scheduler::RunHooks {
                pi_runner: None,
                reviewer_runner: None,
                verifier_runner: None,
                validation_runner: None,
            };
            super::scheduler::run_project_iteration(
                active_project,
                &super::scheduler::IterationOptions {
                    model_override: None,
                    timeout_ms: Some(config.runtime.run_timeout_seconds * 1000),
                    max_attempts_per_task: Some(config.runtime.max_attempts_per_task),
                    no_progress_repeat_limit: Some(config.runtime.no_progress_repeat_limit),
                },
                &mut hooks,
            )
        })();

        match run_result {
            Ok(result) => {
                iteration_result = Some(format!(
                    "{}:{}:{}:{}",
                    result.mode,
                    result.task_id.clone().unwrap_or_else(|| "none".into()),
                    result
                        .exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "idle".into()),
                    result.stopped_by.as_str()
                ));
                if result.exit_code.is_some() {
                    // Measured provider cost when parsed, else the configured estimate.
                    let cost = result
                        .cost_usd
                        .unwrap_or(config.budgets.estimated_cost_per_run_usd.unwrap_or(0.0));
                    charge_usd = cost;

                    // B3: Accumulate estimated cost on the task itself.
                    if let Some(task_id) = &result.task_id {
                        if cost > 0.0 {
                            let _ = super::task_ledger::with_task_ledger(
                                Path::new(&active_project.path),
                                |ledger| {
                                    if let Some(task) =
                                        ledger.tasks.iter_mut().find(|t| &t.id == task_id)
                                    {
                                        task.estimated_cost_usd = Some(
                                            ((task.estimated_cost_usd.unwrap_or(0.0) + cost) * 10000.0).round()
                                                / 10000.0,
                                        );
                                        task.updated_at = now_iso();
                                    }
                                    Ok(())
                                },
                            );
                        }
                    }
                }
                // D3: Append structured event to events.jsonl audit trail.
                let mut event = now_event(if result.exit_code.is_some() {
                    "pi_completed"
                } else if result.mode == "idle" {
                    "idle"
                } else {
                    "task_skipped"
                });
                event.project = Some(active_project.alias.clone());
                event.task_id = result.task_id.clone();
                event.exit_code = result.exit_code;
                event.mode = Some(result.mode.clone());
                event.role = result.role.map(|r| r.as_str().to_string());
                event.stopped_by = Some(result.stopped_by.as_str().to_string());
                event.task_status = result.task_status.map(|s| s.as_str().to_string());
                let _ = append_event(&event, None);

                let proj_config = super::project_config::load_project_config(Path::new(&active_project.path)).ok();
                let _ = emit_lifecycle_hooks_and_notifications(
                    &config,
                    proj_config.as_ref(),
                    &active_project.path,
                    &result,
                    &active_project.alias,
                );

                // A2: Post task status back to linked issue (if configured)
                if let (Some(task_id), Some(task_status)) = (result.task_id.clone(), result.task_status) {
                    let project_path = active_project.path.clone();
                    std::thread::spawn(move || {
                        let _ = super::issue_sync::post_task_status_to_issue(
                            Path::new(&project_path),
                            &task_id,
                            task_status.as_str(),
                            None,
                        );
                    });
                }

                // A2: Post PR link back to issue when a promotion creates a PR
                if let (Some(task_id), Some(artifact_path)) = (&result.task_id, &result.promotion_result_artifact_path) {
                    if let Some(artifact) =
                        super::promotion_artifacts::read_promotion_result_artifact(artifact_path)
                    {
                        if let Some(pr_url) = artifact.pr_url {
                            let project_path = active_project.path.clone();
                            let task_id = task_id.clone();
                            let branch = artifact.branch.clone();
                            std::thread::spawn(move || {
                                let _ = super::issue_sync::post_pr_link_to_issue(
                                    Path::new(&project_path),
                                    &task_id,
                                    &pr_url,
                                    branch.as_deref(),
                                );
                            });
                        }
                    }
                }

                // A2: Auto-sync issues if configured
                if let Some(proj_config) = &proj_config {
                    if let Some(source) = &proj_config.issue_source {
                        if source.auto_sync.unwrap_or(false) && source.token.is_some() {
                            let last_synced = source
                                .last_synced_at
                                .as_deref()
                                .map(super::parse_iso_millis)
                                .unwrap_or(0);
                            let interval_ms =
                                (source.sync_interval_minutes.unwrap_or(30) as i64) * 60_000;
                            if now_millis() - last_synced >= interval_ms {
                                let project_path = active_project.path.clone();
                                let source = source.clone();
                                std::thread::spawn(move || {
                                    let _ = super::issue_sync::sync_issues(
                                        Path::new(&project_path),
                                        &source,
                                    );
                                });
                            }
                        }
                    }
                }
            }
            Err(error) => {
                iteration_result = Some(error);
                // Handled below: timeout charges the estimate.
            }
        }
    }

    // A timed-out run consumed budget (no parseable usage) — charge the estimate.
    if let Some(err) = &iteration_result {
        if err.starts_with("__iteration_timeout__") || err.contains("Run exceeded timeout") {
            charge_usd = config.budgets.estimated_cost_per_run_usd.unwrap_or(0.0);
        }
    }

    let post_run_state = persist_tick_state(
        active_project.as_ref().map(|p| p.alias.as_str()),
        Some(&now_iso()),
        iteration_result.as_deref(),
        &|state| {
            state.active_project = active_project.as_ref().map(|p| p.alias.clone());
            state.current_run = None;
            charge_budget(state, charge_usd, started_at);
        },
    )?;
    Ok(post_run_state)
}

/// Apply a budget charge as a DELTA to freshly loaded state (never a stale
/// snapshot), rolling over the daily window when the calendar day changed.
fn charge_budget(state: &mut DaemonState, cost_usd: f64, _now: &str) {
    let current_date = local_date_stamp();
    if state.budget_date != current_date {
        state.budget_date = current_date;
        state.budget_spent_usd = 0.0;
        state.budget_blocked = false;
    }
    if cost_usd > 0.0 {
        state.budget_spent_usd = ((state.budget_spent_usd + cost_usd) * 10000.0).round() / 10000.0;
        state.total_budget_spent_usd =
            ((state.total_budget_spent_usd + cost_usd) * 10000.0).round() / 10000.0;
    }
}

fn normalize_daemon_budget_state(mut state: DaemonState, _now: &str) -> DaemonState {
    let current_date = local_date_stamp();
    if state.budget_date == current_date {
        return state;
    }
    state.budget_date = current_date;
    state.budget_spent_usd = 0.0;
    state.budget_blocked = false;
    state
}

pub fn build_lifecycle_payload(
    event: &str,
    project: &str,
    task_id: &str,
    message: &str,
    mode: &str,
) -> LifecycleHookPayload {
    LifecycleHookPayload {
        event: event.to_string(),
        project: project.to_string(),
        task_id: task_id.to_string(),
        message: message.to_string(),
        timestamp: now_iso(),
        mode: Some(mode.to_string()),
        exit_code: None,
        task_status: None,
        role: None,
        stopped_by: None,
        promotion_decision: None,
        promotion_action: None,
        budget_snapshot_usd: None,
        validation: Vec::new(),
        approval_packet_path: None,
        digest: None,
        pending_promotions: None,
        failed_checks: None,
        extra: Default::default(),
    }
}

pub fn fire_shell_command(command: &str, event: &str, project: &str, task_id: &str) {
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("OPENLOOP_EVENT", event)
        .env("OPENLOOP_PROJECT", project)
        .env("OPENLOOP_TASK_ID", task_id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn emit_lifecycle_hooks_and_notifications(
    config: &GlobalConfig,
    project_config: Option<&ProjectConfig>,
    project_path: &str,
    result: &SchedulerResult,
    project_alias: &str,
) -> Result<(), String> {
    let mut result = result.clone();
    let mut hook_notes: Vec<String> = Vec::new();
    let mut require_manual_review = false;

    for payload in build_lifecycle_payloads(&result, project_alias) {
        let hook_result = super::hooks::run_lifecycle_hooks(
            config,
            project_config,
            &payload,
            Some(&daemon_log_path(None).to_string_lossy()),
        );
        for note in &hook_result.notes {
            hook_notes.push(format!("{}: {note}", payload.event));
        }
        require_manual_review = require_manual_review || hook_result.require_manual_review;
    }

    if require_manual_review {
        if let Some(task_id) = &result.task_id {
            if result.promotion_action == PromotionAction::QueueAutoMerge {
                let downgraded = super::promotion_queue::downgrade_pending_auto_merge_promotion_to_review(
                    Path::new(project_path),
                    task_id,
                    "Manual review required by lifecycle hook.",
                )
                .unwrap_or(false);
                if downgraded {
                    result.promotion_decision = PromotionDecision::ManualReview;
                    result.promotion_action = PromotionAction::QueueReview;
                    hook_notes.push(
                        "promotion-auto-merge-queued: Downgraded to manual review by lifecycle hook."
                            .to_string(),
                    );
                }
            }
        }
    }

    for payload in build_lifecycle_payloads(&result, project_alias) {
        let _ = super::notifications::fire_notifications(config, &payload);
    }
    persist_hook_notes(project_path, &result, &hook_notes);
    fire_notification(config, &result, project_alias);
    Ok(())
}

pub fn build_lifecycle_payloads(result: &SchedulerResult, project_alias: &str) -> Vec<LifecycleHookPayload> {
    let base = |event: &str, message: &str| -> LifecycleHookPayload {
        LifecycleHookPayload {
            event: event.to_string(),
            project: project_alias.to_string(),
            task_id: result.task_id.clone().unwrap_or_default(),
            message: message.to_string(),
            timestamp: now_iso(),
            mode: Some(result.mode.clone()),
            exit_code: result.exit_code,
            task_status: result.task_status.map(|s| s.as_str().to_string()),
            role: result.role.map(|r| r.as_str().to_string()),
            stopped_by: Some(result.stopped_by.as_str().to_string()),
            promotion_decision: Some(result.promotion_decision.as_str().to_string()),
            promotion_action: Some(result.promotion_action.as_str().to_string()),
            budget_snapshot_usd: result.budget_snapshot_usd,
            validation: result.validation.clone(),
            approval_packet_path: result.approval_packet_path.clone(),
            digest: None,
            pending_promotions: None,
            failed_checks: None,
            extra: Default::default(),
        }
    };

    let mut payloads = Vec::new();
    let task_id_display = result.task_id.clone().unwrap_or_else(|| "unknown".into());

    match result.task_status {
        Some(TaskStatus::Done) => payloads.push(base(
            "task-complete",
            &format!("Task {task_id_display} completed in project {project_alias}"),
        )),
        Some(TaskStatus::Failed) => payloads.push(base(
            "task-failed",
            &format!("Task {task_id_display} failed in project {project_alias}"),
        )),
        Some(TaskStatus::Blocked) => payloads.push(base(
            "task-failed",
            &format!("Task {task_id_display} blocked in project {project_alias}"),
        )),
        Some(TaskStatus::AwaitingApproval) => payloads.push(base(
            "task-awaiting-approval",
            &format!("Task {task_id_display} is awaiting approval in project {project_alias}"),
        )),
        _ if result.mode == "idle" => payloads.push(base(
            "all-tasks-done",
            &format!("All tasks done in project {project_alias}"),
        )),
        _ => {}
    }

    if result.task_status == Some(TaskStatus::Failed) && result.validation.iter().any(|item| item.exit_code != 0) {
        payloads.push(base(
            "validation-failed",
            &format!("Validation failed for task {task_id_display} in project {project_alias}"),
        ));
    }

    match result.promotion_action {
        PromotionAction::QueueAutoMerge => payloads.push(base(
            "promotion-auto-merge-queued",
            &format!("Promotion queued for auto-merge for task {task_id_display} in project {project_alias}"),
        )),
        PromotionAction::QueueReview => payloads.push(base(
            "promotion-review-queued",
            &format!("Promotion queued for review for task {task_id_display} in project {project_alias}"),
        )),
        PromotionAction::Block => payloads.push(base(
            "promotion-blocked",
            &format!("Promotion blocked for task {task_id_display} in project {project_alias}"),
        )),
        PromotionAction::None => {}
    }

    payloads
}

fn persist_hook_notes(project_path: &str, result: &SchedulerResult, notes: &[String]) {
    if notes.is_empty() {
        return;
    }

    if let Some(task_id) = &result.task_id {
        let _ = super::task_ledger::with_task_ledger(Path::new(project_path), |ledger| {
            if let Some(task) = ledger.tasks.iter_mut().find(|candidate| &candidate.id == task_id) {
                let mut task_notes = task.notes.clone().unwrap_or_default();
                for note in notes {
                    task_notes.push(format!("Hook: {note}"));
                }
                task.notes = Some(task_notes);
                task.updated_at = now_iso();
            }
            Ok(())
        });
    }

    if let Some(run_summary_path) = &result.run_summary_path {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(run_summary_path) {
            let _ = write!(f, "\n## Hook Notes\n");
            for note in notes {
                let _ = write!(f, "- {note}\n");
            }
        }
    }
}

fn fire_notification(config: &GlobalConfig, result: &SchedulerResult, project_alias: &str) {
    let env_for = |event: &str| {
        (
            event.to_string(),
            project_alias.to_string(),
            result.task_id.clone().unwrap_or_default(),
        )
    };

    let notifications = config.notifications.clone().unwrap_or_default();
    if result.task_status == Some(TaskStatus::Done) {
        if let Some(command) = &notifications.on_task_complete {
            let (_, project, task_id) = env_for("task-complete");
            fire_shell_command(command, "task-complete", &project, &task_id);
        }
    } else if result.task_status == Some(TaskStatus::Failed) || result.task_status == Some(TaskStatus::Blocked) {
        if let Some(command) = &notifications.on_task_failed {
            let (_, project, task_id) = env_for("task-failed");
            fire_shell_command(command, "task-failed", &project, &task_id);
        }
    } else if result.mode == "idle" {
        if let Some(command) = &notifications.on_all_tasks_done {
            let (_, project, task_id) = env_for("all-tasks-done");
            fire_shell_command(command, "all-tasks-done", &project, &task_id);
        }
    }
}

const SCHEDULE_MAX_CATCHUP_MINUTES: i64 = 7 * 24 * 60; // bound catch-up after long downtime

fn to_minute_iso(epoch_millis: i64) -> String {
    let floored = (epoch_millis / 60000) * 60000;
    chrono::Utc
        .timestamp_millis_opt(floored)
        .single()
        .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default()
}

/// Fire due schedules for one project: enqueue a task for every schedule whose
/// cron matched in (lastFiredMinute, nowMinute], deduplicating against tasks
/// already created for that schedule after the last fire.
fn evaluate_schedules(project: &LinkedProject) -> Result<(), String> {
    let project_path = Path::new(&project.path);
    let mut config = super::project_config::load_project_config(project_path)?;
    let schedules = config.schedule.clone().unwrap_or_default();
    if schedules.is_empty() {
        return Ok(());
    }

    let now = now_millis();
    let now_minute = to_minute_iso(now);
    let mut schedule_state = config.schedule_state.clone().unwrap_or_default();
    let mut mutated = false;

    for schedule in &schedules {
        let result = evaluate_single_schedule(project_path, project, now, &now_minute, schedule, &mut schedule_state);
        match result {
            Ok(fired) => {
                mutated = mutated || fired;
            }
            Err(error) => {
                // A malformed cron must not poison the project's other schedules.
                append_log_fmt(&format!(
                    "schedule {} failed in {}: {}",
                    schedule.id, project.alias, error
                ));
            }
        }
    }

    if mutated {
        config.schedule_state = Some(schedule_state);
        super::project_config::save_project_config(project_path, &config)?;
    }
    Ok(())
}

fn evaluate_single_schedule(
    project_path: &Path,
    project: &LinkedProject,
    now: i64,
    now_minute: &str,
    schedule: &ProjectSchedule,
    schedule_state: &mut std::collections::BTreeMap<String, String>,
) -> Result<bool, String> {
    let Some(last_fired) = schedule_state.get(&schedule.id).cloned() else {
        // Fresh installs don't backfill — arm the schedule from now on.
        schedule_state.insert(schedule.id.clone(), now_minute.to_string());
        return Ok(true);
    };

    let last_fired_ms = super::parse_iso_millis(&last_fired);
    let catchup_start = std::cmp::max(
        last_fired_ms,
        now - SCHEDULE_MAX_CATCHUP_MINUTES * 60_000,
    );
    let mut fired = false;
    let mut ts = catchup_start + 60_000;
    while ts <= now {
        if super::cron::matches_five_field_cron(&schedule.cron, ts)? {
            fired = true;
            break;
        }
        ts += 60_000;
    }
    if !fired {
        return Ok(false);
    }

    super::task_ledger::with_task_ledger(project_path, |ledger| {
        let already_queued = ledger.tasks.iter().any(|task| {
            task.source.source_type == "schedule"
                && task.source.r#ref == schedule.id
                && task.created_at.as_str() > last_fired.as_str()
        });
        if already_queued {
            return Ok(());
        }
        let now_iso = now_iso();
        ledger.tasks.push(ProjectTask {
            id: format!("{}-{}", schedule.id, now / 60000),
            title: schedule.title.clone(),
            kind: TaskKind::Feature,
            status: TaskStatus::Proposed,
            risk: TaskRisk::MediumRisk, // conservative default for unattended scheduled work
            scope: None,
            source: TaskSource {
                source_type: "schedule".to_string(),
                r#ref: schedule.id.clone(),
            },
            spec_id: None,
            branch: None,
            owner: Some("openloop".to_string()),
            acceptance_criteria: schedule
                .prompt
                .as_deref()
                .map(|p| vec![p.to_string()])
                .unwrap_or_else(|| vec!["To be defined during planning.".to_string()]),
            attempts: 0,
            last_failure_signature: None,
            promotion: TaskPromotion::PullRequest,
            promoted_at: None,
            depends_on: None,
            estimated_cost_usd: None,
            notes: Some(vec![format!("Created by schedule {} ({}).", schedule.id, schedule.cron)]),
            last_run: None,
            created_at: now_iso.clone(),
            updated_at: now_iso,
        });
        Ok(())
    })?;
    schedule_state.insert(schedule.id.clone(), now_minute.to_string());
    let mut event = now_event("schedule_fired");
    event.project = Some(project.alias.clone());
    event.task_id = Some(schedule.id.clone());
    let _ = append_event(&event, None);
    Ok(true)
}

pub fn recover_stuck_tasks() -> Result<(), String> {
    let projects = match super::project_registry::list_projects(None) {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    for project in projects {
        let result = (|| -> Result<usize, String> {
            let project_path = Path::new(&project.path);
            let recovered = super::task_ledger::with_task_ledger(project_path, |ledger| {
                let mut count = 0;
                for task in ledger.tasks.iter_mut() {
                    if task.status != TaskStatus::InProgress {
                        continue;
                    }
                    task.status = TaskStatus::Ready;
                    let mut notes = task.notes.clone().unwrap_or_default();
                    notes.push("Openloop recovered task from in_progress on daemon startup.".to_string());
                    task.notes = Some(notes);
                    task.updated_at = now_iso();
                    count += 1;
                }
                Ok(count)
            })?;
            if recovered > 0 {
                append_log_fmt(&format!("recovered {recovered} stuck task(s) in {}", project.alias));
            }
            reclaim_stale_worktrees(project_path, &project)?;
            Ok(recovered)
        })();
        // ignore per-project errors during recovery
        let _ = result;
    }
    Ok(())
}

fn is_pid_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

// Reclaim worktrees whose task is no longer in_progress: a crashed run leaves
// the checkout behind and wedges the next iteration's worktree add.
fn reclaim_stale_worktrees(project_path: &Path, project: &LinkedProject) -> Result<(), String> {
    let worktrees_dir = project_path.join(".openloop").join("worktrees");
    let entries = match std::fs::read_dir(&worktrees_dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    let config = super::project_config::load_project_config(project_path).ok();
    let branch_prefix = config
        .map(|c| c.runtime.branch_prefix)
        .unwrap_or_else(|| "openloop/".to_string());
    let ledger = super::task_ledger::load_task_ledger(project_path)?;

    for entry in entries.flatten() {
        if !entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
            continue;
        }
        let task_id = entry.file_name().to_string_lossy().to_string();
        let worktree_path = worktrees_dir.join(&task_id);

        // A concurrent CLI run or second daemon writes a pid marker at worktree
        // creation; never force-remove a checkout whose owner is still alive.
        let owner_pid = std::fs::read_to_string(worktree_path.join(".openloop-run.pid"))
            .ok()
            .and_then(|raw| raw.trim().parse::<i32>().ok());
        if let Some(pid) = owner_pid {
            if is_pid_alive(pid) {
                continue; // live cross-process run — leave it alone
            }
        }

        let task = ledger.tasks.iter().find(|candidate| candidate.id == task_id);
        if task.map(|t| t.status == TaskStatus::InProgress).unwrap_or(false) {
            continue; // live run — leave it alone
        }

        super::git::remove_worktree(project_path, &worktree_path, true);
        // Remove strays that git doesn't know about (never a registered worktree).
        let _ = std::fs::remove_dir_all(&worktree_path);
        // Abandoned tasks also drop their branch; done/awaiting tasks may hold
        // unpromoted work — keep theirs.
        if let Some(task) = task {
            if task.status == TaskStatus::Blocked || task.status == TaskStatus::Cancelled {
                let _ = super::git::delete_branch(project_path, &format!("{branch_prefix}{task_id}"));
            }
        }
        append_log_fmt(&format!("reclaimed stale worktree for {task_id} in {}", project.alias));
    }
    Ok(())
}

