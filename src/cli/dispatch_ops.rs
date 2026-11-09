use std::path::{Path, PathBuf};

use super::*;
use crate::core as c;
use c::types::*;

// status / events / watch

pub(super) fn status(args: StatusArgs) -> Result<(), String> {
    let projects = c::project_registry::list_projects(None)?;
    let daemon_state = c::daemon_state::load_daemon_state(None).unwrap_or_default();
    // Live "why is this not running" per project — the same gates the tick
    // applies, evaluated now rather than replayed from the last tick.
    let idle = c::idle_reasons::analyze().ok();

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
        let why = idle
            .as_ref()
            .and_then(|idle| idle.projects.iter().find(|p| p.alias == project.alias))
            .map(|p| p.kind.clone())
            .unwrap_or_else(|| "--".to_string());

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
                why,
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
                    "why",
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
                        "why": cells[8],
                    })
                })
                .collect();
            let idle_projects: Vec<serde_json::Value> = idle
                .as_ref()
                .map(|idle| {
                    idle.projects
                        .iter()
                        .map(|p| serde_json::to_value(p).unwrap_or(serde_json::Value::Null))
                        .collect()
                })
                .unwrap_or_default();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "daemon": {
                        "pid": daemon_state.pid,
                        "paused": daemon_state.paused,
                        "budgetSpentUsd": daemon_state.budget_spent_usd,
                        "activeProject": daemon_state.active_project,
                    },
                    "idleReasons": {
                        "daemon": idle.as_ref().and_then(|i| i.daemon.as_ref())
                            .and_then(|d| serde_json::to_value(d).ok()),
                        "projects": idle_projects,
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
    // checked chain: an absurd input ("999999999999999d") must not panic
    // (debug) or wrap (release) — fall back to the default window.
    match unit {
        "m" => value.checked_mul(60).and_then(|v| v.checked_mul(1000)),
        "h" => value
            .checked_mul(60)
            .and_then(|v| v.checked_mul(60))
            .and_then(|v| v.checked_mul(1000)),
        "d" => value
            .checked_mul(24)
            .and_then(|v| v.checked_mul(60))
            .and_then(|v| v.checked_mul(60))
            .and_then(|v| v.checked_mul(1000)),
        _ => None,
    }
    .unwrap_or(60 * 60 * 1000)
}

pub(super) fn events(args: EventsArgs) -> Result<(), String> {
    if args.follow {
        return events_follow(&args);
    }
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

/// `events --follow`: print the initial snapshot, then stream appended
/// events.jsonl lines until interrupted (Ctrl-C). Truncation/rotation resets
/// to the file start.
fn events_follow(args: &EventsArgs) -> Result<(), String> {
    use std::io::Write as _;

    let log_path = c::event_log::events_log_path(None);
    let project_filter = args.project.as_deref();

    fn render(line: &str, json: bool) {
        if json {
            println!("{line}");
        } else if let Ok(evt) = serde_json::from_str::<serde_json::Value>(line) {
            let ts: String = evt["ts"]
                .as_str()
                .unwrap_or("")
                .chars()
                .skip(11)
                .take(8)
                .collect();
            let event = evt["event"].as_str().unwrap_or("?");
            let proj = evt["project"]
                .as_str()
                .map(|p| format!(" [{p}]"))
                .unwrap_or_default();
            let task = evt["taskId"]
                .as_str()
                .map(|t| format!(" #{t}"))
                .unwrap_or_default();
            println!("{ts} {event}{proj}{task}");
        }
    }

    fn matches_filter(line: &str, project: Option<&str>) -> bool {
        match project {
            None => true,
            Some(alias) => serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| v["project"].as_str().map(|p| p == alias))
                .unwrap_or(false),
        }
    }

    // Initial snapshot (same window/filter semantics as the one-shot read).
    let initial = c::event_log::read_recent_events(&c::event_log::ReadEventsOptions {
        since_ms: Some(parse_since_ms(&args.since)),
        project: project_filter,
        limit: (args.limit != 0).then_some(args.limit),
        ..Default::default()
    });
    for evt in &initial {
        let line = serde_json::to_string(evt).unwrap_or_default();
        render(&line, args.json);
    }
    let _ = std::io::stdout().flush();

    // Stream: track how many lines of the current file we have printed.
    let mut printed_lines: usize = std::fs::read_to_string(&log_path)
        .map(|content| content.lines().count())
        .unwrap_or(0);

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let content = match std::fs::read_to_string(&log_path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() < printed_lines {
            // Truncated or rotated: re-read from the start.
            printed_lines = 0;
        }
        for line in lines.iter().skip(printed_lines) {
            if line.trim().is_empty() {
                continue;
            }
            if matches_filter(line, project_filter) {
                render(line, args.json);
            }
        }
        printed_lines = lines.len();
        let _ = std::io::stdout().flush();
    }
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
        .map(|p| {
            c::fsutil::normalize_lexical_path(&p)
                .to_string_lossy()
                .to_string()
        })
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
