mod app;
mod ui;

use std::io::{stdout, Stdout};
use std::panic;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use app::{App, Focus, Mode, View};

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen)?;
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(out))?,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if app.mode == Mode::Command {
        match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.cmd.clear();
            }
            KeyCode::Enter => app.exec_command(),
            KeyCode::Backspace => {
                app.cmd.pop();
            }
            KeyCode::Char(c) => app.cmd.push(c),
            _ => {}
        }
        return;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    if key.code == KeyCode::Char(':') && app.mode == Mode::Normal {
        app.mode = Mode::Command;
        app.status.clear();
        return;
    }

    // Help is a dismiss-on-any-key overlay (`:` already handled above so the
    // user can still type another command straight from it).
    if app.view == View::Help {
        app.view = View::Workspace;
        return;
    }

    if app.view == View::Home {
        if key.code == KeyCode::Enter {
            app.view = View::Workspace;
        }
        return;
    }

    if key.code == KeyCode::Tab {
        app.focus = if app.focus == Focus::Explorer {
            Focus::Editor
        } else {
            Focus::Explorer
        };
        return;
    }

    match app.focus {
        Focus::Explorer => handle_explorer_key(app, key),
        Focus::Editor => handle_editor_key(app, key),
    }
}

fn handle_explorer_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            if app.explorer.selected + 1 < app.explorer.entries.len() {
                app.explorer.selected += 1;
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.explorer.selected = app.explorer.selected.saturating_sub(1);
        }
        KeyCode::Enter | KeyCode::Char('l') => {
            let i = app.explorer.selected;
            if let Some(e) = app.explorer.entries.get(i) {
                if e.is_dir {
                    app.explorer.toggle(i);
                } else {
                    let p = e.path.clone();
                    if let Err(err) = app.open(&p) {
                        app.status = format!("error: {err}");
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_editor_key(app: &mut App, key: KeyEvent) {
    if app.mode == Mode::Insert {
        match key.code {
            KeyCode::Esc => app.mode = Mode::Normal,
            KeyCode::Char(c) => app.insert_char(c),
            KeyCode::Enter => app.insert_char('\n'),
            KeyCode::Backspace => app.backspace(),
            KeyCode::Left => app.move_cursor(0, -1),
            KeyCode::Right => app.move_cursor(0, 1),
            KeyCode::Up => app.move_cursor(-1, 0),
            KeyCode::Down => app.move_cursor(1, 0),
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('i') => app.mode = Mode::Insert,
        KeyCode::Char('h') | KeyCode::Left => app.move_cursor(0, -1),
        KeyCode::Char('l') | KeyCode::Right => app.move_cursor(0, 1),
        KeyCode::Char('k') | KeyCode::Up => app.move_cursor(-1, 0),
        KeyCode::Char('j') | KeyCode::Down => app.move_cursor(1, 0),
        _ => {}
    }
}

fn run(app: &mut App) -> Result<()> {
    let mut guard = TerminalGuard::new()?;
    let mut last_autosave = Instant::now();
    const AUTOSAVE_EVERY: Duration = Duration::from_secs(3);

    while !app.should_quit {
        guard.terminal.draw(|f| ui::draw(f, app))?;

        // Poll instead of blocking so the autosave timer keeps ticking even
        // when the user isn't typing. 250ms keeps idle CPU near zero.
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key);
                }
            }
        }

        if last_autosave.elapsed() >= AUTOSAVE_EVERY {
            app.autosave();
            last_autosave = Instant::now();
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    let arg = std::env::args().nth(1).map(PathBuf::from);
    let cwd = std::env::current_dir()?;

    let mut app = match &arg {
        Some(p) if p.is_dir() => App::new(p.canonicalize()?),
        _ => App::new(cwd),
    };

    if let Some(p) = arg.filter(|p| p.is_file()) {
        app.open(&p)?;
    }

    run(&mut app)
}
