use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use super::theme::{status_color, Theme};
use crate::core as c;
use c::types::*;

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
}

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
        self.projects.iter().find(|p| Some(&p.alias) == self.active_alias.as_ref())
    }

    pub fn active_project_path(&self) -> Option<PathBuf> {
        self.active_project().map(|p| PathBuf::from(&p.path))
    }

    pub fn input_capturing(&self) -> bool {
        self.input_mode != InputMode::None
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
        // Tab-gated polling: only the data source the active view renders is
        // refreshed (TS polls per-view via onCleanup; the port used to poll
        // everything on every tab).
        let tab_needs_ledger = self.active_tab == 2; // Tasks
        let tab_needs_promotions = self.active_tab == 6; // Promotions
        let tab_needs_config = self.active_tab == 4; // Config
        let tab_needs_events = self.active_tab == 7; // Events
        let tab_needs_logs = self.active_tab == 3 || self.active_tab == 8; // Service / Logs
        if let Some(path) = self.active_project_path() {
            if tab_needs_ledger && self.ledger_refreshed.elapsed() >= std::time::Duration::from_secs(2) {
                self.ledger = c::task_ledger::load_task_ledger(&path).unwrap_or(TaskLedger {
                    version: 1,
                    updated_at: String::new(),
                    tasks: Vec::new(),
                });
                self.ledger_refreshed = Instant::now();
            }
            if tab_needs_promotions && self.promotions_refreshed.elapsed() >= std::time::Duration::from_secs(5) {
                self.promotions =
                    c::promotion_queue::list_promotion_artifacts(&path).unwrap_or_default();
                self.promotions_refreshed = Instant::now();
            }
            if tab_needs_config && self.config_refreshed.elapsed() >= std::time::Duration::from_secs(5) {
                self.reload_config_entries();
                self.config_refreshed = Instant::now();
            }
        }
        if tab_needs_events && self.events_refreshed.elapsed() >= std::time::Duration::from_secs(3) {
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
            let file = self.run_files.get(self.selected.min(self.run_files.len().saturating_sub(1)));
            self.run_file_content = match file {
                Some(file) => std::fs::read_to_string(runs_dir.join(file)).unwrap_or_else(|_| "(unable to read file)".into()),
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
            fn walk(value: &serde_json::Value, prefix: &str, out: &mut Vec<(String, String, bool)>) {
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
        if self.show_help {
            self.render_help(frame, frame.area());
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();
        for (index, (key, label)) in TABS.iter().enumerate() {
            let selected = index == self.active_tab;
            spans.push(Span::styled(
                format!("{key} {label}  "),
                Style::default()
                    .fg(if selected {
                        self.theme.accent_bright
                    } else {
                        self.theme.text_dim
                    })
                    .bg(if selected { self.theme.bg_selected } else { self.theme.bg_panel })
                    .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() }),
            ));
        }
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
                self.active_alias.clone().unwrap_or_else(|| "no project".into()),
                Style::default().fg(self.theme.accent),
            ),
            Span::styled(" │ ", Style::default().fg(self.theme.text_dim)),
            Span::styled(
                format!("${:.4} today", self.daemon.budget_spent_usd),
                Style::default().fg(self.theme.text),
            ),
        ];
        let right = format!("openloop v{} │ ? help │ q quit", self.version);
        line_spans.push(Span::raw(format!(
            "{:>width$}",
            right,
            width = (right.len() + 2) as usize
        )));
        let paragraph = Paragraph::new(Line::from(line_spans));
        frame.render_widget(paragraph.style(Style::default().bg(self.theme.bg_panel)), area);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.accent))
            .title(Span::styled(" Keyboard Shortcuts ", Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD)));
        let inner = centered_rect(area, 50, 18);
        frame.render_widget(ratatui::widgets::Clear, inner);
        let lines = vec![
            Line::from(Span::styled("1-9  Jump to tab", Style::default().fg(self.theme.text))),
            Line::from(Span::styled("Tab  Next tab", Style::default().fg(self.theme.text))),
            Line::from(Span::styled("S-Tab  Previous tab", Style::default().fg(self.theme.text))),
            Line::from(Span::styled("↑/↓ j/k  Navigate lists", Style::default().fg(self.theme.text))),
            Line::from(Span::styled("Enter  Select / confirm", Style::default().fg(self.theme.text))),
            Line::from(Span::styled("Esc  Cancel / back", Style::default().fg(self.theme.text))),
            Line::from(Span::styled("?  Toggle this help", Style::default().fg(self.theme.text))),
            Line::from(Span::styled("q  Quit", Style::default().fg(self.theme.text))),
            Line::from(""),
            Line::from(Span::styled(
                "View-specific shortcuts shown at bottom of each view",
                Style::default().fg(self.theme.text_dim),
            )),
            Line::from(Span::styled("Press ? to close", Style::default().fg(self.theme.text_dim))),
        ];
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
        let len = match self.active_tab {
            0 | 1 => self.projects.len(),
            2 => self.filtered_tasks().len(),
            6 => self.promotions.len(),
            _ if self.active_tab == 8 && self.log_tab => self.run_files.len(),
            4 => self.config_entries.len(),
            _ => 0,
        };
        if len == 0 {
            self.selected = 0;
            return;
        }
        let current = self.selected as i32;
        let next = (current + delta).clamp(0, len as i32 - 1);
        self.selected = next as usize;
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
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.input_value.push(ch);
                }
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
                } else {
                    match c::project_registry::add_project(&alias, &value, None) {
                        Ok(_) => self.status_msg = format!("Added: {alias}"),
                        Err(e) => self.status_msg = e,
                    }
                }
                self.input_mode = InputMode::None;
                self.input_value.clear();
                self.projects_refreshed = Instant::now() - std::time::Duration::from_secs(10);
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
                        id: if id.is_empty() { format!("task-{}", c::now_millis()) } else { id.clone() },
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
                let index = self.selected.min(self.config_entries.len().saturating_sub(1));
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
                self.run_output.clear();
                self.run_output.push(format!("> Prompt: {}", &value[..value.len().min(80)]));
                let _ = &value;
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
                        self.run_output.push(format!("[stderr] {}", result.stderr.trim_end()));
                    }
                    self.run_exit_code = Some(result.exit_code);
                    self.run_output.push(format!("> Exit code: {}", result.exit_code));
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
                    if let Ok(mut config) = c::global_config::load_global_config(None) {
                        config.active_project_alias = Some(alias.clone());
                        if c::global_config::save_global_config(&config, None).is_ok() {
                            self.active_alias = Some(alias.clone());
                            self.status_msg = format!("Activated: {alias}");
                        }
                    }
                }
            }
            KeyCode::Char('a') => {
                self.input_mode = InputMode::AddProjectAlias;
                self.input_value.clear();
            }
            KeyCode::Char('d') => {
                if let Some(project) = self.projects.get(self.selected) {
                    let alias = project.alias.clone();
                    if c::project_registry::remove_project(&alias, None).is_ok() {
                        self.status_msg = format!("Removed: {alias}");
                        self.selected = 0;
                        self.projects_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                    }
                }
            }
            KeyCode::Char('i') => {
                if let Some(project) = self.projects.get(self.selected).cloned() {
                    if c::templates::initialize_project_from_templates(&std::path::PathBuf::from("."), &project, false).is_ok() {
                        let _ = c::project_registry::mark_project_initialized(&project.alias, None);
                        self.status_msg = format!("Initialized: {}", project.alias);
                    }
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
                let current = STATUSES.iter().position(|s| *s == self.task_filter).unwrap_or(0);
                self.task_filter = STATUSES[(current + 1) % STATUSES.len()];
                self.selected = 0;
            }
            KeyCode::Char('a') => {
                self.input_mode = InputMode::AddTaskTitle;
                self.input_value.clear();
            }
            KeyCode::Char('d') => {
                if let (Some(path), Some(task)) = (
                    self.active_project_path(),
                    self.filtered_tasks().get(self.selected),
                ) {
                    let task_id = task.id.clone();
                    if c::task_ledger::remove_task(&path, &task_id).is_ok() {
                        self.status_msg = format!("Removed: {task_id}");
                        self.selected = 0;
                        self.ledger_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                    }
                }
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
        const ACTIONS: [&str; 3] = ["Pause", "Resume", "View Logs"];
        match key.code {
            KeyCode::Enter => match ACTIONS[self.service_action.min(2)] {
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
                _ => {
                    self.log_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                }
            },
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
                if let Some((_, value, editable)) =
                    self.config_entries.get(self.selected.min(self.config_entries.len().saturating_sub(1))).cloned()
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
                    let task_id = item.artifact.task_id.clone();
                    match c::promotion_queue::apply_promotion_artifact(&path, &task_id, None) {
                        Ok(_) => {
                            self.status_msg = format!("Applied: {task_id}");
                            self.promotions_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                        }
                        Err(e) => self.status_msg = e,
                    }
                }
            }
            KeyCode::Char('r') => {
                if let Some(item) = self.promotions.get(self.selected) {
                    let task_id = item.artifact.task_id.clone();
                    match c::promotion_queue::update_promotion_artifact(
                        &path,
                        &task_id,
                        PromotionArtifactStatus::Rejected,
                        None,
                    ) {
                        Ok(_) => {
                            self.status_msg = format!("Rejected: {task_id}");
                            self.promotions_refreshed = Instant::now() - std::time::Duration::from_secs(10);
                        }
                        Err(e) => self.status_msg = e,
                    }
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

pub fn status_color_for(theme: &Theme, status: &str) -> Color {
    status_color(theme, status)
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
