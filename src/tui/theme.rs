use ratatui::style::Color;

/// Full palette definition; some colors are reserved for future views and
/// kept so the theme stays the single source of truth.
#[allow(dead_code)]
pub struct Theme {
    pub bg: Color,
    pub bg_panel: Color,
    pub bg_selected: Color,
    pub border: Color,
    pub border_focused: Color,
    pub text: Color,
    pub text_dim: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_bright: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub cyan: Color,
    pub orange: Color,
}

impl Theme {
    pub fn dark() -> Theme {
        Theme {
            bg: Color::Rgb(0x1a, 0x1a, 0x2e),
            bg_panel: Color::Rgb(0x16, 0x21, 0x3e),
            bg_selected: Color::Rgb(0x0f, 0x34, 0x60),
            border: Color::Rgb(0x33, 0x41, 0x55),
            border_focused: Color::Rgb(0x53, 0xa8, 0xb6),
            text: Color::Rgb(0xe2, 0xe8, 0xf0),
            text_dim: Color::Rgb(0x64, 0x74, 0x8b),
            text_muted: Color::Rgb(0x47, 0x55, 0x69),
            accent: Color::Rgb(0x53, 0xa8, 0xb6),
            accent_bright: Color::Rgb(0x79, 0xd4, 0xe2),
            green: Color::Rgb(0x22, 0xc5, 0x5e),
            yellow: Color::Rgb(0xea, 0xb3, 0x08),
            red: Color::Rgb(0xef, 0x44, 0x44),
            cyan: Color::Rgb(0x06, 0xb6, 0xd4),
            orange: Color::Rgb(0xf9, 0x73, 0x16),
        }
    }
}

pub fn status_color(theme: &Theme, status: &str) -> Color {
    match status {
        "done" | "promoted" | "applied" | "running" => theme.green,
        "in_progress" | "ready" | "planned" | "pending" => theme.accent,
        "blocked" | "failed" | "rejected" => theme.red,
        "awaiting-approval" | "paused" => theme.yellow,
        _ => theme.text_dim,
    }
}
