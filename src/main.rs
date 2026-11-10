mod app;
mod ui;

use std::io::{stdout, Stdout};
use std::panic;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
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
        execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(out))?,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

/// Translate a crossterm key event into the byte sequence a PTY expects.
/// Returns None for keys with no terminal representation.
fn key_to_pty_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let bytes = match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                let lower = c.to_ascii_lowercase();
                match lower {
                    // Ctrl-A..Ctrl-Z -> control codes 0x01..0x1a
                    'a'..='z' => vec![(lower as u8 - b'a') + 1],
                    ' ' => vec![0], // Ctrl-Space -> NUL
                    _ => {
                        let mut b = [0u8; 4];
                        c.encode_utf8(&mut b).as_bytes().to_vec()
                    }
                }
            } else {
                let mut b = [0u8; 4];
                c.encode_utf8(&mut b).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => vec![0x1b, b'[', b'Z'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],
        _ => return None,
    };
    Some(bytes)
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

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Ctrl-t toggles the terminal from anywhere and is never sent to the shell.
    if ctrl && key.code == KeyCode::Char('t') {
        app.toggle_terminal();
        return;
    }

    // While the terminal is focused, every other key goes straight to the shell
    // — including Ctrl-C (SIGINT to the foreground program), Tab (completion),
    // and `:` — so none of the editor bindings below apply.
    if app.focus == Focus::Terminal {
        if let Some(bytes) = key_to_pty_bytes(key) {
            if let Some(term) = app.terminal.as_mut() {
                term.send(&bytes);
            }
        }
        return;
    }

    if ctrl && key.code == KeyCode::Char('c') {
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
        // Terminal focus is fully handled above and returns before reaching
        // here; treat anything else as editor input.
        Focus::Editor | Focus::Terminal => handle_editor_key(app, key),
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
        KeyCode::Enter | KeyCode::Char('l') => app.activate_explorer_selected(),
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
        KeyCode::Char('i') => app.start_insert(),
        KeyCode::Char('h') | KeyCode::Left => app.move_cursor(0, -1),
        KeyCode::Char('l') | KeyCode::Right => app.move_cursor(0, 1),
        KeyCode::Char('k') | KeyCode::Up => app.move_cursor(-1, 0),
        KeyCode::Char('j') | KeyCode::Down => app.move_cursor(1, 0),
        _ => {}
    }
}

/// Left click: select/open/toggle in the explorer, or place the cursor in
/// the editor (exiting Markdown preview if it was active). The explorer is
/// tried first since its pane sits to the left; a click outside both panes'
/// last-known areas (e.g. the terminal panel) is ignored.
fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    if app.view != View::Workspace {
        return;
    }
    let delta = match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if !app.click_explorer(mouse.row, mouse.column) {
                app.click_editor(mouse.row, mouse.column);
            }
            return;
        }
        MouseEventKind::ScrollDown => SCROLL_LINES,
        MouseEventKind::ScrollUp => -SCROLL_LINES,
        _ => return,
    };
    // The wheel scrolls whichever pane is under the pointer, without
    // changing focus.
    if contains(app.explorer_area, mouse.column, mouse.row) {
        app.scroll_explorer(delta);
    } else if contains(app.editor_area, mouse.column, mouse.row) {
        app.scroll_editor(delta);
    }
}

const SCROLL_LINES: isize = 3;

fn contains(area: Rect, col: u16, row: u16) -> bool {
    col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height
}

fn run(app: &mut App) -> Result<()> {
    let mut guard = TerminalGuard::new()?;
    let mut last_autosave = Instant::now();
    const AUTOSAVE_EVERY: Duration = Duration::from_secs(3);

    while !app.should_quit {
        guard.terminal.draw(|f| ui::draw(f, app))?;

        // Poll instead of blocking so the autosave timer keeps ticking when the
        // user isn't typing. With the terminal open we poll faster so shell
        // output paints promptly; otherwise 250ms keeps idle CPU near zero.
        let timeout = if app.term_visible {
            Duration::from_millis(30)
        } else {
            Duration::from_millis(250)
        };
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        handle_key(app, key);
                    }
                }
                Event::Mouse(mouse) => handle_mouse(app, mouse),
                _ => {}
            }
            app.sync_preview();
        }

        // Close the panel if the shell exited on its own (exit / Ctrl-D).
        app.reap_terminal();

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
