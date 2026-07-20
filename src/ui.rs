use crate::app::{App, Focus, Mode, View};
use ratatui::{prelude::*, widgets::*};

pub const ACCENT: Color = Color::Rgb(120, 170, 255);
pub const DIM: Color = Color::Rgb(90, 95, 110);

fn pane(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { ACCENT } else { DIM }))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(if focused { ACCENT } else { DIM }),
        ))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    match app.view {
        View::Home => draw_home(f, app, root[0]),
        View::Workspace => draw_workspace(f, app, root[0]),
        View::Help => draw_help(f, app, root[0]),
    }
    draw_command_box(f, app, root[1]);
    draw_status(f, app, root[2]);
}

/// The single source of truth for every command. `:?` and the help view both
/// render from this, so they can never drift out of sync.
const COMMANDS: &[(&str, &str)] = &[
    (":new <file>", "create a file and start editing"),
    (":e <file>", "open an existing file"),
    (":serve", "live server + open in browser"),
    (":serve stop", "stop the live server"),
    (":cd <path>", "change working directory"),
    (":w", "save now"),
    (":?", "show this command list"),
    (":back", "return to the home screen"),
    (":q", "quit"),
];

fn draw_home(f: &mut Frame, app: &App, area: Rect) {
    let wordmark = [
        r"  ██╗   ██╗███╗   ███╗███╗   ██╗ ",
        r"  ██║   ██║████╗ ████║████╗  ██║ ",
        r"  ██║   ██║██╔████╔██║██╔██╗ ██║ ",
        r"  ╚██╗ ██╔╝██║╚██╔╝██║██║╚██╗██║ ",
        r"   ╚████╔╝ ██║ ╚═╝ ██║██║ ╚████║ ",
        r"    ╚═══╝  ╚═╝     ╚═╝╚═╝  ╚═══╝ ",
    ];

    // Vertically center the block within the pane.
    let inner_h = area.height.saturating_sub(2) as usize;
    let content_h = wordmark.len() + 7;
    let top_pad = inner_h.saturating_sub(content_h) / 2;

    let mut lines: Vec<Line> = Vec::new();
    for _ in 0..top_pad {
        lines.push(Line::from(""));
    }
    for l in wordmark {
        lines.push(Line::from(Span::styled(l, Style::default().fg(ACCENT))).centered());
    }
    lines.push(Line::from(""));
    lines.push(
        Line::from(Span::styled(
            "keyboard-first · no plugins · no AI",
            Style::default().fg(DIM),
        ))
        .centered(),
    );
    lines.push(Line::from(""));
    lines.push(
        Line::from(vec![
            Span::styled("in  ", Style::default().fg(DIM)),
            Span::styled(app.cwd.display().to_string(), Style::default().fg(Color::Gray)),
        ])
        .centered(),
    );
    lines.push(Line::from(""));
    lines.push(
        Line::from(vec![
            Span::styled("enter", Style::default().fg(ACCENT).bold()),
            Span::styled(" open workspace     ", Style::default().fg(DIM)),
            Span::styled(":?", Style::default().fg(ACCENT).bold()),
            Span::styled(" all commands", Style::default().fg(DIM)),
        ])
        .centered(),
    );

    f.render_widget(Paragraph::new(lines).block(pane("vmn", true)), area);
}

fn draw_help(f: &mut Frame, _app: &App, area: Rect) {
    let mut lines: Vec<Line> = vec![Line::from("")];
    for (k, d) in COMMANDS {
        lines.push(Line::from(vec![
            Span::styled(format!("  {k:<16}"), Style::default().fg(ACCENT)),
            Span::styled(*d, Style::default().fg(Color::Gray)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  press any key to return  ·  :back for home",
        Style::default().fg(DIM),
    )));

    f.render_widget(Paragraph::new(lines).block(pane("commands", true)), area);
}

fn draw_workspace(f: &mut Frame, app: &mut App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(32), Constraint::Min(20)])
        .split(area);

    draw_explorer(f, app, cols[0]);
    draw_editor(f, app, cols[1]);
}

