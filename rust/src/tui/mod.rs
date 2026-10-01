mod app;
mod theme;
mod views;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, event};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::core as c;

pub fn run() -> Result<(), String> {
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let result = event_loop(&mut terminal);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    result
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<(), String> {
    let mut app = app::App::new(c::types::VERSION.to_string());

    loop {
        // Refresh polling state as needed per view interval.
        app.refresh_if_needed();

        terminal.draw(|frame| app.render(frame)).map_err(|e| e.to_string())?;

        if event::poll(std::time::Duration::from_millis(200)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                if key.kind == crossterm::event::KeyEventKind::Press {
                    // Global quit: q / Ctrl+C
                    if app.should_quit() {
                        return Ok(());
                    }
                    if handle_global_keys(&mut app, &key) {
                        continue;
                    }
                    app.handle_key(&key);
                }
            }
        }

        if app.quit_requested {
            return Ok(());
        }
    }
}

fn handle_global_keys(app: &mut app::App, key: &KeyEvent) -> bool {
    // Number keys jump to tabs (unless an input field is capturing).
    if !app.input_capturing() {
        if let KeyCode::Char(ch) = key.code {
            if let Some(digit) = ch.to_digit(10) {
                if (1..=app::TABS.len() as u32).contains(&digit) {
                    app.active_tab = (digit - 1) as usize;
                    app.selected = 0;
                    return true;
                }
            }
        }
        if key.code == KeyCode::Tab {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.active_tab = (app.active_tab + app::TABS.len() - 1) % app::TABS.len();
            } else {
                app.active_tab = (app.active_tab + 1) % app::TABS.len();
            }
            app.selected = 0;
            return true;
        }
        if key.code == KeyCode::Char('?') {
            app.show_help = !app.show_help;
            return true;
        }
        if key.code == KeyCode::Char('q') {
            app.quit_requested = true;
            return true;
        }
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.quit_requested = true;
        return true;
    }
    false
}
