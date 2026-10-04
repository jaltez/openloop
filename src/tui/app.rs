use std::path::PathBuf;
use std::time::Instant;

use super::theme::Theme;
use crate::core as c;
use c::types::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, TableState};
use ratatui::Frame;

pub const TABS: [(&str, &str); 9] = [
    ("1", "Dashboard"),
    ("2", "Projects"),
    ("3", "Tasks"),
    ("4", "Service"),
    ("5", "Config"),
    ("6", "Run"),
    ("7", "Promotions"),
    ("8", "Events"),
    ("9", "Logs"),
];

/// Destructive or git-mutating actions gated behind the shared confirm modal.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    DeleteTask { task_id: String },
    RemoveProject { alias: String },
    ApplyPromotion { task_id: String },
    RejectPromotion { task_id: String },
    StopDaemon,
    RestartDaemon,
    ReinitProject { alias: String },
}

/// Option-picker state (task status selection).
pub struct PickerState {
    pub task_id: String,
    pub options: Vec<TaskStatus>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    None,
    AddProjectAlias,
    AddProjectPath,
    AddTaskTitle,
    EditConfigValue,
    RunPrompt,
    RunModel,
}

pub struct App {
    pub version: String,
    pub theme: Theme,
    pub active_tab: usize,
    pub selected: usize,
    pub show_help: bool,
    pub quit_requested: bool,

    // Polling state
    pub daemon: DaemonState,
    pub daemon_refreshed: Instant,
    pub projects: Vec<LinkedProject>,
    pub projects_refreshed: Instant,
    pub active_alias: Option<String>,
    pub ledger: TaskLedger,
    pub ledger_refreshed: Instant,
    pub events: Vec<OpenLoopEvent>,
    pub events_refreshed: Instant,
    pub events_since_idx: usize,
    pub events_project_filter: Option<String>,
    pub promotions: Vec<crate::core::promotion_queue::PromotionQueueItem>,
    pub promotions_refreshed: Instant,
    pub promotion_detail: String,
    pub daemon_log_lines: Vec<String>,
    pub log_tab: bool,
    pub log_refreshed: Instant,
    pub run_prompt_value: String,
    pub run_files: Vec<String>,
    pub run_file_content: String,

    // Config view
    pub config_panel: bool, // false = global, true = project
    pub config_entries: Vec<(String, String, bool)>,
    pub config_refreshed: Instant,

    // Inputs
    pub input_mode: InputMode,
    pub input_value: String,
    pub input_pending_alias: String,
    pub status_msg: String,

    // Task filter
    pub task_filter: Option<TaskStatus>,

    // Service actions
    pub service_action: usize,

    // Run view
    pub run_output: Vec<String>,
    pub run_running: bool,
    pub run_exit_code: Option<i32>,

    // Redesign state
    pub table_state: TableState,
    // Per-project git working-tree state (None = path missing / not a repo).
    pub git_states: Vec<(String, Option<c::git::GitWorkingTreeState>)>,
    pub git_states_refreshed: Instant,
    // Project config (agent/model) for the selected project, Projects tab.
    pub project_detail: Option<(String, c::types::ProjectConfig)>,
    pub confirm: Option<ConfirmAction>,
    pub picker: Option<PickerState>,
    pub global_config: GlobalConfig,
    pub global_config_refreshed: Instant,
    pub tab_spans: Vec<(u16, u16)>,
    pub tick_running: bool,
}

/// Result of a background worker tick triggered from the Service view.
pub static TICK_RESULT: parking_lot::Mutex<Option<Result<String, String>>> =
    parking_lot::Mutex::new(None);

pub fn elapsed_str(from: &str) -> String {
    let ms = c::now_millis() - c::parse_iso_millis(from);
    if ms < 60_000 {
        format!("{}s", (ms as f64 / 1000.0).round() as i64)
    } else if ms < 3_600_000 {
        format!("{}m", (ms as f64 / 60_000.0).round() as i64)
    } else {
        format!("{}h", (ms as f64 / 3_600_000.0).round() as i64)
    }
}

impl App {
    pub fn new(version: String) -> App {
        let mut app = App {
            version,
            theme: Theme::dark(),
            active_tab: 0,
            selected: 0,
            show_help: false,
            quit_requested: false,
            daemon: DaemonState::default(),
            daemon_refreshed: Instant::now() - std::time::Duration::from_secs(10),
            projects: Vec::new(),
            projects_refreshed: Instant::now() - std::time::Duration::from_secs(10),
            run_prompt_value: String::new(),
            active_alias: None,
            ledger: TaskLedger {
                version: 1,
                updated_at: String::new(),
                tasks: Vec::new(),
            },
            ledger_refreshed: Instant::now() - std::time::Duration::from_secs(10),
            events: Vec::new(),
            events_refreshed: Instant::now() - std::time::Duration::from_secs(10),
            events_since_idx: 1,
            events_project_filter: None,
            promotions: Vec::new(),
            promotions_refreshed: Instant::now() - std::time::Duration::from_secs(10),
            promotion_detail: String::new(),
            daemon_log_lines: Vec::new(),
            log_refreshed: Instant::now() - std::time::Duration::from_secs(10),
            log_tab: false,
            run_files: Vec::new(),
            run_file_content: String::new(),
            config_panel: false,
            config_entries: Vec::new(),
            config_refreshed: Instant::now() - std::time::Duration::from_secs(10),
            git_states: Vec::new(),
            git_states_refreshed: Instant::now() - std::time::Duration::from_secs(10),
            project_detail: None,
            table_state: TableState::default(),
            confirm: None,
            picker: None,
            global_config: c::global_config::load_global_config(None)
                .unwrap_or_else(|_| c::global_config::default_global_config()),
            global_config_refreshed: Instant::now(),
            tab_spans: Vec::new(),
            tick_running: false,
            input_mode: InputMode::None,
            input_value: String::new(),
            input_pending_alias: String::new(),
            status_msg: String::new(),
            task_filter: None,
            service_action: 0,
            run_output: Vec::new(),
            run_running: false,
            run_exit_code: None,
        };
        app.refresh_if_needed();
        app
    }

    pub fn active_project(&self) -> Option<&LinkedProject> {
        self.projects
            .iter()
            .find(|p| Some(&p.alias) == self.active_alias.as_ref())
    }

    pub fn active_project_path(&self) -> Option<PathBuf> {
        self.active_project().map(|p| PathBuf::from(&p.path))
    }

