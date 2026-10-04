mod app;
mod theme;
mod ui;
mod views;

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{event, execute};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::core as c;

pub fn run() -> Result<(), String> {
    // A panic must restore the terminal (raw mode + alternate screen) even
    // though the release profile aborts without unwinding.
    std::panic::set_hook(Box::new(|info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        eprintln!("{info}");
    }));
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let result = event_loop(&mut terminal);

    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .ok();
    terminal.show_cursor().ok();
    result
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<(), String> {
    let mut app = app::App::new(c::types::VERSION.to_string());

    loop {
        // Refresh polling state as needed per view interval.
        app.refresh_if_needed();

        terminal
            .draw(|frame| app.render(frame))
            .map_err(|e| e.to_string())?;

        if event::poll(std::time::Duration::from_millis(200)).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(key) => {
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
                Event::Mouse(mouse) => app.handle_mouse(&mouse),
                _ => {}
            }
        }

        if app.quit_requested {
            return Ok(());
        }
    }
}

fn handle_global_keys(app: &mut app::App, key: &KeyEvent) -> bool {
    // A confirm/picker modal owns ALL routing while armed — global keys must
    // not hide it ('?') or act underneath it (digits/Tab switch views, 'y'
    // would execute an invisible destructive action on the next draw).
    if app.confirm.is_some() || app.picker.is_some() {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            app.quit_requested = true;
            return true;
        }
        return false;
    }
    // Number keys jump to tabs (unless an input field is capturing).
    if !app.input_capturing() {
        if let KeyCode::Char(ch) = key.code {
            if let Some(digit) = ch.to_digit(10) {
                if (1..=app::TABS.len() as u32).contains(&digit) {
                    app.switch_tab((digit - 1) as usize);
                    return true;
                }
            }
        }
        // Tab cycles tabs, except on views that own Tab as a sub-panel
        // toggle (Config: global/project, Logs: daemon/project-runs).
        // BackTab is how crossterm reports Shift+Tab on standard terminals
        // (the kitty protocol is not enabled), so treat both.
        let tab_owned_by_view = app.active_tab == 4 || app.active_tab == 8;
        if key.code == KeyCode::Tab && !tab_owned_by_view {
            app.switch_tab((app.active_tab + 1) % app::TABS.len());
            return true;
        }
        if (key.code == KeyCode::BackTab)
            || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT))
        {
            app.switch_tab((app.active_tab + app::TABS.len() - 1) % app::TABS.len());
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
