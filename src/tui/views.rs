use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use super::app::{elapsed_str, App};
use super::theme::status_color;
use super::ui::{bar, key_hint_line, panel, stat_card};
use crate::core as c;

/// Relative age for timestamps: "12s", "3m", "2h", "4d", or "—".
fn age_str(from: &str) -> String {
    if from.is_empty() {
        return "—".into();
    }
    let ms = c::now_millis() - c::parse_iso_millis(from);
    if ms < 0 {
        return "—".into();
    }
    if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else if ms < 3_600_000 {
        format!("{}m", ms / 60_000)
    } else if ms < 86_400_000 {
        format!("{}h", ms / 3_600_000)
    } else {
        format!("{}d", ms / 86_400_000)
    }
}

fn event_line(theme: &super::theme::Theme, event: &c::types::OpenLoopEvent) -> Line<'static> {
    let time = chrono::DateTime::parse_from_rfc3339(&event.ts)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| "??:??:??".to_string());
    let color = if event.event.contains("complete") || event.event.contains("promoted") {
        theme.green
    } else if event.event.contains("fail") || event.event.contains("error") {
        theme.red
    } else if event.event.contains("start") || event.event.contains("running") {
        theme.cyan
    } else if event.event.contains("pause") || event.event.contains("budget") {
        theme.yellow
    } else {
        theme.text
    };
    Line::from(vec![
        Span::styled(time, Style::default().fg(theme.text_dim)),
        Span::raw("  "),
        Span::styled(event.event.clone(), Style::default().fg(color)),
        Span::raw("  "),
        Span::styled(
            event.project.clone().unwrap_or_else(|| "—".into()),
            Style::default().fg(theme.accent),
        ),
        Span::raw("  "),
        Span::styled(
            event.task_id.clone().unwrap_or_default(),
            Style::default().fg(theme.text_dim),
        ),
    ])
}