    pub fn input_capturing(&self) -> bool {
        self.input_mode != InputMode::None
    }

    /// Switch tab and reset per-tab list state (selection + scroll offset).
    pub fn switch_tab(&mut self, tab: usize) {
        self.active_tab = tab.min(TABS.len() - 1);
        self.selected = 0;
        self.table_state = TableState::default();
    }

    /// Sync the shared TableState with `selected` before rendering.
    fn sync_table_state(&mut self) {
        self.table_state.select(Some(self.selected));
    }

    pub fn should_quit(&self) -> bool {
        self.quit_requested
    }

    pub fn refresh_if_needed(&mut self) {
        // Daemon state drives the status bar on every tab: keep polling at 1 s.
        if self.daemon_refreshed.elapsed() >= std::time::Duration::from_secs(1) {
            self.daemon = c::daemon_state::load_daemon_state(None).unwrap_or_default();
            self.daemon_refreshed = Instant::now();
        }
        // Projects feed the alias used by task/promotion/log views and the
        // status bar; cheap to reload at a relaxed cadence.
        if self.projects_refreshed.elapsed() >= std::time::Duration::from_secs(5) {
            self.projects = c::project_registry::list_projects(None).unwrap_or_default();
            if self.active_alias.is_none() {
                self.active_alias = c::global_config::load_global_config(None)
                    .ok()
                    .and_then(|cfg| cfg.active_project_alias);
            }
            self.projects_refreshed = Instant::now();
        }
        // Global config feeds the dashboard/service cards (budget limit,
        // tick interval); it changes rarely.
        if self.global_config_refreshed.elapsed() >= std::time::Duration::from_secs(10) {
            if let Ok(config) = c::global_config::load_global_config(None) {
                self.global_config = config;
                if self.active_alias.is_none() {
                    self.active_alias = self.global_config.active_project_alias.clone();
                }
            }
            self.global_config_refreshed = Instant::now();
        }
        // Background worker tick from the Service view reports here.
        if self.tick_running {
            if let Some(result) = TICK_RESULT.lock().take() {
                self.tick_running = false;
                self.daemon_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                match result {
                    Ok(msg) => self.status_msg = msg,
                    Err(e) => self.status_msg = format!("Tick failed: {e}"),
                }
            }
        }
        // Repo state for the Projects view: one git invocation pair per
        // project, so it is polled only while that tab is visible.
        if self.active_tab == 1
            && self.git_states_refreshed.elapsed() >= std::time::Duration::from_secs(5)
        {
            self.git_states = self
                .projects
                .iter()
                .map(|project| {
                    let state = if std::path::Path::new(&project.path).is_dir() {
                        c::git::get_git_working_tree_state(std::path::Path::new(&project.path)).ok()
                    } else {
                        None
                    };
                    (project.alias.clone(), state)
                })
                .collect();
            let selected_alias = self
                .projects
                .get(self.selected)
                .map(|project| project.alias.clone());
            self.project_detail = selected_alias.and_then(|alias| {
                let path = self
                    .projects
                    .iter()
                    .find(|p| p.alias == alias)?
                    .path
                    .clone();
                c::project_config::load_project_config(std::path::Path::new(&path))
                    .ok()
                    .map(|config| (alias, config))
            });
            self.git_states_refreshed = Instant::now();
        }
        // Tab-gated polling: only the data source the active view renders is
        // refreshed (TS polls per-view via onCleanup; the port used to poll
        // everything on every tab).
        let tab_needs_ledger = self.active_tab == 2; // Tasks
        let tab_needs_promotions = self.active_tab == 6; // Promotions
        let tab_needs_config = self.active_tab == 4; // Config
        let tab_needs_events = self.active_tab == 0 || self.active_tab == 7; // Dashboard / Events
        let tab_needs_logs = self.active_tab == 3 || self.active_tab == 8; // Service / Logs
        if let Some(path) = self.active_project_path() {
            if tab_needs_ledger
                && self.ledger_refreshed.elapsed() >= std::time::Duration::from_secs(2)
            {
                self.ledger = c::task_ledger::load_task_ledger(&path).unwrap_or(TaskLedger {
                    version: 1,
                    updated_at: String::new(),
                    tasks: Vec::new(),
                });
                self.ledger_refreshed = Instant::now();
            }
            if tab_needs_promotions
                && self.promotions_refreshed.elapsed() >= std::time::Duration::from_secs(5)
            {
                self.promotions =
                    c::promotion_queue::list_promotion_artifacts(&path).unwrap_or_default();
                self.promotions_refreshed = Instant::now();
            }
            if tab_needs_config
                && self.config_refreshed.elapsed() >= std::time::Duration::from_secs(5)
            {
                self.reload_config_entries();
                self.config_refreshed = Instant::now();
            }
        }
        if tab_needs_events && self.events_refreshed.elapsed() >= std::time::Duration::from_secs(3)
        {
            const SINCE_OPTIONS: [(&str, i64); 4] = [
                ("30m", 30 * 60_000),
                ("1h", 60 * 60_000),
                ("2h", 2 * 60 * 60_000),
                ("1d", 24 * 60 * 60_000),
            ];
            self.events = c::event_log::read_recent_events(&c::event_log::ReadEventsOptions {
                since_ms: Some(SINCE_OPTIONS[self.events_since_idx.min(3)].1),
                project: self.events_project_filter.as_deref(),
                limit: Some(100),
                ..Default::default()
            });
            self.events_refreshed = Instant::now();
        }
        if tab_needs_logs && self.log_refreshed.elapsed() >= std::time::Duration::from_secs(3) {
            self.reload_logs();
            self.log_refreshed = Instant::now();
        }
    }

