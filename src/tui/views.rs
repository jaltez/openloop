use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use super::app::{elapsed_str, App, InputMode};
use super::theme::status_color;
use crate::core as c;

fn pad(s: &str, len: usize) -> String {
    if s.chars().count() >= len {
        s.chars().take(len).collect()
    } else {
        format!("{s}{}", " ".repeat(len - s.chars().count()))
    }
}

fn header_line(theme: &ratatui::style::Color, cells: &[(&str, usize)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, (text, width)) in cells.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            pad(text, *width),
            Style::default().fg(*theme).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn key_hint(frame: &mut Frame, area: Rect, hints: &[(&str, &str)], theme: &ratatui::style::Color) {
    let mut spans = Vec::new();
    for (key, label) in hints {
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            *key,
            Style::default().fg(*theme).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme_dim(theme)),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn theme_dim(_accent: &ratatui::style::Color) -> ratatui::style::Color {
    // The dim color is stable across views; pull it from the theme constant.
    ratatui::style::Color::Rgb(0x64, 0x74, 0x8b)
}

pub fn dashboard(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    // Header
    let daemon = &app.daemon;
    let status = if daemon.paused {
        "PAUSED".to_string()
    } else if daemon.pid != 0 {
        format!("RUNNING (PID {})", daemon.pid)
    } else {
        "STOPPED".to_string()
    };
    let uptime = if daemon.started_at.is_empty() {
        String::new()
    } else {
        format!(" │ uptime: {}", elapsed_str(&daemon.started_at))
    };
    let budget = format!(" │ ${:.4} spent today", daemon.budget_spent_usd);
    let header = vec![
        Line::from(vec![
            Span::styled(
                "Dashboard",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — live multi-project overview",
                Style::default().fg(app.theme.text_dim),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("Daemon: {status}"),
                Style::default().fg(if daemon.paused {
                    app.theme.yellow
                } else if daemon.pid != 0 {
                    app.theme.green
                } else {
                    app.theme.text_dim
                }),
            ),
            Span::styled(
                format!("{uptime}{budget}"),
                Style::default().fg(app.theme.text_dim),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(header), chunks[0]);

    // Table
    let mut lines = vec![header_line(
        &app.theme.accent,
        &[
            ("PROJECT", 16),
            ("QUEUE", 8),
            ("ACTIVE TASK", 24),
            ("STATUS", 12),
            ("LAST RUN", 20),
        ],
    )];
    lines.push(Line::from(Span::styled(
        "─".repeat(84),
        Style::default().fg(app.theme.border),
    )));
    if app.projects.is_empty() {
        lines.push(Line::from(Span::styled(
            "No linked projects. Use 'openloop project add' to register one.",
            Style::default().fg(app.theme.text_dim),
        )));
    }
    for (index, project) in app.projects.iter().enumerate() {
        let is_selected = index == app.selected;
        let is_active = app
            .daemon
            .current_run
            .as_ref()
            .map(|r| r.project_alias == project.alias)
            .unwrap_or(false);
        let proj_state = app
            .daemon
            .projects
            .iter()
            .find(|p| p.alias == project.alias);
        let queue_size = proj_state
            .map(|p| p.queue_size.to_string())
            .unwrap_or_else(|| "?".into());
        let active_task = if is_active {
            app.daemon
                .current_run
                .as_ref()
                .and_then(|r| r.task_id.clone())
                .unwrap_or_else(|| "--".into())
        } else {
            "--".to_string()
        };
        let (status_label, status_color) = if is_active {
            ("running", app.theme.green)
        } else if app.daemon.paused {
            ("paused", app.theme.yellow)
        } else {
            ("idle", app.theme.text_dim)
        };
        let last_run: String = proj_state
            .and_then(|p| p.last_result.clone())
            .map(|s| s.chars().take(20).collect())
            .unwrap_or_else(|| "--".into());
        lines.push(Line::from(vec![
            Span::styled(
                pad(&project.alias, 16),
                Style::default()
                    .fg(if is_selected {
                        app.theme.accent_bright
                    } else {
                        app.theme.text
                    })
                    .bg(if is_selected {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    }),
            ),
            Span::raw("  "),
            Span::styled(pad(&queue_size, 8), Style::default()),
            Span::raw("  "),
            Span::styled(pad(&active_task, 24), Style::default()),
            Span::raw("  "),
            Span::styled(pad(status_label, 12), Style::default().fg(status_color)),
            Span::raw("  "),
            Span::styled(last_run, Style::default().fg(app.theme.text_dim)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), chunks[1]);

    key_hint(
        frame,
        chunks[2],
        &[("↑/↓", "navigate"), ("Tab", "next view")],
        &app.theme.accent,
    );
}

pub fn projects(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Projects",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — linked repositories",
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );
    if !app.status_msg.is_empty() {
        frame.render_widget(
            Paragraph::new(app.status_msg.clone()).style(Style::default().fg(app.theme.yellow)),
            chunks[1],
        );
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[2]);

    // Left: project list
    let mut lines = vec![header_line(
        &app.theme.accent,
        &[("ALIAS", 16), ("INIT", 6), ("ACTIVE", 8)],
    )];
    lines.push(Line::from(Span::styled(
        "─".repeat(34),
        Style::default().fg(app.theme.border),
    )));
    if app.projects.is_empty() {
        lines.push(Line::from(Span::styled(
            "No projects. Press 'a' to add.",
            Style::default().fg(app.theme.text_dim),
        )));
    }
    for (index, project) in app.projects.iter().enumerate() {
        let is_selected = index == app.selected;
        let is_active = app.active_alias.as_deref() == Some(project.alias.as_str());
        lines.push(Line::from(vec![
            Span::styled(
                pad(&project.alias, 16),
                Style::default()
                    .fg(if is_selected {
                        app.theme.accent_bright
                    } else {
                        app.theme.text
                    })
                    .bg(if is_selected {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    }),
            ),
            Span::raw("  "),
            Span::styled(
                pad(if project.initialized { "yes" } else { "no" }, 6),
                Style::default(),
            ),
            Span::raw("  "),
            Span::styled(
                if is_active { "●" } else { " " },
                Style::default().fg(if is_active {
                    app.theme.green
                } else {
                    app.theme.text_dim
                }),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), columns[0]);

    // Right: detail
    let detail_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let inner_detail = detail_block.inner(columns[1]);
    frame.render_widget(detail_block, columns[1]);
    let detail_lines = match app.projects.get(app.selected) {
        Some(project) => vec![
            Line::from(Span::styled(
                &project.alias,
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled("Path: ", Style::default().fg(app.theme.text_dim)),
                Span::styled(&project.path, Style::default().fg(app.theme.text)),
            ]),
            Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(app.theme.text_dim)),
                Span::styled(
                    project.default_branch.as_deref().unwrap_or("—"),
                    Style::default().fg(app.theme.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("Initialized: ", Style::default().fg(app.theme.text_dim)),
                Span::styled(
                    if project.initialized { "yes" } else { "no" },
                    Style::default().fg(app.theme.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("Created: ", Style::default().fg(app.theme.text_dim)),
                Span::styled(&project.created_at, Style::default().fg(app.theme.text)),
            ]),
            Line::from(vec![
                Span::styled("Updated: ", Style::default().fg(app.theme.text_dim)),
                Span::styled(&project.updated_at, Style::default().fg(app.theme.text)),
            ]),
        ],
        None => vec![Line::from(Span::styled(
            "Select a project to view details",
            Style::default().fg(app.theme.text_dim),
        ))],
    };
    frame.render_widget(Paragraph::new(detail_lines), inner_detail);

    // Input form when adding
    if app.input_mode == InputMode::AddProjectAlias || app.input_mode == InputMode::AddProjectPath {
        let label = if app.input_mode == InputMode::AddProjectAlias {
            "Alias:"
        } else {
            "Path:"
        };
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.accent))
            .title("Add Project");
        let area = Rect {
            x: columns[0].x,
            y: columns[0].y,
            width: 50.min(columns[0].width),
            height: 3,
        };
        frame.render_widget(ratatui::widgets::Clear, area);
        frame.render_widget(input_block, area);
        frame.render_widget(
            Paragraph::new(format!("{label} {}", app.input_value)),
            Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: 1,
            },
        );
    }

    key_hint(
        frame,
        chunks[3],
        &[
            ("↑/↓", "navigate"),
            ("Enter", "activate"),
            ("a", "add"),
            ("d", "remove"),
            ("i", "init"),
        ],
        &app.theme.accent,
    );
}

pub fn tasks(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    let summary = c::task_ledger::summarize_tasks(&app.ledger.tasks);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Tasks",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" — {}", app.active_alias.as_deref().unwrap_or("no project")),
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );
    let filter_label = app
        .task_filter
        .map(|f| f.as_str().to_string())
        .unwrap_or_else(|| "all".to_string());
    let header_right = format!(
        "total:{} ready:{} done:{} filter:{}",
        summary.total,
        summary.by_status.get("ready").copied().unwrap_or(0),
        summary.by_status.get("done").copied().unwrap_or(0),
        filter_label,
    );
    let header = Line::from(vec![
        Span::styled(
            "Tasks",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(header_right, Style::default().fg(app.theme.text_dim)),
    ]);
    frame.render_widget(Paragraph::new(header), chunks[0]);
    if !app.status_msg.is_empty() {
        frame.render_widget(
            Paragraph::new(app.status_msg.clone()).style(Style::default().fg(app.theme.yellow)),
            chunks[1],
        );
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

    let filtered = app.filtered_tasks();
    let mut lines = vec![header_line(
        &app.theme.accent,
        &[("ID", 20), ("STATUS", 12), ("RISK", 8), ("TITLE", 30)],
    )];
    lines.push(Line::from(Span::styled(
        "─".repeat(74),
        Style::default().fg(app.theme.border),
    )));
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "No tasks. Press 'a' to add.",
            Style::default().fg(app.theme.text_dim),
        )));
    }
    for (index, task) in filtered.iter().enumerate() {
        let is_selected = index == app.selected;
        lines.push(Line::from(vec![
            Span::styled(
                pad(&task.id, 20),
                Style::default()
                    .fg(if is_selected {
                        app.theme.accent_bright
                    } else {
                        app.theme.text
                    })
                    .bg(if is_selected {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    }),
            ),
            Span::raw("  "),
            Span::styled(
                pad(task.status.as_str(), 12),
                Style::default().fg(status_color(&app.theme, task.status.as_str())),
            ),
            Span::raw("  "),
            Span::styled(pad(task.risk.as_str(), 8), Style::default()),
            Span::raw("  "),
            Span::styled(
                task.title.chars().take(30).collect::<String>(),
                Style::default().fg(app.theme.text),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), columns[0]);

    // Right: detail
    let detail_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let inner = detail_block.inner(columns[1]);
    frame.render_widget(detail_block, columns[1]);
    let detail_lines = match filtered.get(app.selected) {
        Some(task) => {
            let mut lines = vec![
                Line::from(Span::styled(
                    task.title.as_str(),
                    Style::default()
                        .fg(app.theme.accent)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(format!("ID: {}", task.id)),
                Line::from(format!("Kind: {}", task.kind.as_str())),
                Line::from(format!("Status: {}", task.status.as_str())),
                Line::from(format!("Risk: {}", task.risk.as_str())),
                Line::from(format!("Branch: {}", task.branch.as_deref().unwrap_or("—"))),
                Line::from(format!("Attempts: {}", task.attempts)),
                Line::from(format!(
                    "Source: {} / {}",
                    task.source.source_type, task.source.r#ref
                )),
                Line::from(format!("Created: {}", task.created_at)),
            ];
            if !task.acceptance_criteria.is_empty() {
                lines.push(Line::from("Acceptance criteria:"));
                for ac in &task.acceptance_criteria {
                    lines.push(Line::from(format!(" • {ac}")));
                }
            }
            if let Some(last_run) = &task.last_run {
                lines.push(Line::from(format!(
                    "Last run: {} @ {}",
                    last_run.outcome.as_str(),
                    last_run.completed_at
                )));
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "Select a task",
            Style::default().fg(app.theme.text_dim),
        ))],
    };
    frame.render_widget(Paragraph::new(detail_lines), inner);

    if app.input_mode == InputMode::AddTaskTitle {
        let area = Rect {
            x: columns[0].x,
            y: columns[0].y,
            width: 50.min(columns[0].width),
            height: 3,
        };
        frame.render_widget(ratatui::widgets::Clear, area);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.accent))
                .title("Add Task"),
            area,
        );
        frame.render_widget(
            Paragraph::new(format!("Title: {}", app.input_value)),
            Rect {
                x: area.x + 1,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: 1,
            },
        );
    }

    key_hint(
        frame,
        chunks[3],
        &[
            ("↑/↓", "navigate"),
            ("f", "filter status"),
            ("a", "add"),
            ("d", "delete"),
            ("r", "recover stuck"),
        ],
        &app.theme.accent,
    );
}

pub fn service(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(9),
            Constraint::Length(5),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Service",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — daemon management",
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );
    if !app.status_msg.is_empty() {
        frame.render_widget(
            Paragraph::new(app.status_msg.clone()).style(Style::default().fg(app.theme.yellow)),
            chunks[1],
        );
    }

    let state_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let state_inner = state_block.inner(chunks[2]);
    frame.render_widget(state_block, chunks[2]);
    let daemon = &app.daemon;
    let status_value = if daemon.paused {
        ("paused".to_string(), app.theme.yellow)
    } else if daemon.pid != 0 {
        ("running".to_string(), app.theme.green)
    } else {
        ("stopped".to_string(), app.theme.text_dim)
    };
    let state_lines = vec![
        Line::from(Span::styled(
            "Daemon State",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("PID: {}", daemon.pid)),
        Line::from(vec![
            Span::raw("Status: "),
            Span::styled(status_value.0, Style::default().fg(status_value.1)),
        ]),
        Line::from(format!("Started: {}", daemon.started_at)),
        Line::from(format!(
            "Uptime: {}",
            if daemon.started_at.is_empty() {
                "—".to_string()
            } else {
                elapsed_str(&daemon.started_at)
            }
        )),
        Line::from(format!("Budget: ${:.4} today", daemon.budget_spent_usd)),
    ];
    frame.render_widget(Paragraph::new(state_lines), state_inner);

    const ACTIONS: [&str; 3] = ["Pause", "Resume", "View Logs"];
    let actions: Vec<ListItem> = ACTIONS
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let selected = index == app.service_action;
            ListItem::new(Line::from(Span::styled(
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
            )))
        })
        .collect();
    frame.render_stateful_widget(
        List::new(actions),
        chunks[3],
        &mut ratatui::widgets::ListState::default(),
    );

    let log_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let log_inner = log_block.inner(chunks[4]);
    frame.render_widget(log_block, chunks[4]);
    let log_lines: Vec<Line> = if app.daemon_log_lines.is_empty() {
        vec![Line::from(Span::styled(
            "(no daemon log)",
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

    key_hint(
        frame,
        chunks[5],
        &[("↑/↓", "select action"), ("Enter", "execute")],
        &app.theme.accent,
    );
}

pub fn config(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Config",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " — {}",
                    if app.config_panel {
                        app.active_alias.as_deref().unwrap_or("no project")
                    } else {
                        "global"
                    }
                ),
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );
    if !app.status_msg.is_empty() {
        frame.render_widget(
            Paragraph::new(app.status_msg.clone()).style(Style::default().fg(app.theme.yellow)),
            chunks[1],
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Global ",
                Style::default()
                    .fg(if !app.config_panel {
                        app.theme.accent
                    } else {
                        app.theme.text_dim
                    })
                    .bg(if !app.config_panel {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " Project ",
                Style::default()
                    .fg(if app.config_panel {
                        app.theme.accent
                    } else {
                        app.theme.text_dim
                    })
                    .bg(if app.config_panel {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        chunks[2],
    );

    let mut lines = vec![header_line(
        &app.theme.accent,
        &[("KEY", 40), ("VALUE", 40)],
    )];
    lines.push(Line::from(Span::styled(
        "─".repeat(80),
        Style::default().fg(app.theme.border),
    )));
    for (index, (key, value, editable)) in app.config_entries.iter().enumerate() {
        let is_selected = index == app.selected;
        lines.push(Line::from(vec![
            Span::styled(
                pad(key, 40),
                Style::default()
                    .fg(if is_selected {
                        app.theme.accent_bright
                    } else if *editable {
                        app.theme.text
                    } else {
                        app.theme.text_dim
                    })
                    .bg(if is_selected {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    }),
            ),
            Span::raw("  "),
            Span::styled(
                if *editable {
                    value.clone()
                } else {
                    format!("{value} (read-only)")
                },
                Style::default().fg(if is_selected {
                    app.theme.accent_bright
                } else {
                    app.theme.text
                }),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), chunks[3]);

    if app.input_mode == InputMode::EditConfigValue {
        let index = app.selected.min(app.config_entries.len().saturating_sub(1));
        if let Some((key, _, _)) = app.config_entries.get(index) {
            let label = format!("{key}: {}", app.input_value);
            frame.render_widget(
                Paragraph::new(label).style(Style::default().fg(app.theme.accent)),
                Rect {
                    x: chunks[3].x,
                    y: chunks[3].y,
                    width: chunks[3].width.min(60),
                    height: 1,
                },
            );
        }
    }

    key_hint(
        frame,
        chunks[4],
        &[
            ("↑/↓", "navigate"),
            ("Tab", "global/project"),
            ("Enter", "edit"),
            ("R", "reload"),
        ],
        &app.theme.accent,
    );
}

pub fn run(app: &mut App, frame: &mut Frame, area: Rect) {
    app.poll_run_result();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Run",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " — direct agent invocation on {}",
                    app.active_alias.as_deref().unwrap_or("no project")
                ),
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );
    if !app.status_msg.is_empty() {
        frame.render_widget(
            Paragraph::new(app.status_msg.clone()).style(Style::default().fg(app.theme.yellow)),
            chunks[1],
        );
    }

    let input_label = match app.input_mode {
        InputMode::RunPrompt => format!("Prompt: {}", app.input_value),
        InputMode::RunModel => format!("Model: {}", app.input_value),
        _ => "Ctrl+R to start a run".to_string(),
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(input_label),
            Line::from(Span::styled(
                if app.run_running {
                    "running..."
                } else {
                    "idle"
                },
                Style::default().fg(if app.run_running {
                    app.theme.yellow
                } else {
                    app.theme.text_dim
                }),
            )),
        ]),
        chunks[2],
    );

    let output_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let output_inner = output_block.inner(chunks[3]);
    frame.render_widget(output_block, chunks[3]);
    let mut lines: Vec<Line> = app
        .run_output
        .iter()
        .map(|l| {
            Line::from(Span::styled(
                l.clone(),
                Style::default().fg(if l.starts_with('>') {
                    app.theme.cyan
                } else {
                    app.theme.text
                }),
            ))
        })
        .collect();
    if let Some(code) = app.run_exit_code {
        lines.push(Line::from(Span::styled(
            format!("Process exited with code {code}"),
            Style::default().fg(if code == 0 {
                app.theme.green
            } else {
                app.theme.red
            }),
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no output yet)",
            Style::default().fg(app.theme.text_dim),
        )));
    }
    frame.render_widget(Paragraph::new(lines), output_inner);

    key_hint(frame, chunks[4], &[("Ctrl+R", "run")], &app.theme.accent);
}

pub fn promotions(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Promotions",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" — {}", app.active_alias.as_deref().unwrap_or("no project")),
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );
    if !app.status_msg.is_empty() {
        frame.render_widget(
            Paragraph::new(app.status_msg.clone()).style(Style::default().fg(app.theme.yellow)),
            chunks[1],
        );
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[2]);

    let mut lines = vec![header_line(
        &app.theme.accent,
        &[("TASK", 20), ("STATUS", 10), ("ACTION", 14), ("DATE", 10)],
    )];
    lines.push(Line::from(Span::styled(
        "─".repeat(58),
        Style::default().fg(app.theme.border),
    )));
    if app.promotions.is_empty() {
        lines.push(Line::from(Span::styled(
            "No promotion artifacts.",
            Style::default().fg(app.theme.text_dim),
        )));
    }
    for (index, item) in app.promotions.iter().enumerate() {
        let is_selected = index == app.selected;
        let artifact = &item.artifact;
        lines.push(Line::from(vec![
            Span::styled(
                pad(&artifact.task_id, 20),
                Style::default()
                    .fg(if is_selected {
                        app.theme.accent_bright
                    } else {
                        app.theme.text
                    })
                    .bg(if is_selected {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    }),
            ),
            Span::raw("  "),
            Span::styled(
                pad(artifact.status.as_str(), 10),
                Style::default().fg(status_color(&app.theme, artifact.status.as_str())),
            ),
            Span::raw("  "),
            Span::styled(pad(artifact.action.as_str(), 14), Style::default()),
            Span::raw("  "),
            Span::styled(
                artifact.created_at.chars().take(10).collect::<String>(),
                Style::default().fg(app.theme.text_dim),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), columns[0]);

    let detail_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let inner = detail_block.inner(columns[1]);
    frame.render_widget(detail_block, columns[1]);
    let detail = if app.promotion_detail.is_empty() {
        Paragraph::new("Select a promotion and press Enter to view details.")
    } else {
        Paragraph::new(app.promotion_detail.clone())
    };
    frame.render_widget(detail, inner);

    key_hint(
        frame,
        chunks[3],
        &[
            ("↑/↓", "navigate"),
            ("Enter", "view detail"),
            ("a", "apply"),
            ("r", "reject"),
        ],
        &app.theme.accent,
    );
}

pub fn events(app: &mut App, frame: &mut Frame, area: Rect) {
    const SINCE_OPTIONS: [&str; 4] = ["30m", "1h", "2h", "1d"];
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Events",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" — audit log", Style::default().fg(app.theme.text_dim)),
            Span::styled(
                format!(
                    "  since: {} │ project: {} │ {} events",
                    SINCE_OPTIONS[app.events_since_idx.min(3)],
                    app.events_project_filter.as_deref().unwrap_or("all"),
                    app.events.len()
                ),
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );

    let mut lines = vec![header_line(
        &app.theme.accent,
        &[("TIME", 10), ("EVENT", 24), ("PROJECT", 14), ("TASK", 20)],
    )];
    lines.push(Line::from(Span::styled(
        "─".repeat(74),
        Style::default().fg(app.theme.border),
    )));
    if app.events.is_empty() {
        lines.push(Line::from(Span::styled(
            "No events in this time window.",
            Style::default().fg(app.theme.text_dim),
        )));
    }
    for event in &app.events {
        let time: String = event.ts.chars().skip(11).take(8).collect();
        let event_color = if event.event.contains("complete") || event.event.contains("promoted") {
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
        lines.push(Line::from(vec![
            Span::styled(pad(&time, 10), Style::default()),
            Span::raw("  "),
            Span::styled(pad(&event.event, 24), Style::default().fg(event_color)),
            Span::raw("  "),
            Span::styled(
                pad(event.project.as_deref().unwrap_or("—"), 14),
                Style::default(),
            ),
            Span::raw("  "),
            Span::styled(
                event.task_id.as_deref().unwrap_or("—"),
                Style::default().fg(app.theme.text_dim),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), chunks[1]);

    key_hint(
        frame,
        chunks[2],
        &[
            ("↑/↓", "navigate"),
            ("s", "cycle since"),
            ("p", "toggle project filter"),
        ],
        &app.theme.accent,
    );
}

pub fn logs(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "Logs",
                Style::default()
                    .fg(app.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " — {}",
                    if app.log_tab {
                        format!(
                            "{} run summaries",
                            app.active_alias.as_deref().unwrap_or("?")
                        )
                    } else {
                        "daemon log".to_string()
                    }
                ),
                Style::default().fg(app.theme.text_dim),
            ),
        ])),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Daemon ",
                Style::default()
                    .fg(if !app.log_tab {
                        app.theme.accent
                    } else {
                        app.theme.text_dim
                    })
                    .bg(if !app.log_tab {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " Project Runs ",
                Style::default()
                    .fg(if app.log_tab {
                        app.theme.accent
                    } else {
                        app.theme.text_dim
                    })
                    .bg(if app.log_tab {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ])),
        chunks[1],
    );

    if !app.log_tab {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border));
        let inner = block.inner(chunks[2]);
        frame.render_widget(block, chunks[2]);
        let lines: Vec<Line> = if app.daemon_log_lines.is_empty() {
            vec![Line::from(Span::styled(
                "(no daemon log)",
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
        frame.render_widget(Paragraph::new(lines), inner);
    } else {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(chunks[2]);

        let files_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border));
        let files_inner = files_block.inner(columns[0]);
        frame.render_widget(files_block, columns[0]);
        let mut lines = vec![Line::from(Span::styled(
            "Run Files",
            Style::default()
                .fg(app.theme.accent)
                .add_modifier(Modifier::BOLD),
        ))];
        if app.run_files.is_empty() {
            lines.push(Line::from(Span::styled(
                "No run summaries.",
                Style::default().fg(app.theme.text_dim),
            )));
        }
        for (index, file) in app.run_files.iter().enumerate() {
            let is_selected = index == app.selected;
            lines.push(Line::from(Span::styled(
                file.clone(),
                Style::default()
                    .fg(if is_selected {
                        app.theme.accent_bright
                    } else {
                        app.theme.text
                    })
                    .bg(if is_selected {
                        app.theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    }),
            )));
        }
        frame.render_widget(Paragraph::new(lines), files_inner);

        let content_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border));
        let content_inner = content_block.inner(columns[1]);
        frame.render_widget(content_block, columns[1]);
        let content = if app.run_file_content.is_empty() {
            "Select a file and press Enter"
        } else {
            &app.run_file_content
        };
        frame.render_widget(Paragraph::new(content.to_string()), content_inner);
    }

    let hints: &[(&str, &str)] = if app.log_tab {
        &[("Tab", "daemon/project"), ("↑/↓", "select file")]
    } else {
        &[("Tab", "daemon/project")]
    };
    key_hint(frame, chunks[3], hints, &app.theme.accent);
}
