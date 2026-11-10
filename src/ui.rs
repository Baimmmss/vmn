use crate::app::{App, Focus, Mode, View};
use ratatui::{prelude::*, widgets::*};
use ropey::Rope;
use tui_term::widget::PseudoTerminal;

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
    (":term  /  ctrl-t", "toggle the integrated terminal"),
    (":readme", "open README.md (.md files preview automatically)"),
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
    // When the terminal is open, carve a panel off the bottom for it.
    let (top, term_area) = if app.term_visible {
        let term_h = (area.height / 3).clamp(6, 16);
        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(term_h)])
            .split(area);
        (v[0], Some(v[1]))
    } else {
        (area, None)
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(32), Constraint::Min(20)])
        .split(top);

    // Remembered so mouse clicks (handled outside the render pass) can be
    // translated back into an explorer row / buffer position.
    app.explorer_area = cols[0];
    app.editor_area = cols[1];

    draw_explorer(f, app, cols[0]);
    draw_editor(f, app, cols[1]);

    if let Some(ta) = term_area {
        draw_terminal(f, app, ta);
    }
}

fn draw_terminal(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Terminal;
    let block = pane("terminal", focused);

    // Match the emulator/PTY to the pane's inner dimensions before rendering,
    // so line wrapping and full-screen programs (vim, htop) lay out correctly.
    let inner_rows = area.height.saturating_sub(2).max(1);
    let inner_cols = area.width.saturating_sub(2).max(1);

    if let Some(term) = app.terminal.as_mut() {
        term.resize(inner_rows, inner_cols);
        let binding = term.parser();
        let parser = binding.lock().unwrap();
        let widget = PseudoTerminal::new(parser.screen()).block(block);
        f.render_widget(widget, area);
    } else {
        f.render_widget(block, area);
    }
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

    let preview = app.preview && app.is_markdown();

    let lines: Vec<Line> = if preview {
        app.editor_gutter_w = 0;
        render_markdown_window(&app.rope, app.scroll, height)
    } else {
        let gutter_w = format!("{}", app.line_count()).len().max(3);
        app.editor_gutter_w = gutter_w as u16;
        let mut lines: Vec<Line> = Vec::new();
        for i in 0..height {
            let n = app.scroll + i;
            if n >= app.line_count() {
                break;
            }
            let text = line_text(&app.rope, n);
            let is_cursor_line = n == app.cursor_line && focused;
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:>w$} ", n + 1, w = gutter_w),
                    Style::default().fg(if is_cursor_line { ACCENT } else { DIM }),
                ),
                Span::raw(text),
            ]));
        }
        lines
    };

    let title = match &app.path {
        Some(p) => format!(
            "{}{}{}",
            p.file_name().unwrap_or_default().to_string_lossy(),
            if app.dirty { " ●" } else { "" },
            if preview { " — preview (i / click to edit)" } else { "" },
        ),
        None => "[no file]".into(),
    };

    f.render_widget(Paragraph::new(lines).block(pane(&title, focused)), area);

    if !preview && focused && app.mode != Mode::Command && app.path.is_some() {
        let row = area.y + 1 + (app.cursor_line.saturating_sub(app.scroll)) as u16;
        let col = area.x + 1 + app.editor_gutter_w + 1 + app.cursor_col as u16;
        f.set_cursor_position((col, row));
    }
}

fn line_text(rope: &Rope, n: usize) -> String {
    rope.line(n)
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .collect()
}

/// Render a window of the buffer (one source line per output line, so
/// scrolling and mouse-click row mapping stay 1:1 with the raw editor) as
/// styled Markdown: headings, bold/italic/code spans, lists, blockquotes,
/// rules, and fenced code blocks.
fn render_markdown_window(rope: &Rope, scroll: usize, height: usize) -> Vec<Line<'static>> {
    let total = rope.len_lines();

    // Fenced code blocks toggle a mode that spans lines, so replay from the
    // top to know whether the first visible line starts inside one.
    let mut in_fence = false;
    for n in 0..scroll.min(total) {
        if line_text(rope, n).trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
    }

    let mut out = Vec::new();
    for i in 0..height {
        let n = scroll + i;
        if n >= total {
            break;
        }
        let raw = line_text(rope, n);
        let trimmed = raw.trim_start();

        // Fence markers are hidden (an empty line keeps rows 1:1 with the
        // source); code lines get a shaded background instead.
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            out.push(Line::from(""));
            continue;
        }
        if in_fence {
            out.push(Line::from(Span::styled(
                format!(" {raw} "),
                Style::default()
                    .fg(Color::Rgb(190, 210, 255))
                    .bg(Color::Rgb(40, 40, 48)),
            )));
            continue;
        }
        out.push(style_markdown_line(&raw));
    }
    out
}

