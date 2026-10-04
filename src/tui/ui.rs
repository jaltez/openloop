// Shared TUI components: panels, stat cards, gauges, key-hint chips, and the
// centered modals (input / confirm / picker). Every view composes these
// instead of hand-rolling boxes, so spacing and styling stay uniform.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};
use ratatui::Frame;

use super::theme::Theme;

/// Bordered, padded, titled panel used as the container of every section.
pub fn panel<'a>(theme: &Theme, title: &str) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1))
}

/// Like [`panel`] but without a title, for pure chrome.
pub fn panel_untitled<'a>(theme: &Theme) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .padding(Padding::horizontal(1))
}

/// A small stat card: dim label on top, styled lines below.
pub fn stat_card(frame: &mut Frame, area: Rect, theme: &Theme, label: &str, lines: Vec<Line>) {
    let mut all = vec![Line::from(Span::styled(
        label.to_string(),
        Style::default().fg(theme.text_dim),
    ))];
    all.extend(lines);
    frame.render_widget(Paragraph::new(all).block(panel_untitled(theme)), area);
}

/// Unicode progress bar, e.g. `██████░░░░░░░░░░░░ 32%`.
pub fn bar(fraction: f64, width: usize) -> String {
    let clamped = fraction.clamp(0.0, 1.0);
    let filled = (clamped * width as f64).round() as usize;
    format!(
        "{}{}",
        "█".repeat(filled.min(width)),
        "░".repeat(width.saturating_sub(filled))
    )
}

/// `[key] label` chip row for per-view shortcuts.
pub fn key_hint_line(theme: &Theme, hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (key, label) in hints {
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            format!("[{key}]"),
            Style::default()
                .fg(theme.bg_panel)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(theme.text_dim),
        ));
    }
    Line::from(spans)
}

/// Centered sub-rectangle with a maximum size, clamped to the parent area.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

/// Modal frame with Clear underlay and a styled title.
fn modal_block<'a>(theme: &Theme, title: &str, danger: bool) -> Block<'a> {
    let accent = if danger { theme.red } else { theme.accent };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1))
}

/// Shared text-input modal. Renders label + current value + a cursor hint.
pub fn input_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    label: &str,
    value: &str,
) {
    let modal = centered(area, 56, 5);
    frame.render_widget(Clear, modal);
    let inner = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(modal_block(theme, title, false).inner(modal));
    frame.render_widget(
        Paragraph::new(Span::styled(label, Style::default().fg(theme.text_dim))),
        inner[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("{value}▏"),
            Style::default().fg(theme.text),
        )),
        inner[1],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Enter confirm · Esc cancel",
            Style::default().fg(theme.text_dim),
        )),
        inner[2],
    );
}

/// Shared yes/no confirmation modal for destructive or git-mutating actions.
pub fn confirm_modal(frame: &mut Frame, area: Rect, theme: &Theme, title: &str, message: &str) {
    let modal = centered(area, 64, 6);
    frame.render_widget(Clear, modal);
    let inner = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(1)])
        .split(modal_block(theme, title, true).inner(modal));
    frame.render_widget(
        Paragraph::new(Span::styled(message, Style::default().fg(theme.text)))
            .wrap(Wrap { trim: true }),
        inner[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "y confirm · Esc cancel",
            Style::default().fg(theme.text_dim),
        )),
        inner[1],
    );
}

