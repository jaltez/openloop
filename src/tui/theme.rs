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
    /// Palette adapted from https://olp.sh/ (void/panel obsidian surfaces,
    /// warm LED text, hairline dividers, phosphor amber accent, mint ok, coral stop).
    pub fn dark() -> Theme {
        Theme {
            bg: Color::Rgb(0x06, 0x06, 0x07),             // --color-void
            bg_panel: Color::Rgb(0x0d, 0x0d, 0x10),       // --color-panel
            bg_selected: Color::Rgb(0x1f, 0x1c, 0x15),    // warm amber-tinted row highlight
            border: Color::Rgb(0x26, 0x26, 0x2b),         // --color-line
            border_focused: Color::Rgb(0xff, 0xb0, 0x00), // --color-amber
            text: Color::Rgb(0xf4, 0xf1, 0xea),           // --color-led
            text_dim: Color::Rgb(0x94, 0x92, 0x9a),       // --color-dim
            text_muted: Color::Rgb(0x66, 0x64, 0x6c),     // --color-faint
            accent: Color::Rgb(0xff, 0xb0, 0x00),         // --color-amber
            accent_bright: Color::Rgb(0xff, 0xd1, 0x66),  // luminous amber highlight
            green: Color::Rgb(0x3d, 0xf0, 0x8a),          // --color-ok
            yellow: Color::Rgb(0xff, 0xb0, 0x00),         // --color-amber
            red: Color::Rgb(0xff, 0x5a, 0x4e),            // --color-stop
            cyan: Color::Rgb(0xff, 0xb0, 0x00),           // --color-amber (run/active)
            orange: Color::Rgb(0xff, 0x8a, 0x3d),         // amber-coral (high-risk)
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