/// Standard selection table: rounded panel + sticky header + scrollable rows
/// driven by the shared TableState in App.
fn selection_table<'a>(
    theme: &super::theme::Theme,
    title: &str,
    header: &'a [&'a str],
    widths: &'a [Constraint],
    rows: Vec<Row<'a>>,
    empty_message: &str,
) -> Table<'a> {
    let header_cells: Vec<Cell> = header
        .iter()
        .map(|h| {
            Cell::from(Span::styled(
                (*h).to_string(),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();
    if rows.is_empty() && !empty_message.is_empty() {
        return Table::new(
            vec![Row::new(vec![Cell::from(Span::styled(
                empty_message.to_string(),
                Style::default().fg(theme.text_dim),
            ))])],
            &[Constraint::Percentage(100)],
        )
        .block(panel(theme, title))
        .row_highlight_style(Style::default().bg(theme.bg_selected));
    }
    Table::new(rows, widths)
        .header(Row::new(header_cells))
        .block(panel(theme, title))
        .row_highlight_style(
            Style::default()
                .bg(theme.bg_selected)
                .fg(theme.accent_bright),
        )
        .highlight_symbol("▌ ")
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

pub fn dashboard(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // stat cards
            Constraint::Min(8),    // projects
            Constraint::Length(7), // recent activity
        ])
        .split(area);

    let daemon = &app.daemon;

    // --- Stat cards row ---------------------------------------------------
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(32),
            Constraint::Percentage(34),
            Constraint::Percentage(34),
        ])
        .split(chunks[0]);

    let status_label = if daemon.paused {
        "⏸ PAUSED"
    } else if daemon.pid != 0 {
        "● RUNNING"
    } else {
        "○ STOPPED"
    };
    let status_color = if daemon.paused {
        app.theme.yellow
    } else if daemon.pid != 0 {
        app.theme.green
    } else {
        app.theme.text_dim
    };
    let uptime = if daemon.started_at.is_empty() {
        "—".to_string()
    } else {
        elapsed_str(&daemon.started_at)
    };
    let tick = app
        .global_config
        .runtime
        .tick_interval_seconds
        .map(|s| format!("{s}s"))
        .unwrap_or_else(|| "30s".into());
    stat_card(
        frame,
        cards[0],
        &app.theme,
        "DAEMON",
        vec![
            Line::from(Span::styled(
                status_label,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("pid {} · uptime {}", daemon.pid, uptime),
                Style::default().fg(app.theme.text_dim),
            )),
            Line::from(Span::styled(
                format!("tick every {tick}"),
                Style::default().fg(app.theme.text_dim),
            )),
        ],
    );

    let limit = app.global_config.budgets.daily_cost_usd;
    let spent = daemon.budget_spent_usd;
    let (budget_line, gauge_color) = if limit > 0.0 {
        let pct = (spent / limit * 100.0).round() as i64;
        let color = if pct >= 100 {
            app.theme.red
        } else if pct >= 80 {
            app.theme.yellow
        } else {
            app.theme.green
        };
        (format!("${spent:.4} / ${limit:.2} · {pct}%"), color)
    } else {
        (format!("${spent:.4} today · no limit"), app.theme.green)
    };
    let gauge_width = cards[1].width.saturating_sub(6) as usize;
    stat_card(
        frame,
        cards[1],
        &app.theme,
        "BUDGET",
        vec![
            Line::from(Span::styled(
                budget_line,
                Style::default()
                    .fg(gauge_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                bar(if limit > 0.0 { spent / limit } else { 0.0 }, gauge_width),
                Style::default().fg(gauge_color),
            )),
            Line::from(Span::styled(
                if daemon.budget_blocked {
                    "blocked — over limit"
                } else {
                    "ok"
                },
                Style::default().fg(if daemon.budget_blocked {
                    app.theme.red
                } else {
                    app.theme.text_dim
                }),
            )),
        ],
    );

    let run_lines: Vec<Line> = match &app.daemon.current_run {
        Some(current) => {
            let elapsed = elapsed_str(&current.started_at);
            let task = current.task_id.clone().unwrap_or_else(|| "—".into());
            let remaining = age_str(&current.deadline_at).replace('—', "?");
            vec![
                Line::from(Span::styled(
                    format!("{} · {}", current.project_alias, task),
                    Style::default().fg(app.theme.accent_bright),
                )),
                Line::from(Span::styled(
                    format!(
                        "{} · attempt {} · {} elapsed",
                        current.mode, current.attempt_number, elapsed
                    ),
                    Style::default().fg(app.theme.text_dim),
                )),
                Line::from(Span::styled(
                    format!("deadline in {remaining}"),
                    Style::default().fg(app.theme.text_dim),
                )),
            ]
        }
        None => vec![Line::from(Span::styled(
            "idle — no active run",
            Style::default().fg(app.theme.text_dim),
        ))],
    };
    stat_card(frame, cards[2], &app.theme, "ACTIVE RUN", run_lines);

    // --- Projects table ---------------------------------------------------
    let mut rows = Vec::new();
    for project in &app.projects {
        let state = app
            .daemon
            .projects
            .iter()
            .find(|p| p.alias == project.alias);
        let is_active = app
            .daemon
            .current_run
            .as_ref()
            .map(|r| r.project_alias == project.alias)
            .unwrap_or(false);
        let queue = state
            .map(|p| p.queue_size.to_string())
            .unwrap_or_else(|| "?".into());
        let blocked = state
            .map(|p| p.blocked_tasks.to_string())
            .unwrap_or_else(|| "?".into());
        let last_result = state
            .and_then(|p| p.last_result.clone())
            .unwrap_or_else(|| "—".into());
        let last_at = state
            .and_then(|p| p.last_iteration_at.clone())
            .unwrap_or_default();
        rows.push(Row::new(vec![
            Cell::from(project.alias.clone()).style(if is_active {
                Style::default()
                    .fg(app.theme.green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.text)
            }),
            Cell::from(queue).style(Style::default().fg(
                if state.map(|p| p.queue_size).unwrap_or(0) > 0 {
                    app.theme.text
                } else {
                    app.theme.text_dim
                },
            )),
            Cell::from(blocked).style(Style::default().fg(
                if state.map(|p| p.blocked_tasks).unwrap_or(0) > 0 {
                    app.theme.red
                } else {
                    app.theme.text_dim
                },
            )),
            Cell::from(last_result).style(Style::default().fg(
                match state.and_then(|p| p.last_result.as_deref()) {
                    Some("failed") => app.theme.red,
                    Some("promoted") | Some("applied") => app.theme.green,
                    _ => app.theme.text_dim,
                },
            )),
            Cell::from(age_str(&last_at)).style(Style::default().fg(app.theme.text_dim)),
        ]));
    }
    let empty = if app.projects.is_empty() {
        "No linked projects yet — press [a] on the Projects tab to add your first repository."
    } else {
        ""
    };
    let table = selection_table(
        &app.theme,
        "Projects",
        &["PROJECT", "QUEUE", "BLOCKED", "LAST RESULT", "AGE"],
        &[
            Constraint::Percentage(28),
            Constraint::Percentage(12),
            Constraint::Percentage(14),
            Constraint::Percentage(28),
            Constraint::Percentage(18),
        ],
        rows,
        empty,
    );
    frame.render_stateful_widget(table, chunks[1], &mut app.table_state);

    // --- Recent activity --------------------------------------------------
    let activity_block = panel(&app.theme, "Recent activity");
    let inner = activity_block.inner(chunks[2]);
    frame.render_widget(activity_block, chunks[2]);
    let lines: Vec<Line> = if app.events.is_empty() {
        vec![Line::from(Span::styled(
            "No events yet — they appear here as the daemon works.",
            Style::default().fg(app.theme.text_dim),
        ))]
    } else {
        app.events
            .iter()
            .take(4)
            .map(|e| event_line(&app.theme, e))
            .collect()
    };
    frame.render_widget(Paragraph::new(lines), inner);
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

pub fn projects(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(8),
            Constraint::Length(1),
        ])
        .split(area);

    let mut rows = Vec::new();
    for project in &app.projects {
        let active = app.active_alias.as_deref() == Some(project.alias.as_str());
        let git_entry = app
            .git_states
            .iter()
            .find(|(alias, _)| alias == &project.alias);
        let git_state = git_entry.and_then(|(_, state)| state.as_ref());
        let path_exists = std::path::Path::new(&project.path).is_dir();
        let branch = git_state
            .and_then(|s| s.current_branch.clone())
            .unwrap_or_else(|| "—".into());
        let (state_label, state_color) = if !path_exists {
            ("missing".to_string(), app.theme.red)
        } else if git_entry.is_none() {
            ("…".to_string(), app.theme.text_dim)
        } else if git_state.is_none() {
            ("not a repo".to_string(), app.theme.red)
        } else if let Some(state) = git_state.filter(|s| s.dirty) {
            (format!("dirty ({})", state.changed_files), app.theme.yellow)
        } else {
            ("clean".to_string(), app.theme.green)
        };
        rows.push(Row::new(vec![
            Cell::from(if active {
                "●".into()
            } else {
                " ".to_string()
            })
            .style(Style::default().fg(if active {
                app.theme.green
            } else {
                app.theme.text_dim
            })),
            Cell::from(project.alias.clone()).style(if active {
                Style::default()
                    .fg(app.theme.accent_bright)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.text)
            }),
            Cell::from(project.path.clone()).style(Style::default().fg(app.theme.text_dim)),
            Cell::from(branch).style(Style::default().fg(app.theme.text)),
            Cell::from(state_label).style(Style::default().fg(state_color)),
            Cell::from(if project.initialized { "yes" } else { "no" }).style(Style::default().fg(
                if project.initialized {
                    app.theme.green
                } else {
                    app.theme.yellow
                },
            )),
        ]));
    }
    let table = selection_table(
        &app.theme,
        "Linked projects",
        &["", "ALIAS", "PATH", "BRANCH", "STATE", "READY"],
        &[
            Constraint::Length(2),
            Constraint::Percentage(14),
            Constraint::Percentage(34),
            Constraint::Percentage(16),
            Constraint::Percentage(16),
            Constraint::Percentage(10),
        ],
        rows,
        "No projects yet — press [a] to add one.",
    );
    frame.render_stateful_widget(table, chunks[0], &mut app.table_state);

    // Repo detail card for the selected project.
    let detail_block = panel(&app.theme, "Repo details");
    let inner = detail_block.inner(chunks[1]);
    frame.render_widget(detail_block, chunks[1]);
    let selected = app
        .projects
        .get(app.selected.min(app.projects.len().saturating_sub(1)));
    let detail: Vec<Line> = match selected {
        Some(project) => {
            let state = app
                .daemon
                .projects
                .iter()
                .find(|p| p.alias == project.alias);
            let git_entry = app
                .git_states
                .iter()
                .find(|(alias, _)| alias == &project.alias);
            let git_state = git_entry.and_then(|(_, state)| state.as_ref());
            let path_exists = std::path::Path::new(&project.path).is_dir();
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("alias    ", Style::default().fg(app.theme.text_dim)),
                    Span::styled(
                        project.alias.clone(),
                        Style::default().fg(app.theme.accent_bright),
                    ),
                    Span::styled(
                        format!("   path {}", project.path),
                        Style::default().fg(app.theme.text_dim),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("branches ", Style::default().fg(app.theme.text_dim)),
                    Span::raw(format!(
                        "current {} · default {}",
                        git_state
                            .and_then(|s| s.current_branch.clone())
                            .unwrap_or_else(|| "—".into()),
                        project.default_branch.clone().unwrap_or_else(|| "—".into()),
                    )),
                ]),
            ];
            // Working tree / agent line.
            let tree = if !path_exists {
                (
                    "working tree missing (path not on disk)".to_string(),
                    app.theme.red,
                )
            } else if git_entry.is_none() {
                ("reading git state…".to_string(), app.theme.text_dim)
            } else if git_state.is_none() {
                ("not a git repository".to_string(), app.theme.red)
            } else if let Some(state) = git_state.filter(|s| s.dirty) {
                (
                    format!(
                        "working tree dirty — {} changed file{}",
                        state.changed_files,
                        if state.changed_files == 1 { "" } else { "s" }
                    ),
                    app.theme.yellow,
                )
            } else {
                ("working tree clean".to_string(), app.theme.green)
            };
            let agent = match &app.project_detail {
                Some((alias, config)) if alias == &project.alias => {
                    let agent_type = config
                        .agent
                        .as_ref()
                        .and_then(|a| a.agent_type.clone())
                        .unwrap_or_else(|| "pi (default)".into());
                    let model = config.pi.model.clone().unwrap_or_else(|| "—".into());
                    format!(" · agent {agent_type} · model {model}")
                }
                _ => String::new(),
            };
            lines.push(Line::from(vec![
                Span::styled(tree.0, Style::default().fg(tree.1)),
                Span::styled(agent, Style::default().fg(app.theme.text_dim)),
            ]));
            lines.push(Line::from(Span::styled(
                format!(
                    "queue {} · blocked {} · last result {}",
                    state
                        .map(|p| p.queue_size.to_string())
                        .unwrap_or_else(|| "?".into()),
                    state
                        .map(|p| p.blocked_tasks.to_string())
                        .unwrap_or_else(|| "?".into()),
                    state
                        .and_then(|p| p.last_result.clone())
                        .unwrap_or_else(|| "—".into()),
                ),
                Style::default().fg(app.theme.text_dim),
            )));
            lines
        }
        None => vec![Line::from(Span::styled(
            "Nothing selected.",
            Style::default().fg(app.theme.text_dim),
        ))],
    };
    frame.render_widget(Paragraph::new(detail), inner);

    frame.render_widget(
        key_hint_line(
            &app.theme,
            &[
                ("↑/↓", "navigate"),
                ("Enter", "set active"),
                ("a", "add"),
                ("i", "init templates"),
                ("x", "remove"),
            ],
        ),
        chunks[2],
    );
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