    fn reload_logs(&mut self) {
        let log_path = c::paths::daemon_log_path(None);
        if let Ok(content) = c::fsutil::read_file_tail(&log_path, 64 * 1024) {
            let lines: Vec<&str> = content.split('\n').collect();
            let start = lines.len().saturating_sub(50);
            self.daemon_log_lines = lines[start..].iter().map(|s| s.to_string()).collect();
        }
        if let Some(path) = self.active_project_path() {
            let runs_dir = path.join(".openloop").join("runs");
            let mut files: Vec<String> = std::fs::read_dir(&runs_dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .filter(|n| n.ends_with(".md"))
                        .collect()
                })
                .unwrap_or_default();
            files.sort();
            files.reverse();
            files.truncate(50);
            self.run_files = files;
            let file = self
                .run_files
                .get(self.selected.min(self.run_files.len().saturating_sub(1)));
            self.run_file_content = match file {
                Some(file) => std::fs::read_to_string(runs_dir.join(file))
                    .unwrap_or_else(|_| "(unable to read file)".into()),
                None => String::new(),
            };
        }
    }

    fn reload_config_entries(&mut self) {
        const SETTABLE: [&str; 7] = [
            "budgets.dailyCostUsd",
            "budgets.estimatedCostPerRunUsd",
            "runtime.runTimeoutSeconds",
            "runtime.maxAttemptsPerTask",
            "runtime.noProgressRepeatLimit",
            "runtime.tickIntervalSeconds",
            "runtime.projectSelectionStrategy",
        ];
        let flatten = |value: &serde_json::Value| -> Vec<(String, String, bool)> {
            fn walk(
                value: &serde_json::Value,
                prefix: &str,
                out: &mut Vec<(String, String, bool)>,
            ) {
                if let Some(object) = value.as_object() {
                    for (key, v) in object {
                        let full = if prefix.is_empty() {
                            key.clone()
                        } else {
                            format!("{prefix}.{key}")
                        };
                        if v.is_object() {
                            walk(v, &full, out);
                        } else {
                            let editable = SETTABLE.contains(&full.as_str());
                            let display = match v {
                                serde_json::Value::Null => String::new(),
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            out.push((full, display, editable));
                        }
                    }
                }
            }
            let mut out = Vec::new();
            walk(value, "", &mut out);
            out
        };
        if !self.config_panel {
            if let Ok(config) = c::global_config::load_global_config(None) {
                if let Ok(raw) = serde_json::to_value(&config) {
                    self.config_entries = flatten(&raw);
                }
            }
        } else if let Some(path) = self.active_project_path() {
            if let Ok(config) = c::project_config::load_project_config(&path) {
                if let Ok(raw) = serde_json::to_value(&config) {
                    self.config_entries = flatten(&raw);
                }
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // tab bar
                Constraint::Min(1),    // content
                Constraint::Length(1), // status bar
            ])
            .split(frame.area());

        self.render_tab_bar(frame, chunks[0]);
        self.sync_table_state();
        match self.active_tab {
            0 => self.render_dashboard(frame, chunks[1]),
            1 => self.render_projects(frame, chunks[1]),
            2 => self.render_tasks(frame, chunks[1]),
            3 => self.render_service(frame, chunks[1]),
            4 => self.render_config(frame, chunks[1]),
            5 => self.render_run(frame, chunks[1]),
            6 => self.render_promotions(frame, chunks[1]),
            7 => self.render_events(frame, chunks[1]),
            _ => self.render_logs(frame, chunks[1]),
        }
        self.render_status_bar(frame, chunks[2]);
        // Central overlays: help > confirm > picker > input.
        if self.show_help {
            self.render_help(frame, frame.area());
        } else if let Some(action) = self.confirm.clone() {
            let (title, message) = match &action {
                ConfirmAction::DeleteTask { task_id } => (
                    "Delete task",
                    format!("Delete task {task_id} from the ledger? This cannot be undone."),
                ),
                ConfirmAction::RemoveProject { alias } => (
                    "Remove project",
                    format!("Unlink project '{alias}'? Files on disk are not touched."),
                ),
                ConfirmAction::ApplyPromotion { task_id } => (
                    "Apply promotion",
                    format!(
                        "Apply promotion for {task_id}? This fast-forward merges the task branch into the base branch."
                    ),
                ),
                ConfirmAction::RejectPromotion { task_id } => (
                    "Reject promotion",
                    format!("Mark the promotion artifact for {task_id} as rejected?"),
                ),
                ConfirmAction::StopDaemon => (
                    "Stop daemon",
                    "Stop the daemon? In-flight agent runs are terminated.".to_string(),
                ),
                ConfirmAction::RestartDaemon => (
                    "Restart daemon",
                    "Restart the daemon now?".to_string(),
                ),
                ConfirmAction::ReinitProject { alias } => (
                    "Re-initialize templates",
                    format!(
                        "Re-materialize control-plane files for '{alias}'? Existing files are backed up to .openloop/backup/."
                    ),
                ),
            };
            super::ui::confirm_modal(frame, frame.area(), &self.theme, title, &message);
        } else if let Some(picker) = &self.picker {
            let options: Vec<String> = picker
                .options
                .iter()
                .map(|s| s.as_str().to_string())
                .collect();
            super::ui::picker_modal(
                frame,
                frame.area(),
                &self.theme,
                "Set task status",
                &options,
                picker.selected,
            );
        } else {
            self.render_input_modal(frame, frame.area());
        }
    }

    /// Shared input modal for every text field in the app.
    fn render_input_modal(&self, frame: &mut Frame, area: Rect) {
        let title = match self.input_mode {
            InputMode::None => return,
            InputMode::AddProjectAlias | InputMode::AddProjectPath => "Add project",
            InputMode::AddTaskTitle => "Add task",
            InputMode::EditConfigValue => "Edit config",
            InputMode::RunPrompt | InputMode::RunModel => "Agent run",
        };
        let label = match self.input_mode {
            InputMode::AddProjectAlias => "Alias for the new project:",
            InputMode::AddProjectPath => "Path to the repository:",
            InputMode::AddTaskTitle => "Task title:",
            InputMode::EditConfigValue => "New value:",
            InputMode::RunPrompt => "Prompt:",
            InputMode::RunModel => "Model (empty = default):",
            InputMode::None => unreachable!(),
        };
        let mut title = title.to_string();
        if self.input_mode == InputMode::EditConfigValue {
            if let Some((key, _, _)) = self.config_entries.get(
                self.selected
                    .min(self.config_entries.len().saturating_sub(1)),
            ) {
                title = format!("Edit config — {key}");
            }
        }
        super::ui::input_modal(frame, area, &self.theme, &title, label, &self.input_value);
    }

    fn render_tab_bar(&mut self, frame: &mut Frame, area: Rect) {
        const ICONS: [&str; 9] = ["◆", "▤", "☑", "⚙", "⚌", "▶", "⇧", "⌁", "≡"];
        let mut spans: Vec<Span> = Vec::new();
        let mut spans_tab: Vec<(u16, u16)> = Vec::new();
        let mut column: u16 = area.x;
        for (index, (_key, label)) in TABS.iter().enumerate() {
            let selected = index == self.active_tab;
            let text = format!(" {} {} ", ICONS[index], label);
            let start = column;
            column += text.chars().count() as u16;
            spans_tab.push((start, column));
            spans.push(Span::styled(
                text.clone(),
                Style::default()
                    .fg(if selected {
                        self.theme.accent_bright
                    } else {
                        self.theme.text_dim
                    })
                    .bg(if selected {
                        self.theme.bg_selected
                    } else {
                        self.theme.bg_panel
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ));
            // Group separator after Dashboard (overview) and Run (operate).
            if index == 0 || index == 5 {
                spans.push(Span::styled(
                    " │ ",
                    Style::default()
                        .fg(self.theme.border)
                        .bg(self.theme.bg_panel),
                ));
                column += 3;
            } else {
                spans.push(Span::styled(" ", Style::default().bg(self.theme.bg_panel)));
                column += 1;
            }
        }
        self.tab_spans = spans_tab;
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(self.theme.bg_panel)),
            area,
        );
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status_text = if self.daemon.paused {
            "⏸ PAUSED"
        } else if self.daemon.pid > 0 {
            "● RUNNING"
        } else {
            "○ STOPPED"
        };
        let status_color = if self.daemon.paused {
            self.theme.yellow
        } else if self.daemon.pid > 0 {
            self.theme.green
        } else {
            self.theme.text_dim
        };
        let mut line_spans = vec![
            Span::styled(status_text, Style::default().fg(status_color)),
            Span::styled(" │ ", Style::default().fg(self.theme.text_dim)),
            Span::styled(
                self.active_alias
                    .clone()
                    .unwrap_or_else(|| "no project".into()),
                Style::default().fg(self.theme.accent),
            ),
            Span::styled(" │ ", Style::default().fg(self.theme.text_dim)),
            Span::styled(
                format!("${:.4} today", self.daemon.budget_spent_usd),
                Style::default().fg(self.theme.text),
            ),
        ];
        let right = format!("openloop v{} │ ? help │ q quit", self.version);
        // Right-align against the rendered area (TS justifyContent=space-between).
        let left_width: usize = line_spans.iter().map(|s| s.to_string().len()).sum();
        let pad = (area.width as usize).saturating_sub(left_width + right.len());
        line_spans.push(Span::raw(format!("{}{}", " ".repeat(pad), right)));
        let paragraph = Paragraph::new(Line::from(line_spans));
        frame.render_widget(
            paragraph.style(Style::default().bg(self.theme.bg_panel)),
            area,
        );
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.accent))
            .title(Span::styled(
                " Keyboard Shortcuts ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = centered_rect(area, 64, 26);
        frame.render_widget(ratatui::widgets::Clear, inner);
        let global: [(&str, &str); 8] = [
            ("1-9", "jump to tab"),
            ("Tab / S-Tab", "next / previous tab"),
            ("↑/↓ j/k", "navigate lists"),
            ("Enter", "select / confirm"),
            ("Esc", "cancel / back"),
            ("mouse", "wheel scrolls; click tabs to switch"),
            ("?", "toggle this help"),
            ("q", "quit"),
        ];
        let view_keys: [(&str, &[(&str, &str)]); 9] = [
            ("Dashboard", &[("↑/↓", "navigate")]),
            (
                "Projects",
                &[("Enter", "set active"), ("a", "add"), ("x", "remove")],
            ),
            (
                "Tasks",
                &[
                    ("a", "add"),
                    ("y", "approve"),
                    ("u", "set status"),
                    ("f", "filter"),
                    ("d", "delete"),
                    ("r", "recover stuck"),
                ],
            ),
            ("Service", &[("↑/↓", "select action"), ("Enter", "execute")]),
            (
                "Config",
                &[
                    ("Tab", "global/project"),
                    ("Enter", "edit"),
                    ("R", "reload"),
                ],
            ),
            ("Run", &[("Ctrl+R", "new run")]),
            (
                "Promotions",
                &[("Enter", "view artifact"), ("a", "apply"), ("r", "reject")],
            ),
            ("Events", &[("s", "cycle since"), ("p", "project filter")]),
            ("Logs", &[("Tab", "daemon/runs"), ("↑/↓", "select file")]),
        ];
        let mut lines: Vec<Line> = Vec::new();
        for (key, label) in global {
            lines.push(Line::from(vec![
                Span::styled(format!("{key:<14}"), Style::default().fg(self.theme.accent)),
                Span::styled(label.to_string(), Style::default().fg(self.theme.text)),
            ]));
        }
        lines.push(Line::from(""));
        for (view, keys) in view_keys {
            let mut spans = vec![Span::styled(
                format!("{view:<11}"),
                Style::default().fg(self.theme.accent_bright),
            )];
            for (index, (key, label)) in keys.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("  ·  "));
                }
                spans.push(Span::styled(
                    format!("[{key}] {label}"),
                    Style::default().fg(self.theme.text),
                ));
            }
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press ? to close",
            Style::default().fg(self.theme.text_dim),
        )));
        frame.render_widget(Paragraph::new(lines).block(block), inner);
    }

    // -----------------------------------------------------------------------
    // Views are implemented in views.rs; thin delegation keeps render() flat.
    // -----------------------------------------------------------------------

    fn render_dashboard(&mut self, frame: &mut Frame, area: Rect) {
        super::views::dashboard(self, frame, area);
    }
    fn render_projects(&mut self, frame: &mut Frame, area: Rect) {
        super::views::projects(self, frame, area);
    }
    fn render_tasks(&mut self, frame: &mut Frame, area: Rect) {
        super::views::tasks(self, frame, area);
    }
    fn render_service(&mut self, frame: &mut Frame, area: Rect) {
        super::views::service(self, frame, area);
    }
    fn render_config(&mut self, frame: &mut Frame, area: Rect) {
        super::views::config(self, frame, area);
    }
    fn render_run(&mut self, frame: &mut Frame, area: Rect) {
        super::views::run(self, frame, area);
    }
    fn render_promotions(&mut self, frame: &mut Frame, area: Rect) {
        super::views::promotions(self, frame, area);
    }
    fn render_events(&mut self, frame: &mut Frame, area: Rect) {
        super::views::events(self, frame, area);
    }
    fn render_logs(&mut self, frame: &mut Frame, area: Rect) {
        super::views::logs(self, frame, area);
    }

    // -----------------------------------------------------------------------
    // Key handling
    // -----------------------------------------------------------------------

    pub fn handle_key(&mut self, key: &KeyEvent) {
        // Confirmation modal captures everything.
        if self.confirm.is_some() {
            self.handle_confirm_key(key);
            return;
        }
        // Picker modal captures everything.
        if self.picker.is_some() {
            self.handle_picker_key(key);
            return;
        }
        // Input mode captures everything.
        match self.input_mode {
            InputMode::None => {}
            _ => {
                self.handle_input_key(key);
                return;
            }
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            _ => {}
        }

        match self.active_tab {
            0 => {}
            1 => self.handle_projects_key(key),
            2 => self.handle_tasks_key(key),
            3 => self.handle_service_key(key),
            4 => self.handle_config_key(key),
            5 => self.handle_run_key(key),
            6 => self.handle_promotions_key(key),
            7 => self.handle_events_key(key),
            _ => self.handle_logs_key(key),
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.selection_len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let current = self.selected as i32;
        let next = (current + delta).clamp(0, len as i32 - 1);
        self.selected = next as usize;
    }

    fn selection_len(&self) -> usize {
        match self.active_tab {
            0 | 1 => self.projects.len(),
            2 => self.filtered_tasks().len(),
            3 => super::views::SERVICE_ACTIONS.len(),
            4 => self.config_entries.len(),
            6 => self.promotions.len(),
            7 => self.events.len(),
            8 if self.log_tab => self.run_files.len(),
            _ => 0,
        }
    }

    fn handle_input_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::None;
                self.input_value.clear();
            }
            KeyCode::Enter => {
                self.submit_input();
            }
            KeyCode::Backspace => {
                self.input_value.pop();
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_value.push(ch);
            }
            _ => {}
        }
    }

    fn submit_input(&mut self) {
        let value = self.input_value.trim().to_string();
        match self.input_mode.clone() {
            InputMode::AddProjectAlias => {
                self.input_pending_alias = value;
                self.input_value.clear();
                self.input_mode = InputMode::AddProjectPath;
            }
            InputMode::AddProjectPath => {
                let alias = self.input_pending_alias.trim().to_string();
                if alias.is_empty() || value.is_empty() {
                    self.status_msg = "Alias and path are required".to_string();
                } else if !std::path::Path::new(&value).is_dir() {
                    self.status_msg = format!("Path is not a directory: {value}");
                } else {
                    // TS path.resolve parity: absolute + lexically normalized.
                    let resolved_str = std::path::absolute(&value)
                        .map(|p| {
                            c::fsutil::normalize_lexical_path(&p)
                                .to_string_lossy()
                                .to_string()
                        })
                        .unwrap_or_else(|_| value.clone());
                    match c::project_registry::add_project(&alias, &resolved_str, None) {
                        Ok(_) => {
                            self.status_msg = format!("Added: {alias} -> {resolved_str}");
                        }
                        Err(e) => self.status_msg = e,
                    }
                }
                self.input_mode = InputMode::None;
                self.input_value.clear();
                self.projects_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                self.git_states_refreshed = Instant::now() - std::time::Duration::from_secs(10);
            }
            InputMode::AddTaskTitle => {
                let Some(path) = self.active_project_path() else {
                    self.status_msg = "No active project selected".to_string();
                    self.input_mode = InputMode::None;
                    self.input_value.clear();
                    return;
                };
                if value.is_empty() {
                    self.status_msg = "Title is required".to_string();
                } else {
                    let now = c::now_iso();
                    let mut slug = String::new();
                    let mut prev_dash = false;
                    for ch in value.to_lowercase().chars() {
                        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
                            slug.push(ch);
                            prev_dash = false;
                        } else if !prev_dash {
                            slug.push('-');
                            prev_dash = true;
                        }
                    }
                    let id: String = slug.trim_matches('-').chars().take(60).collect();
                    let mut task = ProjectTask {
                        id: if id.is_empty() {
                            format!("task-{}", c::now_millis())
                        } else {
                            id.clone()
                        },
                        title: value.clone(),
                        kind: TaskKind::Feature,
                        status: TaskStatus::Proposed,
                        risk: TaskRisk::MediumRisk,
                        scope: None,
                        source: TaskSource {
                            source_type: "human".to_string(),
                            r#ref: "tui".to_string(),
                        },
                        spec_id: None,
                        branch: None,
                        owner: None,
                        acceptance_criteria: Vec::new(),
                        attempts: 0,
                        last_failure_signature: None,
                        promotion: TaskPromotion::ManualOnly,
                        promoted_at: None,
                        depends_on: None,
                        estimated_cost_usd: None,
                        notes: None,
                        last_run: None,
                        created_at: now.clone(),
                        updated_at: now,
                    };
                    match c::task_ledger::add_task(&path, &mut task) {
                        Ok(()) => self.status_msg = format!("Added: {}", task.id),
                        Err(e) => self.status_msg = e,
                    }
                }
                self.input_mode = InputMode::None;
                self.input_value.clear();
                self.ledger_refreshed = Instant::now() - std::time::Duration::from_secs(10);
            }
            InputMode::EditConfigValue => {
                let index = self
                    .selected
                    .min(self.config_entries.len().saturating_sub(1));
                if let Some((key_path, _, _)) = self.config_entries.get(index).cloned() {
                    let result = self.apply_config_edit(&key_path, &value);
                    match result {
                        Ok(()) => self.status_msg = format!("Updated: {key_path}"),
                        Err(e) => self.status_msg = e,
                    }
                }
                self.input_mode = InputMode::None;
                self.input_value.clear();
                self.config_refreshed = Instant::now() - std::time::Duration::from_secs(10);
            }
            InputMode::RunPrompt => {
                if value.is_empty() {
                    self.status_msg = "Prompt is required".to_string();
                    self.input_mode = InputMode::None;
                    self.input_value.clear();
                    return;
                }
                self.run_output.clear();
                // Char-boundary-safe truncation (TS slice semantics).
                let prefix: String = value.chars().take(80).collect();
                let suffix = if value.chars().count() > 80 {
                    "..."
                } else {
                    ""
                };
                self.run_output.push(format!("> Prompt: {prefix}{suffix}"));
                self.run_prompt_value = value;
                self.input_value.clear();
                self.input_mode = InputMode::RunModel;
            }
            InputMode::RunModel => {
                let model = if value.is_empty() { None } else { Some(value) };
                let prompt = self.run_prompt_value.clone();
                let project = self.active_project().cloned();
                self.input_mode = InputMode::None;
                self.input_value.clear();
                self.run_running = true;
                self.run_exit_code = None;
                self.run_output.push("> Running agent...".to_string());
                if let Some(project) = project {
                    std::thread::spawn(move || {
                        let result = c::pi::run_pi(&c::pi::PiRunOptions {
                            prompt: &prompt,
                            model: model.as_deref(),
                            project: &project,
                            timeout_ms: None,
                        });
                        RUN_RESULT.lock().replace(result);
                    });
                } else {
                    self.status_msg = "No active project selected".to_string();
                    self.run_running = false;
                }
            }
            InputMode::None => {}
        }
    }

    fn apply_config_edit(&self, key_path: &str, value: &str) -> Result<(), String> {
        let parsed: serde_json::Value = match value.parse::<f64>() {
            Ok(num) => serde_json::json!(num),
            Err(_) => serde_json::json!(value),
        };
        if !self.config_panel {
            let config = c::global_config::load_global_config(None)?;
            let mut raw = serde_json::to_value(config).unwrap_or_default();
            c::global_config::set_nested_json_value(&mut raw, key_path, parsed);
            let config: GlobalConfig = serde_json::from_value(raw).map_err(|e| e.to_string())?;
            c::global_config::save_global_config(&config, None)
        } else {
            let Some(path) = self.active_project_path() else {
                return Err("No active project".to_string());
            };
            let config = c::project_config::load_project_config(&path)?;
            let mut raw = serde_json::to_value(config).unwrap_or_default();
            c::global_config::set_nested_json_value(&mut raw, key_path, parsed);
            let config: ProjectConfig = serde_json::from_value(raw).map_err(|e| e.to_string())?;
            c::project_config::save_project_config(&path, &config)
        }
    }

    pub fn poll_run_result(&mut self) {
        if !self.run_running {
            return;
        }
        let mut guard = RUN_RESULT.lock();
        if let Some(result) = guard.take() {
            self.run_running = false;
            match result {
                Ok(result) => {
                    if !result.stdout.trim().is_empty() {
                        self.run_output.push(result.stdout.trim_end().to_string());
                    }
                    if !result.stderr.trim().is_empty() {
                        self.run_output
                            .push(format!("[stderr] {}", result.stderr.trim_end()));
                    }
                    self.run_exit_code = Some(result.exit_code);
                    self.run_output
                        .push(format!("> Exit code: {}", result.exit_code));
                }
                Err(e) => {
                    self.run_output.push(format!("> Error: {e}"));
                    self.status_msg = e;
                }
            }
        }
    }

    fn handle_projects_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                if let Some(project) = self.projects.get(self.selected) {
                    let alias = project.alias.clone();
                    let activated =
                        c::project_registry::get_project(&alias, None).and_then(|project| {
                            let mut config = c::global_config::load_global_config(None)?;
                            config.active_project_alias = Some(project.alias.clone());
                            c::global_config::save_global_config(&config, None)
                        });
                    match activated {
                        Ok(()) => {
                            self.active_alias = Some(alias.clone());
                            self.global_config_refreshed =
                                Instant::now() - std::time::Duration::from_secs(20);
                            self.status_msg = format!("Active project: {alias}");
                        }
                        Err(e) => self.status_msg = e,
                    }
                }
            }
            KeyCode::Char('x') => {
                if let Some(project) = self.projects.get(self.selected) {
                    self.confirm = Some(ConfirmAction::RemoveProject {
                        alias: project.alias.clone(),
                    });
                }
            }
            KeyCode::Char('a') => {
                self.input_mode = InputMode::AddProjectAlias;
                self.input_value.clear();
            }
            KeyCode::Char('i') => {
                if let Some(project) = self.projects.get(self.selected).cloned() {
                    match c::templates::initialize_project_from_templates(
                        &std::path::PathBuf::from("."),
                        &project,
                        false,
                    ) {
                        Ok(()) => {
                            let _ =
                                c::project_registry::mark_project_initialized(&project.alias, None);
                            self.status_msg = format!(
                                "Initialized {} — .openloop/ templates materialized",
                                project.alias
                            );
                            self.projects_refreshed =
                                Instant::now() - std::time::Duration::from_secs(10);
                        }
                        Err(e) => self.status_msg = e,
                    }
                }
            }
            KeyCode::Char('I') => {
                if let Some(project) = self.projects.get(self.selected) {
                    self.confirm = Some(ConfirmAction::ReinitProject {
                        alias: project.alias.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    pub fn filtered_tasks(&self) -> Vec<&ProjectTask> {
        self.ledger
            .tasks
            .iter()
            .filter(|t| self.task_filter.map(|f| t.status == f).unwrap_or(true))
            .collect()
    }

    fn handle_tasks_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Char('f') => {
                const STATUSES: [Option<TaskStatus>; 8] = [
                    None,
                    Some(TaskStatus::Proposed),
                    Some(TaskStatus::Planned),
                    Some(TaskStatus::Ready),
                    Some(TaskStatus::InProgress),
                    Some(TaskStatus::Done),
                    Some(TaskStatus::Blocked),
                    Some(TaskStatus::Failed),
                ];
                let current = STATUSES
                    .iter()
                    .position(|s| *s == self.task_filter)
                    .unwrap_or(0);
                self.task_filter = STATUSES[(current + 1) % STATUSES.len()];
                self.selected = 0;
            }
            KeyCode::Char('a') => {
                self.input_mode = InputMode::AddTaskTitle;
                self.input_value.clear();
            }
            KeyCode::Char('d') => {
                if let Some(task) = self.filtered_tasks().get(self.selected) {
                    self.confirm = Some(ConfirmAction::DeleteTask {
                        task_id: task.id.clone(),
                    });
                }
            }
            KeyCode::Char('y') => {
                let Some(task) = self.filtered_tasks().get(self.selected).cloned() else {
                    return;
                };
                if task.status != TaskStatus::AwaitingApproval {
                    self.status_msg = format!(
                        "Task {} is not awaiting approval (current: {})",
                        task.id,
                        task.status.as_str()
                    );
                    return;
                }
                let Some(path) = self.active_project_path() else {
                    return;
                };
                let result = c::task_ledger::with_task_ledger(&path, |ledger| {
                    let found = ledger
                        .tasks
                        .iter_mut()
                        .find(|t| t.id == task.id)
                        .ok_or_else(|| format!("Task {} not found", task.id))?;
                    if found.status != TaskStatus::AwaitingApproval {
                        return Err(format!("Task {} is no longer awaiting approval", found.id));
                    }
                    found.status = TaskStatus::Ready;
                    let mut notes = found.notes.clone().unwrap_or_default();
                    notes.push("Approved via TUI.".to_string());
                    found.notes = Some(notes);
                    found.updated_at = c::now_iso();
                    Ok(found.id.clone())
                });
                match result {
                    Ok(id) => {
                        self.status_msg = format!("Approved {id} — now ready for implementation");
                        self.ledger_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                    }
                    Err(e) => self.status_msg = e,
                }
            }
            KeyCode::Char('u') => {
                let Some(task) = self
                    .filtered_tasks()
                    .get(self.selected)
                    .map(|task| (task.id.clone(), task.status))
                else {
                    return;
                };
                let (task_id, task_status) = task;
                let options = vec![
                    TaskStatus::Proposed,
                    TaskStatus::Planned,
                    TaskStatus::Ready,
                    TaskStatus::AwaitingApproval,
                    TaskStatus::Blocked,
                    TaskStatus::Failed,
                    TaskStatus::Done,
                ];
                self.picker = Some(PickerState {
                    task_id,
                    selected: options.iter().position(|s| *s == task_status).unwrap_or(0),
                    options,
                });
            }
            KeyCode::Char('r') => {
                if let Some(path) = self.active_project_path() {
                    let stuck: Vec<String> = self
                        .ledger
                        .tasks
                        .iter()
                        .filter(|t| t.status == TaskStatus::InProgress)
                        .map(|t| t.id.clone())
                        .collect();
                    for task_id in &stuck {
                        let _ = c::task_ledger::update_task(
                            &path,
                            task_id,
                            &c::task_ledger::TaskPatch {
                                status: Some(TaskStatus::Ready),
                                ..Default::default()
                            },
                        );
                    }
                    self.status_msg = format!("Recovered {} stuck tasks", stuck.len());
                    self.ledger_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                }
            }
            _ => {}
        }
    }

    fn handle_service_key(&mut self, key: &KeyEvent) {
        const ACTIONS: [&str; 7] = super::views::SERVICE_ACTIONS;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.service_action = self.service_action.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.service_action = (self.service_action + 1).min(ACTIONS.len() - 1);
            }
            KeyCode::Enter => {
                let action = ACTIONS[self.service_action.min(ACTIONS.len() - 1)];
                match action {
                    "Start daemon" => {
                        match c::service_control::start_service() {
                            Ok(msg) => self.status_msg = msg,
                            Err(e) => self.status_msg = e,
                        }
                        self.daemon_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                    }
                    "Stop daemon" => {
                        self.confirm = Some(ConfirmAction::StopDaemon);
                    }
                    "Restart daemon" => {
                        self.confirm = Some(ConfirmAction::RestartDaemon);
                    }
                    "Pause" => {
                        if c::daemon_state::pause_daemon(None).is_ok() {
                            self.status_msg = "Daemon paused".to_string();
                        }
                    }
                    "Resume" => {
                        if c::daemon_state::resume_daemon(None).is_ok() {
                            self.status_msg = "Daemon resumed".to_string();
                        }
                    }
                    "Run one iteration now" => {
                        if self.tick_running {
                            self.status_msg = "A tick is already running".to_string();
                        } else {
                            self.tick_running = true;
                            self.status_msg = "Running one worker iteration…".to_string();
                            std::thread::spawn(|| {
                                let result =
                                    c::worker::run_worker_tick(&c::now_iso()).map(|state| {
                                        format!(
                                            "Tick done — {} projects, current run: {}",
                                            state.projects.len(),
                                            state
                                                .current_run
                                                .as_ref()
                                                .map(|r| r
                                                    .task_id
                                                    .clone()
                                                    .unwrap_or(r.mode.clone()))
                                                .unwrap_or_else(|| "none".into())
                                        )
                                    });
                                TICK_RESULT.lock().replace(result);
                            });
                        }
                    }
                    _ => {
                        // View daemon logs: jump to the Logs tab.
                        self.switch_tab(8);
                        self.log_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Tab => {
                self.config_panel = !self.config_panel;
                self.selected = 0;
            }
            KeyCode::Enter => {
                if let Some((_, value, editable)) = self
                    .config_entries
                    .get(
                        self.selected
                            .min(self.config_entries.len().saturating_sub(1)),
                    )
                    .cloned()
                {
                    if editable {
                        self.input_value = value;
                        self.input_mode = InputMode::EditConfigValue;
                    }
                }
            }
            KeyCode::Char('R') => {
                self.config_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                self.status_msg = "Reloaded".to_string();
            }
            _ => {}
        }
    }

    fn handle_run_key(&mut self, key: &KeyEvent) {
        if self.run_running {
            return;
        }
        if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.input_mode = InputMode::RunPrompt;
            self.input_value.clear();
            self.run_output.clear();
            self.run_exit_code = None;
        }
    }

    fn handle_promotions_key(&mut self, key: &KeyEvent) {
        let Some(path) = self.active_project_path() else {
            return;
        };
        match key.code {
            KeyCode::Enter => {
                if let Some(item) = self.promotions.get(self.selected) {
                    match c::promotion_queue::get_promotion_detail(&path, &item.artifact.task_id) {
                        Ok(detail) => {
                            self.promotion_detail =
                                serde_json::to_string_pretty(&detail).unwrap_or_default();
                        }
                        Err(_) => self.promotion_detail.clear(),
                    }
                }
            }
            KeyCode::Char('a') => {
                if let Some(item) = self.promotions.get(self.selected) {
                    self.confirm = Some(ConfirmAction::ApplyPromotion {
                        task_id: item.artifact.task_id.clone(),
                    });
                }
            }
            KeyCode::Char('r') => {
                if let Some(item) = self.promotions.get(self.selected) {
                    self.confirm = Some(ConfirmAction::RejectPromotion {
                        task_id: item.artifact.task_id.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    fn handle_events_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Char('s') => {
                self.events_since_idx = (self.events_since_idx + 1) % 4;
                self.events_refreshed = Instant::now() - std::time::Duration::from_secs(10);
            }
            KeyCode::Char('p') => {
                self.events_project_filter = if self.events_project_filter.is_some() {
                    None
                } else {
                    self.active_alias.clone()
                };
                self.events_refreshed = Instant::now() - std::time::Duration::from_secs(10);
            }
            _ => {}
        }
    }

    fn handle_logs_key(&mut self, key: &KeyEvent) {
        if key.code == KeyCode::Tab {
            self.log_tab = !self.log_tab;
            self.selected = 0;
            self.table_state = TableState::default();
        }
    }

    // -----------------------------------------------------------------------
    // Confirmation + picker + mouse
    // -----------------------------------------------------------------------

    fn handle_confirm_key(&mut self, key: &KeyEvent) {
        let Some(action) = self.confirm.take() else {
            return;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                self.status_msg = "Cancelled".to_string();
            }
            KeyCode::Char('y') | KeyCode::Enter => self.execute_confirmed(action),
            _ => {
                // Any other key re-arms the modal.
                self.confirm = Some(action);
            }
        }
    }

    fn execute_confirmed(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::DeleteTask { task_id } => {
                if let Some(path) = self.active_project_path() {
                    match c::task_ledger::remove_task(&path, &task_id) {
                        Ok(()) => {
                            self.status_msg = format!("Removed: {task_id}");
                            self.selected = 0;
                            self.ledger_refreshed =
                                Instant::now() - std::time::Duration::from_secs(10);
                        }
                        Err(e) => self.status_msg = e,
                    }
                }
            }
            ConfirmAction::RemoveProject { alias } => {
                match c::project_registry::remove_project(&alias, None) {
                    Ok(()) => {
                        self.status_msg = format!("Removed project: {alias}");
                        if self.active_alias.as_deref() == Some(alias.as_str()) {
                            self.active_alias = None;
                        }
                        self.selected = 0;
                        self.projects_refreshed =
                            Instant::now() - std::time::Duration::from_secs(10);
                    }
                    Err(e) => self.status_msg = e,
                }
            }
            ConfirmAction::ApplyPromotion { task_id } => {
                if let Some(path) = self.active_project_path() {
                    match c::promotion_queue::apply_promotion_artifact(&path, &task_id, None) {
                        Ok(_) => {
                            self.status_msg = format!("Applied: {task_id}");
                            self.promotions_refreshed =
                                Instant::now() - std::time::Duration::from_secs(10);
                        }
                        Err(e) => self.status_msg = e,
                    }
                }
            }
            ConfirmAction::RejectPromotion { task_id } => {
                if let Some(path) = self.active_project_path() {
                    match c::promotion_queue::update_promotion_artifact(
                        &path,
                        &task_id,
                        PromotionArtifactStatus::Rejected,
                        None,
                    ) {
                        Ok(_) => {
                            self.status_msg = format!("Rejected: {task_id}");
                            self.promotions_refreshed =
                                Instant::now() - std::time::Duration::from_secs(10);
                        }
                        Err(e) => self.status_msg = e,
                    }
                }
            }
            ConfirmAction::StopDaemon => {
                match c::service_control::stop_service() {
                    Ok(msg) => self.status_msg = msg,
                    Err(e) => self.status_msg = e,
                }
                self.daemon_refreshed = Instant::now() - std::time::Duration::from_secs(10);
            }
            ConfirmAction::RestartDaemon => {
                self.status_msg = "Restarting daemon…".to_string();
                match c::service_control::restart_service() {
                    Ok(msg) => self.status_msg = msg,
                    Err(e) => self.status_msg = e,
                }
                self.daemon_refreshed = Instant::now() - std::time::Duration::from_secs(10);
            }
            ConfirmAction::ReinitProject { alias } => {
                let result = c::project_registry::get_project(&alias, None).and_then(|project| {
                    c::templates::initialize_project_from_templates(
                        &std::path::PathBuf::from("."),
                        &project,
                        true,
                    )?;
                    c::project_registry::mark_project_initialized(&alias, None)
                });
                match result {
                    Ok(_) => {
                        self.status_msg =
                            format!("Re-initialized {alias} — backup in .openloop/backup/");
                        self.projects_refreshed =
                            Instant::now() - std::time::Duration::from_secs(10);
                    }
                    Err(e) => self.status_msg = e,
                }
            }
        }
    }

    fn handle_picker_key(&mut self, key: &KeyEvent) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.picker = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                picker.selected = picker.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                picker.selected = (picker.selected + 1).min(picker.options.len() - 1);
            }
            KeyCode::Enter => {
                let picker_state = self.picker.take().unwrap_or_else(|| PickerState {
                    task_id: String::new(),
                    options: Vec::new(),
                    selected: 0,
                });
                let Some(path) = self.active_project_path() else {
                    return;
                };
                let status = picker_state.options[picker_state.selected];
                match c::task_ledger::update_task(
                    &path,
                    &picker_state.task_id,
                    &c::task_ledger::TaskPatch {
                        status: Some(status),
                        ..Default::default()
                    },
                ) {
                    Ok(task) => {
                        self.status_msg = format!("Task {} → {}", task.id, task.status.as_str());
                        self.ledger_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                    }
                    Err(e) => self.status_msg = e,
                }
            }
            _ => {}
        }
    }

    /// Mouse support: wheel scrolls the focused list, clicks on the tab bar
    /// switch views.
    pub fn handle_mouse(&mut self, mouse: &MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.picker.is_some() {
                    if let Some(picker) = self.picker.as_mut() {
                        picker.selected = picker.selected.saturating_sub(1);
                    }
                } else if !self.input_capturing() && self.confirm.is_none() {
                    self.move_selection(-1);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.picker.is_some() {
                    if let Some(picker) = self.picker.as_mut() {
                        picker.selected =
                            (picker.selected + 1).min(picker.options.len().saturating_sub(1));
                    }
                } else if !self.input_capturing() && self.confirm.is_none() {
                    self.move_selection(1);
                }
            }
            MouseEventKind::Down(_)
                if mouse.row == 0 && !self.input_capturing() && self.picker.is_none() =>
            {
                if let Some(index) = self
                    .tab_spans
                    .iter()
                    .position(|(start, end)| mouse.column >= *start && mouse.column < *end)
                {
                    self.switch_tab(index);
                }
            }
            _ => {}
        }
    }
}

pub static RUN_RESULT: parking_lot::Mutex<Option<Result<AgentRunResult, String>>> =
    parking_lot::Mutex::new(None);

impl Default for DaemonState {
    fn default() -> Self {
        c::daemon_state::create_default_daemon_state()
    }
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}