fn draw_explorer(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Explorer;
    let height = area.height.saturating_sub(2) as usize;

    if app.explorer.selected < app.explorer.scroll {
        app.explorer.scroll = app.explorer.selected;
    } else if height > 0 && app.explorer.selected >= app.explorer.scroll + height {
        app.explorer.scroll = app.explorer.selected - height + 1;
    }

    let items: Vec<Line> = app
        .explorer
        .entries
        .iter()
        .enumerate()
        .skip(app.explorer.scroll)
        .take(height)
        .map(|(i, e)| {
            let name = e.path.file_name().unwrap_or_default().to_string_lossy();
            let icon = if e.is_dir {
                if e.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };
            let indent = "  ".repeat(e.depth);
            let selected = i == app.explorer.selected;
            let style = match (selected, e.is_dir) {
                (true, _) => Style::default().fg(Color::Black).bg(ACCENT),
                (false, true) => Style::default().fg(ACCENT),
                (false, false) => Style::default().fg(Color::Gray),
            };
            Line::from(Span::styled(format!("{indent}{icon}{name}"), style))
        })
        .collect();

    let title = app
        .cwd
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| app.cwd.display().to_string());

    f.render_widget(Paragraph::new(items).block(pane(&title, focused)), area);
}

fn draw_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Editor;
    let height = area.height.saturating_sub(2) as usize;
    app.adjust_scroll(height);

    let gutter_w = format!("{}", app.line_count()).len().max(3);
    let mut lines: Vec<Line> = Vec::new();

    for i in 0..height {
        let n = app.scroll + i;
        if n >= app.line_count() {
            break;
        }
        let text: String = app
            .rope
            .line(n)
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .collect();
        let is_cursor_line = n == app.cursor_line && focused;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:>w$} ", n + 1, w = gutter_w),
                Style::default().fg(if is_cursor_line { ACCENT } else { DIM }),
            ),
            Span::raw(text),
        ]));
    }

    let title = match &app.path {
        Some(p) => format!(
            "{}{}",
            p.file_name().unwrap_or_default().to_string_lossy(),
            if app.dirty { " ●" } else { "" }
        ),
        None => "[no file]".into(),
    };

    f.render_widget(Paragraph::new(lines).block(pane(&title, focused)), area);

    if focused && app.mode != Mode::Command && app.path.is_some() {
        let row = area.y + 1 + (app.cursor_line.saturating_sub(app.scroll)) as u16;
        let col = area.x + 1 + gutter_w as u16 + 1 + app.cursor_col as u16;
        f.set_cursor_position((col, row));
    }
}

fn draw_command_box(f: &mut Frame, app: &App, area: Rect) {
    let active = app.mode == Mode::Command;
    let content = if active {
        Line::from(vec![
            Span::styled(":", Style::default().fg(ACCENT)),
            Span::raw(&app.cmd),
        ])
    } else {
        Line::from(Span::styled(
            "  :  command   ·   :?  all commands   ·   tab  switch pane   ·   i  insert",
            Style::default().fg(DIM),
        ))
    };

    f.render_widget(
        Paragraph::new(content).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(if active { ACCENT } else { DIM })),
        ),
        area,
    );

    if active {
        f.set_cursor_position((area.x + 2 + app.cmd.chars().count() as u16, area.y + 1));
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let (label, color) = match app.mode {
        Mode::Normal => (" NORMAL ", ACCENT),
        Mode::Insert => (" INSERT ", Color::Rgb(140, 220, 160)),
        Mode::Command => (" COMMAND ", Color::Rgb(230, 200, 130)),
    };

    let right = if app.status.is_empty() {
        format!("{}:{} ", app.cursor_line + 1, app.cursor_col + 1)
    } else {
        format!("{} ", app.status)
    };

    let serve = match &app.server {
        Some(s) => format!(" ● :{} ", s.port),
        None => String::new(),
    };

    let cwd = app.cwd.display().to_string();
    let pad = (area.width as usize)
        .saturating_sub(label.len() + cwd.len() + serve.len() + right.len() + 2);

    let line = Line::from(vec![
        Span::styled(label, Style::default().bg(color).fg(Color::Black).bold()),
        Span::styled(format!(" {cwd} "), Style::default().fg(DIM)),
        Span::styled(serve, Style::default().fg(Color::Rgb(140, 220, 160))),
        Span::raw(" ".repeat(pad)),
        Span::styled(right, Style::default().fg(DIM)),
    ]);

    f.render_widget(Paragraph::new(line), area);
}