/// Shared option-picker modal (e.g. task status). Options render with the
/// selected one highlighted; arrow keys move, Enter picks.
pub fn picker_modal(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    options: &[String],
    selected: usize,
) {
    let height = (options.len() as u16 + 4).min(area.height);
    let modal = centered(area, 44, height);
    frame.render_widget(Clear, modal);
    let inner = modal_block(theme, title, false).inner(modal);
    let lines: Vec<Line> = options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let is_selected = index == selected;
            Line::from(Span::styled(
                format!("{} {option}", if is_selected { "▸" } else { " " }),
                Style::default()
                    .fg(if is_selected {
                        theme.accent_bright
                    } else {
                        theme.text
                    })
                    .bg(if is_selected {
                        theme.bg_selected
                    } else {
                        ratatui::style::Color::Reset
                    }),
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "↑/↓ choose · Enter apply · Esc cancel",
            Style::default().fg(theme.text_dim),
        )),
        Rect {
            x: modal.x + 1,
            y: modal.y + modal.height.saturating_sub(2),
            width: modal.width.saturating_sub(2),
            height: 1,
        },
    );
}

/// Directory browser modal for picking a repository path. `entries` are
/// (name, is_dir) pairs already filtered and sorted; `selected` highlights
/// the current row; `filter` narrows by substring as the user types.
/// Bundle of browser render inputs (keeps `browser_modal` under the
/// clippy argument limit).
pub struct BrowserView<'a> {
    pub cwd: &'a std::path::Path,
    pub entries: &'a [(String, bool)],
    pub selected: usize,
    pub filter: &'a str,
    pub show_hidden: bool,
}

pub fn browser_modal(frame: &mut Frame, area: Rect, theme: &Theme, view: BrowserView<'_>) {
    let BrowserView {
        cwd,
        entries,
        selected,
        filter,
        show_hidden,
    } = view;
    let modal = centered(area, 72, 22);
    frame.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Span::styled(
            " Browse — pick the repository directory ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .padding(Padding::horizontal(1));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);

    let mut lines: Vec<Line> = Vec::new();
    // Path header, left-truncated so the tail (the discriminating part) stays.
    let cwd_str = cwd.to_string_lossy().to_string();
    let header_width = inner.width.saturating_sub(2) as usize;
    let header: String = if cwd_str.chars().count() > header_width {
        format!(
            "…{}",
            cwd_str
                .chars()
                .skip(cwd_str.chars().count() - header_width + 1)
                .collect::<String>()
        )
    } else {
        cwd_str
    };
    lines.push(Line::from(Span::styled(
        header,
        Style::default().fg(theme.accent_bright),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "{} entr{} · {}hidden files shown",
            entries.len(),
            if entries.len() == 1 { "y" } else { "ies" },
            if show_hidden { "" } else { "no " }
        ),
        Style::default().fg(theme.text_dim),
    )));
    lines.push(Line::from(""));

    let visible = inner.height.saturating_sub(6) as usize;
    let offset = selected.saturating_sub(visible.saturating_sub(1));
    for (index, (name, is_dir)) in entries.iter().enumerate().skip(offset).take(visible.max(1)) {
        let is_selected = index == selected;
        let icon = if *is_dir { "▸" } else { " " };
        let style = Style::default()
            .fg(if *is_dir { theme.cyan } else { theme.text_dim })
            .bg(if is_selected {
                theme.bg_selected
            } else {
                ratatui::style::Color::Reset
            });
        lines.push(Line::from(Span::styled(
            format!("{icon} {name}{}", if *is_dir { "/" } else { "" }),
            style,
        )));
    }
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no matching entries)",
            Style::default().fg(theme.text_dim),
        )));
    }

    // Filter line + footer hints.
    let mut footer = vec![Line::from(Span::styled(
        if filter.is_empty() {
            "filter: (type to jump)".to_string()
        } else {
            format!("filter: {filter}▏")
        },
        Style::default().fg(if filter.is_empty() {
            theme.text_dim
        } else {
            theme.yellow
        }),
    ))];
    footer.push(Line::from(Span::styled(
        "Enter accept dir · → descend · ←/Bs parent · Ctrl+H hidden · Esc cancel · type to filter",
        Style::default().fg(theme.text_dim),
    )));
    let content = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);
    frame.render_widget(Paragraph::new(lines), content[0]);
    frame.render_widget(Paragraph::new(footer), content[1]);
}
