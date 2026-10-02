use std::path::Path;
use std::path::PathBuf;

use super::*;
use crate::core as c;
use c::types::*;

// ---------------------------------------------------------------------------
// task / approval
// ---------------------------------------------------------------------------

fn slugify_title(title: &str, max: usize) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in title.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            slug.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(max).collect();
    if slug.is_empty() {
        format!("task-{}", c::now_millis())
    } else {
        slug
    }
}

pub(super) fn task(cmd: TaskCommand) -> Result<(), String> {
    match cmd.command {
        TaskSubcommand::Add {
            project,
            title,
            from_ref,
            kind,
            risk,
            scope,
            depends_on,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            let now = c::now_iso();

            let r#ref = from_ref.clone();
            let title_str = match (&title, &r#ref) {
                (Some(t), _) => t.clone(),
                (None, Some(r)) => format!("Queued work from {r}"),
                (None, None) => {
                    eprintln!("Error: provide --title or --from-ref");
                    std::process::exit(1);
                }
            };

            // W10: Validate scope paths are relative and free of traversal.
            let mut raw_scope: Vec<String> = Vec::new();
            for path in &scope {
                if !raw_scope.contains(path) {
                    raw_scope.push(path.clone());
                }
            }
            for scope_path in &raw_scope {
                if scope_path.starts_with('/') || scope_path.starts_with('\\') {
                    eprintln!("Error: scope path must be relative, got: {scope_path}");
                    std::process::exit(1);
                }
                if scope_path.split('/').any(|segment| segment == "..") {
                    eprintln!("Error: scope path must not contain '..', got: {scope_path}");
                    std::process::exit(1);
                }
            }

            let mut unique_deps: Vec<String> = Vec::new();
            for dep in &depends_on {
                if !unique_deps.contains(dep) {
                    unique_deps.push(dep.clone());
                }
            }

            let mut task = ProjectTask {
                id: slugify_title(&title_str, 64),
                title: title_str,
                kind: TaskKind::parse(&kind).ok_or_else(|| format!("Invalid kind: {kind}"))?,
                status: TaskStatus::Proposed,
                risk: TaskRisk::parse(&risk).ok_or_else(|| format!("Invalid risk: {risk}"))?,
                scope: (!raw_scope.is_empty()).then_some(TaskScope { paths: raw_scope }),
                source: match &r#ref {
                    Some(r) => TaskSource {
                        source_type: "issue".to_string(),
                        r#ref: r.clone(),
                    },
                    None => TaskSource {
                        source_type: "human".to_string(),
                        r#ref: "openloop task add".to_string(),
                    },
                },
                spec_id: None,
                branch: None,
                owner: Some("openloop".to_string()),
                acceptance_criteria: vec!["To be defined during planning.".to_string()],
                attempts: 0,
                last_failure_signature: None,
                promotion: TaskPromotion::PullRequest,
                promoted_at: None,
                depends_on: (!unique_deps.is_empty()).then_some(unique_deps),
                estimated_cost_usd: None,
                notes: if r#ref.is_some() {
                    Some(vec!["Created by openloop task add --from-ref.".to_string()])
                } else {
                    Some(Vec::new())
                },
                last_run: None,
                created_at: now.clone(),
                updated_at: now,
            };

            c::task_ledger::add_task(Path::new(&project.path), &mut task)?;
            println!("Added task {} to {}", task.id, project.alias);
            Ok(())
        }
        TaskSubcommand::List {
            project,
            status,
            risk,
            format,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            let filters = c::task_ledger::TaskListFilters {
                status: status.as_deref().and_then(TaskStatus::parse),
                risk: risk.as_deref().and_then(TaskRisk::parse),
            };
            let tasks = c::task_ledger::list_tasks(Path::new(&project.path), &filters)?;
            match c::table::resolve_output_format(format.as_deref()) {
                c::table::OutputFormat::Table => {
                    let rows: Vec<c::table::Row> = tasks
                        .iter()
                        .map(|task| {
                            c::table::Row(vec![
                                task.id.clone(),
                                task.status.as_str().to_string(),
                                task.risk.as_str().to_string(),
                                task.kind.as_str().to_string(),
                                task.promotion.as_str().to_string(),
                                task.title.chars().take(40).collect(),
                            ])
                        })
                        .collect();
                    c::table::print_table(
                        &["id", "status", "risk", "kind", "promotion", "title"],
                        &rows,
                    );
                }
                c::table::OutputFormat::Json => {
                    let summary = c::task_ledger::summarize_tasks(&tasks);
                    let mapped: Vec<serde_json::Value> = tasks
                        .iter()
                        .map(|task| {
                            serde_json::json!({
                                "id": task.id,
                                "title": task.title,
                                "status": task.status.as_str(),
                                "risk": task.risk.as_str(),
                                "kind": task.kind.as_str(),
                                "scopePaths": task.scope.as_ref().map(|s| s.paths.clone()).unwrap_or_default(),
                                "promotion": task.promotion.as_str(),
                                "updatedAt": task.updated_at,
                                "promotedAt": task.promoted_at,
                            })
                        })
                        .collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "summary": {
                                "total": summary.total,
                                "byStatus": summary.by_status,
                                "byRisk": summary.by_risk,
                            },
                            "tasks": mapped,
                        }))
                        .unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        TaskSubcommand::Show { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            let inspection =
                c::task_inspection::get_task_inspection(Path::new(&project.path), &task)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&inspection).unwrap_or_default()
            );
            Ok(())
        }
        TaskSubcommand::Update {
            project,
            task,
            title,
            status,
            risk,
            kind,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            let patch = c::task_ledger::TaskPatch {
                title,
                status: status.as_deref().and_then(TaskStatus::parse),
                risk: risk.as_deref().and_then(TaskRisk::parse),
                kind: kind.as_deref().and_then(TaskKind::parse),
                scope: None,
            };
            let updated = c::task_ledger::update_task(Path::new(&project.path), &task, &patch)?;
            println!("Updated task {}", updated.id);
            Ok(())
        }
        TaskSubcommand::Remove { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            c::task_ledger::remove_task(Path::new(&project.path), &task)?;
            println!("Removed task {task} from {}", project.alias);
            Ok(())
        }
        TaskSubcommand::Recover { project } => {
            let project = c::project_registry::get_project(&project, None)?;
            let recovered = c::task_ledger::with_task_ledger(Path::new(&project.path), |ledger| {
                let mut recovered = Vec::new();
                for task in ledger.tasks.iter_mut() {
                    if task.status != TaskStatus::InProgress {
                        continue;
                    }
                    task.status = TaskStatus::Ready;
                    let mut notes = task.notes.clone().unwrap_or_default();
                    notes.push("Manually recovered via 'task recover'.".to_string());
                    task.notes = Some(notes);
                    task.updated_at = c::now_iso();
                    recovered.push(task.id.clone());
                }
                Ok(recovered)
            })?;
            if recovered.is_empty() {
                println!("No in_progress tasks found in {}.", project.alias);
                return Ok(());
            }
            println!(
                "Recovered {} task(s) in {}: {}",
                recovered.len(),
                project.alias,
                recovered.join(", ")
            );
            Ok(())
        }
        TaskSubcommand::Approve { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            let approved_id =
                c::task_ledger::with_task_ledger(Path::new(&project.path), |ledger| {
                    let found = ledger
                        .tasks
                        .iter_mut()
                        .find(|t| t.id == task)
                        .ok_or_else(|| format!("Task {task} not found in {}.", project.alias))?;
                    if found.status != TaskStatus::AwaitingApproval {
                        return Err(format!(
                            "Task {} is not awaiting approval (current status: {}).",
                            found.id,
                            found.status.as_str()
                        ));
                    }
                    found.status = TaskStatus::Ready;
                    let mut notes = found.notes.clone().unwrap_or_default();
                    notes.push("Approved via 'task approve'.".to_string());
                    found.notes = Some(notes);
                    found.updated_at = c::now_iso();
                    Ok(found.id.clone())
                });
            match approved_id {
                Ok(approved_id) => {
                    println!(
                        "Approved task {approved_id} in {} — now ready for implementation.",
                        project.alias
                    );
                    Ok(())
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
    }
}

pub(super) fn approval(cmd: ApprovalCommand) -> Result<(), String> {
    match cmd.command {
        ApprovalSubcommand::List { project, format } => {
            let project = c::project_registry::get_project(&project, None)?;
            let packets = c::approval_packets::list_approval_packets(Path::new(&project.path))?;
            match c::table::resolve_output_format(format.as_deref()) {
                c::table::OutputFormat::Table => {
                    let rows: Vec<c::table::Row> = packets
                        .iter()
                        .map(|(_, packet)| {
                            c::table::Row(vec![
                                packet.task_id.clone(),
                                packet.risk.as_str().to_string(),
                                packet.attempts.to_string(),
                                format!("{:.4}", packet.cost_usd),
                                packet.cost_source.as_str().to_string(),
                                packet.branch.clone().unwrap_or_else(|| "-".into()),
                                packet.title.chars().take(40).collect(),
                            ])
                        })
                        .collect();
                    c::table::print_table(
                        &[
                            "taskId",
                            "risk",
                            "attempts",
                            "costUsd",
                            "costSource",
                            "branch",
                            "title",
                        ],
                        &rows,
                    );
                }
                c::table::OutputFormat::Json => {
                    let mapped: Vec<&c::approval_packets::ApprovalPacket> =
                        packets.iter().map(|(_, packet)| packet).collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&mapped).unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        ApprovalSubcommand::Show { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            match c::approval_packets::read_approval_packet(Path::new(&project.path), &task)? {
                Some(packet) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&packet).unwrap_or_default()
                    );
                    Ok(())
                }
                None => {
                    eprintln!("No approval packet for task {task} in {}.", project.alias);
                    std::process::exit(1);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// status / events / watch
// ---------------------------------------------------------------------------

pub(super) fn status(args: StatusArgs) -> Result<(), String> {
    let projects = c::project_registry::list_projects(None)?;
    let daemon_state = c::daemon_state::load_daemon_state(None).unwrap_or_default();

    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for project in &projects {
        let ledger =
            c::task_ledger::load_task_ledger(Path::new(&project.path)).unwrap_or(TaskLedger {
                version: 1,
                updated_at: String::new(),
                tasks: Vec::new(),
            });
        let tasks = &ledger.tasks;
        let count = |pred: &dyn Fn(&ProjectTask) -> bool| tasks.iter().filter(|t| pred(t)).count();
        let ready = count(&|t| t.status == TaskStatus::Ready);
        let in_progress = count(&|t| t.status == TaskStatus::InProgress);
        let blocked = count(&|t| t.status == TaskStatus::Blocked || t.status == TaskStatus::Failed);
        let done = count(&|t| t.status == TaskStatus::Done || t.status == TaskStatus::Promoted);
        let total = tasks.len();

        let project_state = daemon_state
            .projects
            .iter()
            .find(|p| p.alias == project.alias);
        let last_run = project_state
            .and_then(|p| p.last_result.clone())
            .unwrap_or_else(|| "--".to_string());
        let paused = project_state
            .map(|p| p.paused)
            .unwrap_or(daemon_state.paused);

        rows.push((
            project.alias.clone(),
            vec![
                project.initialized.to_string(),
                paused.to_string(),
                total.to_string(),
                ready.to_string(),
                in_progress.to_string(),
                blocked.to_string(),
                done.to_string(),
                last_run,
            ],
        ));
    }

    match c::table::resolve_output_format(args.format.as_deref()) {
        c::table::OutputFormat::Table => {
            if rows.is_empty() {
                println!("No linked projects. Use 'openloop project add' to register one.");
                return Ok(());
            }
            let daemon_status = if daemon_state.paused {
                "paused".to_string()
            } else {
                format!("running (PID {})", daemon_state.pid)
            };
            println!("Daemon: {daemon_status}");
            println!(
                "Budget: ${:.4} / ${:.4} today",
                daemon_state.budget_spent_usd, daemon_state.budget_spent_usd
            );
            println!();
            let table_rows: Vec<c::table::Row> = rows
                .iter()
                .map(|(alias, cells)| {
                    let mut row = vec![alias.clone()];
                    row.extend(cells.iter().cloned());
                    c::table::Row(row)
                })
                .collect();
            c::table::print_table(
                &[
                    "alias",
                    "initialized",
                    "paused",
                    "total",
                    "ready",
                    "in_progress",
                    "blocked",
                    "done",
                    "lastRun",
                ],
                &table_rows,
            );
        }
        c::table::OutputFormat::Json => {
            let mapped: Vec<serde_json::Value> = rows
                .iter()
                .map(|(alias, cells)| {
                    serde_json::json!({
                        "alias": alias,
                        "initialized": cells[0],
                        "paused": cells[1],
                        "total": cells[2],
                        "ready": cells[3],
                        "in_progress": cells[4],
                        "blocked": cells[5],
                        "done": cells[6],
                        "lastRun": cells[7],
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "daemon": {
                        "pid": daemon_state.pid,
                        "paused": daemon_state.paused,
                        "budgetSpentUsd": daemon_state.budget_spent_usd,
                        "activeProject": daemon_state.active_project,
                    },
                    "projects": mapped,
                }))
                .unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn parse_since_ms(since: &str) -> i64 {
    let bytes = since.as_bytes();
    if bytes.len() < 2 {
        return 60 * 60 * 1000;
    }
    let (digits, unit) = since.split_at(since.len() - 1);
    let Ok(value) = digits.parse::<i64>() else {
        return 60 * 60 * 1000;
    };
    match unit {
        "m" => value * 60 * 1000,
        "h" => value * 60 * 60 * 1000,
        "d" => value * 24 * 60 * 60 * 1000,
        _ => 60 * 60 * 1000,
    }
}

pub(super) fn events(args: EventsArgs) -> Result<(), String> {
    let events_list = c::event_log::read_recent_events(&c::event_log::ReadEventsOptions {
        since_ms: Some(parse_since_ms(&args.since)),
        project: args.project.as_deref(),
        limit: (args.limit != 0).then_some(args.limit),
        ..Default::default()
    });

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&events_list).unwrap_or_default()
        );
        return Ok(());
    }

    if events_list.is_empty() {
        println!("No events found in the last {}.", args.since);
        return Ok(());
    }

    for evt in &events_list {
        let ts: String = evt.ts.chars().skip(11).take(8).collect();
        let proj = evt
            .project
            .as_deref()
            .map(|p| format!("[{p}]"))
            .unwrap_or_default();
        let task = evt
            .task_id
            .as_deref()
            .map(|t| format!("#{t}"))
            .unwrap_or_default();
        let mut detail_parts = vec![evt.event.clone(), proj, task];
        detail_parts.retain(|p| !p.is_empty());
        println!("{ts} {}", detail_parts.join(" "));
    }
    Ok(())
}

pub(super) fn watch() -> Result<(), String> {
    if !c::table::stdout_is_tty() {
        eprintln!("Error: 'watch' requires a TTY. Use 'status --format json' for scripted use.");
        std::process::exit(1);
    }

    const REFRESH_MS: u64 = 1000;
    const CLEAR: &str = "\x1b[2J\x1b[H";
    const BOLD: &str = "\x1b[1m";
    const DIM: &str = "\x1b[2m";
    const RESET: &str = "\x1b[0m";
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";
    const CYAN: &str = "\x1b[36m";

    fn pad_right(s: &str, len: usize) -> String {
        if s.chars().count() >= len {
            s.chars().take(len).collect()
        } else {
            format!("{s}{}", " ".repeat(len - s.chars().count()))
        }
    }

    let elapsed = |from: &str| -> String {
        let ms = c::now_millis() - c::parse_iso_millis(from);
        if ms < 60_000 {
            format!("{}s", (ms as f64 / 1000.0).round() as i64)
        } else if ms < 3_600_000 {
            format!("{}m", (ms as f64 / 60_000.0).round() as i64)
        } else {
            format!("{}h", (ms as f64 / 3_600_000.0).round() as i64)
        }
    };

    print!("{CLEAR}");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    extern "C" fn watch_sigint(_sig: i32) {
        // TS parity: reset attributes + newline before exiting.
        unsafe {
            let msg = b"\x1b[0m\n";
            libc::write(1, msg.as_ptr() as *const libc::c_void, msg.len());
        }
        std::process::exit(0);
    }
    unsafe {
        libc::signal(libc::SIGINT, watch_sigint as extern "C" fn(i32) as usize);
    }

    loop {
        let state = c::daemon_state::load_daemon_state(None).unwrap_or_default();
        let projects = c::project_registry::list_projects(None).unwrap_or_default();

        let mut lines: Vec<String> = Vec::new();

        let daemon_status = if state.paused {
            format!("{YELLOW}paused{RESET}")
        } else {
            format!("{GREEN}running (PID {}){RESET}", state.pid)
        };
        let uptime = if state.started_at.is_empty() {
            String::new()
        } else {
            format!("uptime: {}", elapsed(&state.started_at))
        };
        let budget = format!("${:.4} spent today", state.budget_spent_usd);

        lines.push(format!(
            "{BOLD}OpenLoop Watch{RESET} {DIM}(Ctrl+C to exit){RESET}"
        ));
        lines.push(format!(
            "Daemon: {daemon_status}  {DIM}{uptime}  {budget}{RESET}"
        ));
        lines.push(String::new());
        lines.push(format!(
            "{BOLD}{}{}{}{}LAST RUN{RESET}",
            pad_right("PROJECT", 16),
            pad_right("QUEUE", 8),
            pad_right("ACTIVE TASK", 24),
            pad_right("STATUS", 14)
        ));
        lines.push("─".repeat(80));

        for project in &projects {
            let ledger =
                c::task_ledger::load_task_ledger(Path::new(&project.path)).unwrap_or(TaskLedger {
                    version: 1,
                    updated_at: String::new(),
                    tasks: Vec::new(),
                });
            let queue_size = ledger
                .tasks
                .iter()
                .filter(|t| {
                    matches!(
                        t.status,
                        TaskStatus::Ready | TaskStatus::Proposed | TaskStatus::Planned
                    )
                })
                .count();

            let project_state = state.projects.iter().find(|p| p.alias == project.alias);
            let is_active = state
                .current_run
                .as_ref()
                .map(|r| r.project_alias == project.alias)
                .unwrap_or(false);
            let current_run = if is_active {
                state.current_run.as_ref()
            } else {
                None
            };

            let active_task_id = current_run
                .and_then(|r| r.task_id.clone())
                .unwrap_or_else(|| "--".to_string());
            let elapsed_run = current_run
                .map(|r| format!(" ({}) elapsed", elapsed(&r.started_at)))
                .unwrap_or_default();
            let status_str = if is_active {
                format!("{GREEN}running{RESET}")
            } else if state.paused {
                format!("{YELLOW}paused{RESET}")
            } else if queue_size > 0 {
                format!("{CYAN}idle{RESET}")
            } else {
                format!("{DIM}idle{RESET}")
            };
            let last_run = project_state
                .and_then(|p| p.last_result.clone())
                .unwrap_or_else(|| "--".to_string());

            // Strip ANSI when padding the colored status (pad plain text, then re-wrap).
            let status_plain = if is_active {
                "running"
            } else if state.paused {
                "paused"
            } else {
                "idle"
            };
            let status_padded = format!(
                "{status_str}{}",
                " ".repeat(22usize.saturating_sub(status_plain.len()))
            );
            let last_run_trim: String = last_run.chars().take(24).collect();
            lines.push(format!(
                "{}{}{}{}{DIM}{}{RESET}",
                pad_right(&project.alias, 16),
                pad_right(&queue_size.to_string(), 8),
                pad_right(&format!("{active_task_id}{elapsed_run}"), 24),
                status_padded,
                last_run_trim
            ));
        }

        if projects.is_empty() {
            lines.push(format!("{DIM}No linked projects.{RESET}"));
        }

        lines.push(String::new());
        lines.push(format!(
            "{DIM}Refreshing every {}s...{RESET}",
            REFRESH_MS / 1000
        ));

        println!("{CLEAR}{}", lines.join("\n"));
        let _ = std::io::Write::flush(&mut std::io::stdout());
        std::thread::sleep(std::time::Duration::from_millis(REFRESH_MS));
    }
}

// ---------------------------------------------------------------------------
// doctor / report / digest
// ---------------------------------------------------------------------------

pub(super) fn doctor(args: DoctorArgs) -> Result<(), String> {
    let results = c::doctor::run_doctor_checks(None)?;
    match c::table::resolve_output_format(args.format.as_deref()) {
        c::table::OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&results).unwrap_or_default()
            );
        }
        c::table::OutputFormat::Table => {
            println!("{}", c::doctor::format_doctor_results(&results));
            let fails = results.iter().filter(|r| r.status == "fail").count();
            let warns = results.iter().filter(|r| r.status == "warn").count();
            println!();
            if fails > 0 {
                println!(
                    "{fails} issue(s) found. Fix errors above to ensure openloop runs correctly."
                );
                std::process::exit(1);
            } else if warns > 0 {
                println!("All critical checks passed. {warns} warning(s) noted.");
            } else {
                println!("All checks passed.");
            }
        }
    }
    Ok(())
}

pub(super) fn report(args: ReportArgs) -> Result<(), String> {
    let since_ms = (args.hours * 60.0 * 60.0 * 1000.0) as i64;
    let report = c::run_report::generate_run_report(Some(since_ms), None)?;
    match c::table::resolve_output_format(args.format.as_deref()) {
        c::table::OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_default()
            );
        }
        c::table::OutputFormat::Table => {
            println!("{}", c::run_report::format_run_report(&report));
        }
    }
    Ok(())
}

pub(super) fn digest(args: DigestArgs) -> Result<(), String> {
    let since_ms = (args.since_hours * 60.0 * 60.0 * 1000.0) as i64;
    let digest = c::digest::build_digest(Some(since_ms), None)?;
    match c::table::resolve_output_format(args.format.as_deref()) {
        c::table::OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&digest).unwrap_or_default()
            );
        }
        c::table::OutputFormat::Table => {
            for project in &digest.projects {
                println!("{}", project.alias);
                println!(
                    "  completed={} failed={} blocked={} promoted={}",
                    project.completed, project.failed, project.blocked, project.promoted
                );
                println!(
                    "  spend: measured=${} estimated=${}",
                    project.spend_usd.measured, project.spend_usd.estimated
                );
                if project.review_queue.is_empty() {
                    println!("  review queue: empty");
                    continue;
                }
                println!("  review queue:");
                for entry in &project.review_queue {
                    println!(
                        "    [{}] {} ({}) {}",
                        match entry.confidence {
                            c::digest::DigestConfidence::High => "high",
                            c::digest::DigestConfidence::Medium => "medium",
                            c::digest::DigestConfidence::Low => "low",
                        },
                        entry.task_id,
                        entry.risk,
                        entry.title.chars().take(60).collect::<String>()
                    );
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------

fn ask(question: &str, default_value: Option<&str>) -> String {
    let suffix = default_value.map(|d| format!(" [{d}]")).unwrap_or_default();
    print!("{question}{suffix}: ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    let trimmed = answer.trim().to_string();
    if trimmed.is_empty() {
        default_value.unwrap_or("").to_string()
    } else {
        trimmed
    }
}

fn confirm(question: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{question} {hint}: ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut answer = String::new();
    let _ = std::io::stdin().read_line(&mut answer);
    let val = answer.trim().to_lowercase();
    if val.is_empty() {
        default_yes
    } else {
        val == "y" || val == "yes"
    }
}

pub(super) fn setup() -> Result<(), String> {
    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║        OpenLoop Setup Wizard         ║");
    println!("╚══════════════════════════════════════╝");
    println!();

    // Step 1: Check prerequisites
    println!("Step 1/4: Checking prerequisites...");
    println!();
    let checks = c::doctor::run_doctor_checks(None)?;
    let critical: Vec<&c::doctor::CheckResult> = checks
        .iter()
        .filter(|c| ["pi", "git", "node", "Provider (pi)"].contains(&c.label.as_str()))
        .collect();
    for check in &critical {
        let icon = match check.status {
            "ok" => "✅",
            "warn" => "⚠️",
            _ => "❌",
        };
        println!("  {icon} {}: {}", check.label, check.detail);
    }
    println!();

    let has_git = critical
        .iter()
        .any(|c| c.label == "git" && c.status == "ok");
    let has_pi = critical
        .iter()
        .any(|c| (c.label == "pi" || c.label == "Provider (pi)") && c.status == "ok");

    if !has_git {
        println!("❌ git is required. Please install git and re-run 'openloop setup'.");
        return Ok(());
    }
    if !has_pi {
        println!("⚠️  'pi' binary not found. You can still set up projects, but the daemon will need an agent binary (pi, claude, aider, codex, opencode, ka, or omp) to run tasks.");
        println!();
    }

    // Step 2: Link a project
    println!("Step 2/4: Link a project");
    println!();
    let cwd = std::env::current_dir()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let project_path = ask("Project path", Some(&cwd));
    let resolved_path = std::path::absolute(&project_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(project_path.clone());

    if !Path::new(&resolved_path).exists() {
        println!("❌ Path does not exist: {resolved_path}");
        return Ok(());
    }

    let default_alias = Path::new(&resolved_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let alias = ask("Project alias", Some(&default_alias));

    if c::project_registry::project_exists(&alias, None)? {
        println!("⚠️  Project alias '{alias}' already exists. Skipping registration.");
    } else {
        let project = c::project_registry::add_project(&alias, &resolved_path, None)?;
        println!(
            "✅ Registered project {} -> {}",
            project.alias, project.path
        );
    }
    println!();

    // Step 3: Detect stack and show validation commands
    println!("Step 3/4: Detecting project stack...");
    println!();
    let detected = c::stack_detection::detect_validation_commands(Path::new(&resolved_path))?;
    println!(
        "  Lint:      {}",
        detected.lint_command.as_deref().unwrap_or("(not detected)")
    );
    println!(
        "  Test:      {}",
        detected.test_command.as_deref().unwrap_or("(not detected)")
    );
    println!(
        "  Typecheck: {}",
        detected
            .typecheck_command
            .as_deref()
            .unwrap_or("(not detected)")
    );
    println!();

    // Step 4: Initialize templates
    let should_init = confirm("Initialize control-plane templates in the project?", true);
    if should_init {
        let project = LinkedProject {
            alias: alias.clone(),
            path: resolved_path.clone(),
            default_branch: None,
            initialized: false,
            created_at: c::now_iso(),
            updated_at: c::now_iso(),
        };
        c::templates::initialize_project_from_templates(&PathBuf::from("."), &project, false)?;
        c::project_registry::mark_project_initialized(&alias, None)?;
        let project_config = c::project_config::load_project_config(Path::new(&resolved_path))?;
        println!("✅ Templates initialized. Validation config:");
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "lintCommand": project_config.validation.lint_command,
            "testCommand": project_config.validation.test_command,
            "typecheckCommand": project_config.validation.typecheck_command,
        }))
        .unwrap_or_default();
        println!("  {}", json.replace('\n', "\n  "));
    } else {
        println!("Skipped template initialization. Run 'openloop project init <alias>' later.");
    }
    println!();

    println!("╔══════════════════════════════════════╗");
    println!("║           Setup Complete!            ║");
    println!("╚══════════════════════════════════════╝");
    println!();

    let should_start_daemon = confirm("Would you like to start the daemon now?", true);
    if should_start_daemon {
        println!("Starting daemon...");
        use std::os::unix::process::CommandExt;
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("openloop"));
        let _ = std::process::Command::new(exe)
            .args(["service", "start"])
            .stdin(std::process::Stdio::null())
            .process_group(0)
            .spawn();
        println!("✅ Daemon start requested.");
    } else {
        println!("You can start the daemon later with: openloop service start");
    }
    println!();
    println!("Next steps:");
    println!("  1. Add tasks:      openloop task add --project {alias} --title \"My first task\"");
    println!("  2. Check status:   openloop status");
    println!("  3. View report:    openloop report");
    println!("  4. Health check:   openloop doctor");
    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// issue / dashboard
// ---------------------------------------------------------------------------

pub(super) fn issue(cmd: IssueCommand) -> Result<(), String> {
    match cmd.command {
        IssueSubcommand::SetSource {
            project,
            github,
            gitlab,
            label,
            token,
            auto_sync,
            post_comments,
            no_post_comments,
        } => {
            let provider = if github.is_some() {
                "github".to_string()
            } else {
                "gitlab".to_string()
            };
            let repo = github
                .or(gitlab)
                .ok_or_else(|| "Provide --github or --gitlab".to_string())?;
            let project = c::project_registry::get_project(&project, None)?;
            let mut config = c::project_config::load_project_config(Path::new(&project.path))?;
            let source = IssueSourceConfig {
                provider,
                repo,
                label: label.clone(),
                token,
                auto_sync: Some(auto_sync),
                sync_interval_minutes: Some(30),
                post_status_comments: Some(post_comments && !no_post_comments),
                last_synced_at: None,
            };
            config.issue_source = Some(source.clone());
            c::project_config::save_project_config(Path::new(&project.path), &config)?;
            println!(
                "Issue source configured for {}: {} {} (label: {})",
                project.alias, source.provider, source.repo, source.label
            );
            Ok(())
        }
        IssueSubcommand::Sync { project, token } => {
            let project = c::project_registry::get_project(&project, None)?;
            let config = c::project_config::load_project_config(Path::new(&project.path))?;
            let Some(mut source) = config.issue_source.clone() else {
                eprintln!(
                    "No issue source configured for {}. Use 'issue set-source' first.",
                    project.alias
                );
                std::process::exit(1);
            };
            if let Some(token) = token {
                source.token = Some(token);
            }
            println!(
                "Syncing {} issues from {} (label: {})...",
                source.provider, source.repo, source.label
            );
            let result = c::issue_sync::sync_issues(Path::new(&project.path), &source)?;
            println!(
                "Sync complete: {} imported, {} already synced, {} total remote issues.",
                result.imported, result.skipped, result.total
            );
            if !result.errors.is_empty() {
                eprintln!("Errors ({}):", result.errors.len());
                for err in &result.errors {
                    eprintln!("  - {err}");
                }
            }
            Ok(())
        }
        IssueSubcommand::List { project, format } => {
            let project = c::project_registry::get_project(&project, None)?;
            let issues = c::issue_sync::list_synced_issues(Path::new(&project.path))?;
            if issues.is_empty() {
                println!("No synced issues. Run 'issue sync' first.");
                return Ok(());
            }
            match c::table::resolve_output_format(format.as_deref()) {
                c::table::OutputFormat::Table => {
                    let rows: Vec<c::table::Row> = issues
                        .iter()
                        .map(|i| {
                            c::table::Row(vec![
                                format!("#{}", i.number),
                                i.title.chars().take(50).collect(),
                                i.state.clone(),
                                i.task_id.clone().unwrap_or_else(|| "--".into()),
                                i.synced_at.chars().take(10).collect(),
                            ])
                        })
                        .collect();
                    c::table::print_table(&["#", "title", "state", "taskId", "syncedAt"], &rows);
                }
                c::table::OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&issues).unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        IssueSubcommand::RemoveSource { project } => {
            let project = c::project_registry::get_project(&project, None)?;
            let mut config = c::project_config::load_project_config(Path::new(&project.path))?;
            config.issue_source = None;
            c::project_config::save_project_config(Path::new(&project.path), &config)?;
            println!("Issue source removed from {}.", project.alias);
            Ok(())
        }
    }
}

pub(super) fn dashboard(cmd: DashboardCommand) -> Result<(), String> {
    match cmd.command {
        DashboardSubcommand::Start { port } => {
            let config = c::global_config::load_global_config(None)?;
            let port = port
                .or_else(|| config.dashboard.as_ref().map(|d| d.port))
                .unwrap_or(7399);
            let _handle = c::dashboard::start_dashboard(port)?;
            println!("Dashboard running at http://127.0.0.1:{port}");
            println!("Press Ctrl+C to stop.");
            extern "C" fn dash_sigint(_sig: i32) {
                unsafe {
                    let msg = b"\x1b[0m\n";
                    libc::write(1, msg.as_ptr() as *const libc::c_void, msg.len());
                }
                std::process::exit(0);
            }
            unsafe {
                libc::signal(libc::SIGTERM, dash_sigint as extern "C" fn(i32) as usize);
                libc::signal(libc::SIGINT, dash_sigint as extern "C" fn(i32) as usize);
            }
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        DashboardSubcommand::Enable { port } => {
            let mut config = c::global_config::load_global_config(None)?;
            let effective_port = port
                .or_else(|| config.dashboard.as_ref().map(|d| d.port))
                .unwrap_or(7399);
            config.dashboard = Some(DashboardConfig {
                port: effective_port,
                enabled: true,
            });
            c::global_config::save_global_config(&config, None)?;
            println!("Dashboard enabled on port {effective_port}. It will start with the daemon.");
            Ok(())
        }
        DashboardSubcommand::Disable => {
            let mut config = c::global_config::load_global_config(None)?;
            if let Some(dashboard) = config.dashboard.as_mut() {
                dashboard.enabled = false;
            }
            c::global_config::save_global_config(&config, None)?;
            println!("Dashboard disabled.");
            Ok(())
        }
        DashboardSubcommand::Status => {
            let config = c::global_config::load_global_config(None)?;
            let dashboard = config.dashboard.unwrap_or(DashboardConfig {
                port: 7399,
                enabled: false,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&dashboard).unwrap_or_default()
            );
            Ok(())
        }
    }
}