fn style_markdown_line(raw: &str) -> Line<'static> {
    // Headings: 1-6 leading '#' followed by a space.
    let hashes = raw.chars().take_while(|c| *c == '#').count().min(6);
    if hashes > 0 && raw.as_bytes().get(hashes) == Some(&b' ') {
        let text = raw[hashes..].trim_start().to_string();
        let style = match hashes {
            1 => Style::default().fg(ACCENT).bold().underlined(),
            2 => Style::default().fg(ACCENT).bold(),
            _ => Style::default().fg(Color::Rgb(190, 210, 255)).bold(),
        };
        return Line::from(Span::styled(text, style));
    }

    let trimmed = raw.trim_start();
    let indent = &raw[..raw.len() - trimmed.len()];

    // Horizontal rule: a line of only -, * or _ (at least 3 of them).
    if trimmed.len() >= 3
        && (trimmed.chars().all(|c| c == '-')
            || trimmed.chars().all(|c| c == '*')
            || trimmed.chars().all(|c| c == '_'))
    {
        return Line::from(Span::styled("─".repeat(200), Style::default().fg(DIM)));
    }

    // Blockquote.
    if let Some(rest) = trimmed.strip_prefix('>') {
        let mut spans = vec![Span::styled(format!("{indent}▏ "), Style::default().fg(DIM))];
        spans.extend(style_inline(rest.trim_start()));
        return Line::from(spans).style(Style::default().fg(Color::Rgb(160, 165, 180)).italic());
    }

    // Bulleted list.
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            let mut spans = vec![Span::styled(format!("{indent}• "), Style::default().fg(ACCENT))];
            spans.extend(style_inline(rest));
            return Line::from(spans);
        }
    }

    // Ordered list ("1. text").
    if let Some(dot) = trimmed.find(". ") {
        let (digits, rest) = trimmed.split_at(dot);
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            let mut spans = vec![Span::styled(
                format!("{indent}{digits}. "),
                Style::default().fg(ACCENT),
            )];
            spans.extend(style_inline(&rest[2..]));
            return Line::from(spans);
        }
    }

    Line::from(style_inline(raw))
}

/// Inline Markdown spans within a single line: `` `code` ``, `**bold**`,
/// and `*italic*` / `_italic_`. Deliberately simple and non-nested.
fn style_inline(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '`' {
            if let Some(end) = find_marker(&chars, i + 1, '`') {
                flush(&mut buf, &mut spans);
                spans.push(Span::styled(
                    chars[i + 1..end].iter().collect::<String>(),
                    Style::default()
                        .fg(Color::Rgb(230, 180, 120))
                        .bg(Color::Rgb(40, 40, 48)),
                ));
                i = end + 1;
                continue;
            }
        }

        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find_double_marker(&chars, i + 2, '*') {
                flush(&mut buf, &mut spans);
                spans.push(Span::styled(
                    chars[i + 2..end].iter().collect::<String>(),
                    Style::default().bold(),
                ));
                i = end + 2;
                continue;
            }
        }

        if chars[i] == '*' || chars[i] == '_' {
            let marker = chars[i];
            if let Some(end) = find_marker(&chars, i + 1, marker) {
                if end > i + 1 {
                    flush(&mut buf, &mut spans);
                    spans.push(Span::styled(
                        chars[i + 1..end].iter().collect::<String>(),
                        Style::default().italic(),
                    ));
                    i = end + 1;
                    continue;
                }
            }
        }

        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut buf, &mut spans);
    spans
}

fn flush(buf: &mut String, spans: &mut Vec<Span<'static>>) {
    if !buf.is_empty() {
        spans.push(Span::raw(std::mem::take(buf)));
    }
}

fn find_marker(chars: &[char], from: usize, marker: char) -> Option<usize> {
    chars[from..].iter().position(|&c| c == marker).map(|p| from + p)
}

fn find_double_marker(chars: &[char], from: usize, marker: char) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&i| chars[i] == marker && chars[i + 1] == marker)
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
            "  :  command   ·   :?  commands   ·   tab  switch pane   ·   ctrl-t  terminal   ·   i  insert",
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