pub fn tasks(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(chunks[0]);

    let summary = c::task_ledger::summarize_tasks(&app.ledger.tasks);
    let awaiting = summary
        .by_status
        .get("awaiting-approval")
        .copied()
        .unwrap_or(0);
    let filter_label = app
        .task_filter
        .map(|f| f.as_str().to_string())
        .unwrap_or_else(|| "all".to_string());
    let title = format!(
        "Tasks — {} · total {} · ready {} · done {} · awaiting {} · filter: {}",
        app.active_alias.as_deref().unwrap_or("no project"),
        summary.total,
        summary.by_status.get("ready").copied().unwrap_or(0),
        summary.by_status.get("done").copied().unwrap_or(0),
        awaiting,
        filter_label,
    );

    let filtered = app.filtered_tasks();
    let rows: Vec<Row> = filtered
        .iter()
        .map(|task| {
            Row::new(vec![
                Cell::from(task.id.clone()).style(Style::default().fg(app.theme.text)),
                Cell::from(task.status.as_str().to_string())
                    .style(Style::default().fg(status_color(&app.theme, task.status.as_str()))),
                Cell::from(task.risk.as_str().to_string()).style(Style::default().fg(
                    if task.risk.as_str() == "high-risk" {
                        app.theme.orange
                    } else {
                        app.theme.text_dim
                    },
                )),
                Cell::from(task.title.clone()).style(Style::default().fg(app.theme.text)),
            ])
        })
        .collect();
    let empty = if app.active_alias.is_none() {
        "No active project — activate one in Projects."
    } else {
        "No tasks — [a] add or [f] change filter."
    };
    let table = selection_table(
        &app.theme,
        &title,
        &["ID", "STATUS", "RISK", "TITLE"],
        &[
            Constraint::Length(22),
            Constraint::Length(18),
            Constraint::Length(12),
            Constraint::Min(10),
        ],
        rows,
        empty,
    );
    // Detail panel (built before the stateful render releases the borrow).
    let detail_block = panel(&app.theme, "Task details");
    let inner = detail_block.inner(columns[1]);
    frame.render_widget(detail_block, columns[1]);
    let mut lines: Vec<Line> = match filtered.get(app.selected) {
        Some(task) => {
            let mut lines = vec![
                Line::from(Span::styled(
                    task.title.clone(),
                    Style::default()
                        .fg(app.theme.accent_bright)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(vec![
                    Span::styled("id      ", Style::default().fg(app.theme.text_dim)),
                    Span::raw(task.id.clone()),
                ]),
                Line::from(vec![
                    Span::styled("status  ", Style::default().fg(app.theme.text_dim)),
                    Span::styled(
                        task.status.as_str().to_string(),
                        Style::default().fg(status_color(&app.theme, task.status.as_str())),
                    ),
                    Span::styled(
                        format!("   {} · {}", task.kind.as_str(), task.risk.as_str()),
                        Style::default().fg(app.theme.text_dim),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("branch  ", Style::default().fg(app.theme.text_dim)),
                    Span::raw(task.branch.clone().unwrap_or_else(|| "—".into())),
                ]),
                Line::from(vec![
                    Span::styled("source  ", Style::default().fg(app.theme.text_dim)),
                    Span::raw(format!("{}/{}", task.source.source_type, task.source.r#ref)),
                ]),
                Line::from(vec![
                    Span::styled("attempts", Style::default().fg(app.theme.text_dim)),
                    Span::raw(format!(
                        " {}   promotion {}   cost {}",
                        task.attempts,
                        task.promotion.as_str(),
                        task.estimated_cost_usd
                            .map(|v| format!("${v:.4}"))
                            .unwrap_or_else(|| "—".into()),
                    )),
                ]),
            ];
            if let Some(depends) = &task.depends_on {
                lines.push(Line::from(vec![
                    Span::styled("waits on", Style::default().fg(app.theme.text_dim)),
                    Span::raw(format!(" {}", depends.join(", "))),
                ]));
            }
            if let Some(last_run) = &task.last_run {
                lines.push(Line::from(vec![
                    Span::styled("last run", Style::default().fg(app.theme.text_dim)),
                    Span::styled(
                        format!(" {} ", last_run.outcome.as_str()),
                        Style::default().fg(status_color(
                            &app.theme,
                            match last_run.outcome.as_str() {
                                "success" => "done",
                                "failure" => "failed",
                                other => other,
                            },
                        )),
                    ),
                    Span::styled(
                        format!("{} ago", age_str(&last_run.completed_at)),
                        Style::default().fg(app.theme.text_dim),
                    ),
                ]));
            }
            if !task.acceptance_criteria.is_empty() {
                lines.push(Line::from(Span::styled(
                    "acceptance",
                    Style::default().fg(app.theme.text_dim),
                )));
                for ac in task.acceptance_criteria.iter().take(4) {
                    lines.push(Line::from(format!("  • {ac}")));
                }
            }
            if let Some(notes) = &task.notes {
                if !notes.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "notes",
                        Style::default().fg(app.theme.text_dim),
                    )));
                    for note in notes.iter().rev().take(3) {
                        lines.push(Line::from(Span::styled(
                            format!("  · {note}"),
                            Style::default().fg(app.theme.text_dim),
                        )));
                    }
                }
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "Select a task to inspect it.",
            Style::default().fg(app.theme.text_dim),
        ))],
    };
    if !app.status_msg.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            app.status_msg.clone(),
            Style::default().fg(app.theme.yellow),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    frame.render_stateful_widget(table, columns[0], &mut app.table_state);

    frame.render_widget(
        key_hint_line(
            &app.theme,
            &[
                ("↑/↓", "navigate"),
                ("a", "add"),
                ("y", "approve"),
                ("u", "status"),
                ("f", "filter"),
                ("d", "delete"),
                ("r", "recover stuck"),
            ],
        ),
        chunks[1],
    );
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub const SERVICE_ACTIONS: [&str; 7] = [
    "Start daemon",
    "Stop daemon",
    "Restart daemon",
    "Pause",
    "Resume",
    "Run one iteration now",
    "View daemon logs",
];

pub fn service(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // state panel
            Constraint::Length(10), // actions
            Constraint::Min(4),     // log tail
            Constraint::Length(1),  // hints
        ])
        .split(area);

    // State panel.
    let daemon = &app.daemon;
    let status_label = if daemon.paused {
        "⏸ paused"
    } else if daemon.pid != 0 {
        "● running"
    } else {
        "○ stopped"
    };
    let status_color = if daemon.paused {
        app.theme.yellow
    } else if daemon.pid != 0 {
        app.theme.green
    } else {
        app.theme.text_dim
    };
    let limit = app.global_config.budgets.daily_cost_usd;
    let budget = if limit > 0.0 {
        format!(
            "${:.4} / ${:.2} today ({:.0}%)",
            daemon.budget_spent_usd,
            limit,
            daemon.budget_spent_usd / limit * 100.0
        )
    } else {
        format!("${:.4} today · no limit", daemon.budget_spent_usd)
    };
    let state_block = panel(&app.theme, "Daemon state");
    let state_inner = state_block.inner(chunks[0]);
    frame.render_widget(state_block, chunks[0]);
    let state_lines = vec![
        Line::from(vec![
            Span::styled(
                status_label,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  pid {} · uptime {} · tick every {}",
                    daemon.pid,
                    if daemon.started_at.is_empty() {
                        "—".to_string()
                    } else {
                        elapsed_str(&daemon.started_at)
                    },
                    app.global_config
                        .runtime
                        .tick_interval_seconds
                        .map(|s| format!("{s}s"))
                        .unwrap_or_else(|| "30s".into()),
                ),
                Style::default().fg(app.theme.text_dim),
            ),
        ]),
        Line::from(vec![
            Span::styled("budget ", Style::default().fg(app.theme.text_dim)),
            Span::styled(
                budget,
                Style::default().fg(if daemon.budget_blocked {
                    app.theme.red
                } else {
                    app.theme.text
                }),
            ),
            Span::styled(
                if daemon.budget_blocked {
                    "  ⚠ blocked — raises the daily limit or wait for the window to roll over"
                } else {
                    ""
                },
                Style::default().fg(app.theme.yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("started ", Style::default().fg(app.theme.text_dim)),
            Span::raw(if daemon.started_at.is_empty() {
                "—".to_string()
            } else {
                daemon.started_at.clone()
            }),
        ]),
    ];
    frame.render_widget(Paragraph::new(state_lines), state_inner);

    // Actions list.
    let actions_block = panel(&app.theme, "Actions");
    let actions_inner = actions_block.inner(chunks[1]);
    frame.render_widget(actions_block, chunks[1]);
    let action_lines: Vec<Line> = SERVICE_ACTIONS
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let selected = index == app.service_action;
            Line::from(Span::styled(
                format!("{} {action}", if selected { "▸" } else { " " }),
                Style::default()
                    .fg(if selected {
                        app.theme.accent_bright
                    } else {
                        app.theme.text
                    })
                    .bg(if selected {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    }),
            ))
        })
        .collect();
    frame.render_widget(
        Paragraph::new(action_lines).wrap(Wrap { trim: false }),
        actions_inner,
    );

    // Log tail.
    let log_block = panel(&app.theme, "Daemon log (tail)");
    let log_inner = log_block.inner(chunks[2]);
    frame.render_widget(log_block, chunks[2]);
    let log_lines: Vec<Line> = if app.daemon_log_lines.is_empty() {
        vec![Line::from(Span::styled(
            "(no daemon log yet)",
            Style::default().fg(app.theme.text_dim),
        ))]
    } else {
        app.daemon_log_lines
            .iter()
            .map(|l| {
                Line::from(Span::styled(
                    l.clone(),
                    Style::default().fg(app.theme.text_dim),
                ))
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(log_lines), log_inner);

    frame.render_widget(
        key_hint_line(
            &app.theme,
            &[("↑/↓", "select action"), ("Enter", "execute")],
        ),
        chunks[3],
    );
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

pub fn config(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // segmented control
            Constraint::Min(1),    // entries
            Constraint::Length(1), // hints
        ])
        .split(area);

    let scope = if app.config_panel {
        app.active_alias.as_deref().unwrap_or("no project")
    } else {
        "global"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Global ",
                Style::default()
                    .fg(if !app.config_panel {
                        app.theme.bg_panel
                    } else {
                        app.theme.text_dim
                    })
                    .bg(if !app.config_panel {
                        app.theme.accent
                    } else {
                        ratatui::style::Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " Project ",
                Style::default()
                    .fg(if app.config_panel {
                        app.theme.bg_panel
                    } else {
                        app.theme.text_dim
                    })
                    .bg(if app.config_panel {
                        app.theme.accent
                    } else {
                        ratatui::style::Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  — showing {scope}"),
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );

    let rows: Vec<Row> = app
        .config_entries
        .iter()
        .map(|(key, value, editable)| {
            Row::new(vec![
                Cell::from(key.clone()).style(Style::default().fg(if *editable {
                    app.theme.text
                } else {
                    app.theme.text_muted
                })),
                Cell::from(if *editable {
                    value.clone()
                } else {
                    format!("{value}  (read-only)")
                })
                .style(Style::default().fg(if *editable {
                    app.theme.text
                } else {
                    app.theme.text_muted
                })),
            ])
        })
        .collect();
    let table = selection_table(
        &app.theme,
        "Configuration",
        &["KEY", "VALUE"],
        &[Constraint::Percentage(45), Constraint::Percentage(55)],
        rows,
        "No configuration entries.",
    );
    frame.render_stateful_widget(table, chunks[1], &mut app.table_state);

    frame.render_widget(
        key_hint_line(
            &app.theme,
            &[
                ("↑/↓", "navigate"),
                ("Tab", "global/project"),
                ("Enter", "edit"),
                ("R", "reload"),
            ],
        ),
        chunks[2],
    );
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

pub fn run(app: &mut App, frame: &mut Frame, area: Rect) {
    app.poll_run_result();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let output_block = panel(
        &app.theme,
        &format!(
            "Agent run — {}",
            if app.run_running {
                "running…"
            } else if app.run_exit_code.is_some() {
                "finished"
            } else {
                "idle"
            }
        ),
    );
    let inner = output_block.inner(chunks[0]);
    frame.render_widget(output_block, chunks[0]);
    let lines: Vec<Line> = if app.run_output.is_empty() {
        vec![Line::from(Span::styled(
            "Run the default agent ad-hoc against the active project. Press Ctrl+R to start.",
            Style::default().fg(app.theme.text_dim),
        ))]
    } else {
        app.run_output
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(app.theme.text))))
            .collect()
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

    frame.render_widget(
        key_hint_line(&app.theme, &[("Ctrl+R", "new run")]),
        chunks[1],
    );
}

// ---------------------------------------------------------------------------
// Promotions
// ---------------------------------------------------------------------------

pub fn promotions(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(chunks[0]);

    let rows: Vec<Row> = app
        .promotions
        .iter()
        .map(|item| {
            let artifact = &item.artifact;
            Row::new(vec![
                Cell::from(artifact.task_id.clone()).style(Style::default().fg(app.theme.text)),
                Cell::from(artifact.status.as_str().to_string())
                    .style(Style::default().fg(status_color(&app.theme, artifact.status.as_str()))),
                Cell::from(artifact.action.as_str().to_string())
                    .style(Style::default().fg(app.theme.text_dim)),
                Cell::from(artifact.created_at.chars().take(10).collect::<String>())
                    .style(Style::default().fg(app.theme.text_dim)),
            ])
        })
        .collect();
    let table = selection_table(
        &app.theme,
        &format!(
            "Promotions — {}",
            app.active_alias.as_deref().unwrap_or("no project")
        ),
        &["TASK", "STATUS", "ACTION", "DATE"],
        &[
            Constraint::Percentage(34),
            Constraint::Percentage(20),
            Constraint::Percentage(26),
            Constraint::Percentage(20),
        ],
        rows,
        "No promotion artifacts yet — finished runs land here.",
    );
    frame.render_stateful_widget(table, columns[0], &mut app.table_state);

    let detail_block = panel(&app.theme, "Artifact");
    let inner = detail_block.inner(columns[1]);
    frame.render_widget(detail_block, columns[1]);
    let detail = if app.promotion_detail.is_empty() {
        Paragraph::new(Span::styled(
            "Select a promotion and press [Enter] to load its artifact.",
            Style::default().fg(app.theme.text_dim),
        ))
    } else {
        Paragraph::new(app.promotion_detail.clone()).wrap(Wrap { trim: false })
    };
    frame.render_widget(detail, inner);

    frame.render_widget(
        key_hint_line(
            &app.theme,
            &[
                ("↑/↓", "navigate"),
                ("Enter", "view artifact"),
                ("a", "apply"),
                ("r", "reject"),
            ],
        ),
        chunks[1],
    );
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

pub fn events(app: &mut App, frame: &mut Frame, area: Rect) {
    const SINCE_OPTIONS: [&str; 4] = ["30m", "1h", "2h", "1d"];
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let rows: Vec<Row> = app
        .events
        .iter()
        .map(|event| {
            let time = chrono::DateTime::parse_from_rfc3339(&event.ts)
                .map(|t| {
                    t.with_timezone(&chrono::Local)
                        .format("%H:%M:%S")
                        .to_string()
                })
                .unwrap_or_else(|_| "??:??:??".to_string());
            let color = if event.event.contains("complete") || event.event.contains("promoted") {
                app.theme.green
            } else if event.event.contains("fail") || event.event.contains("error") {
                app.theme.red
            } else if event.event.contains("start") || event.event.contains("running") {
                app.theme.cyan
            } else if event.event.contains("pause") || event.event.contains("budget") {
                app.theme.yellow
            } else {
                app.theme.text
            };
            Row::new(vec![
                Cell::from(time).style(Style::default().fg(app.theme.text_dim)),
                Cell::from(event.event.clone()).style(Style::default().fg(color)),
                Cell::from(event.project.clone().unwrap_or_else(|| "—".into()))
                    .style(Style::default().fg(app.theme.accent)),
                Cell::from(event.task_id.clone().unwrap_or_default())
                    .style(Style::default().fg(app.theme.text_dim)),
            ])
        })
        .collect();
    let table = selection_table(
        &app.theme,
        &format!(
            "Events — since {} · project {} · {} shown",
            SINCE_OPTIONS[app.events_since_idx.min(3)],
            app.events_project_filter.as_deref().unwrap_or("all"),
            app.events.len()
        ),
        &["TIME", "EVENT", "PROJECT", "TASK"],
        &[
            Constraint::Percentage(12),
            Constraint::Percentage(34),
            Constraint::Percentage(18),
            Constraint::Percentage(36),
        ],
        rows,
        "No events — widen with [s] or let the daemon work.",
    );
    frame.render_stateful_widget(table, chunks[0], &mut app.table_state);

    frame.render_widget(
        key_hint_line(
            &app.theme,
            &[
                ("↑/↓", "navigate"),
                ("s", "cycle since"),
                ("p", "pick project"),
            ],
        ),
        chunks[1],
    );
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

pub fn logs(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(chunks[0]);

    let rows: Vec<Row> = app
        .run_files
        .iter()
        .map(|file| {
            Row::new(vec![
                Cell::from(file.clone()).style(Style::default().fg(app.theme.text))
            ])
        })
        .collect();
    let files_table = selection_table(
        &app.theme,
        if app.log_tab {
            "Run summaries"
        } else {
            "Daemon log"
        },
        &["FILE"],
        &[Constraint::Percentage(100)],
        rows,
        "No run summaries yet.",
    );
    if app.log_tab {
        frame.render_stateful_widget(files_table, columns[0], &mut app.table_state);
    } else {
        let block = panel(&app.theme, "Run summaries");
        let inner = block.inner(columns[0]);
        frame.render_widget(block, columns[0]);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Press [Tab] to switch to run summaries.",
                Style::default().fg(app.theme.text_dim),
            )),
            inner,
        );
    }

    let content_block = panel(
        &app.theme,
        if app.log_tab {
            "Summary content"
        } else {
            "Daemon log (tail)"
        },
    );
    let inner = content_block.inner(columns[1]);
    frame.render_widget(content_block, columns[1]);
    let lines: Vec<Line> = if app.log_tab {
        if app.run_file_content.is_empty() {
            vec![Line::from(Span::styled(
                "Select a run summary on the left.",
                Style::default().fg(app.theme.text_dim),
            ))]
        } else {
            app.run_file_content
                .lines()
                .map(|l| {
                    Line::from(Span::styled(
                        l.to_string(),
                        Style::default().fg(app.theme.text),
                    ))
                })
                .collect()
        }
    } else if app.daemon_log_lines.is_empty() {
        vec![Line::from(Span::styled(
            "(no daemon log yet)",
            Style::default().fg(app.theme.text_dim),
        ))]
    } else {
        app.daemon_log_lines
            .iter()
            .map(|l| {
                Line::from(Span::styled(
                    l.clone(),
                    Style::default().fg(app.theme.text_dim),
                ))
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

    frame.render_widget(
        key_hint_line(
            &app.theme,
            &[("Tab", "daemon/runs"), ("↑/↓", "select file")],
        ),
        chunks[1],
    );
}
