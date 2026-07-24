use anyhow::Result;
use ropey::Rope;
use std::path::{Path, PathBuf};
use vimini::server::{open_in_browser, LiveServer};
use vimini::terminal::Terminal;

#[derive(PartialEq, Clone, Copy)]
pub enum View {
    Home,
    Workspace,
    Help,
}

/// Local wall-clock as HH:MM:SS, without pulling in a date crate. Uses the
/// TZ offset the OS reports via `localtime`-free arithmetic on the unix epoch.
fn clock() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Best-effort local offset from the TZ the shell exports; falls back to UTC.
    let offset = local_offset_secs();
    let t = (secs as i64 + offset).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", t / 3600, (t % 3600) / 60, t % 60)
}

/// Local UTC offset in seconds, resolved once and cached for the session.
/// macOS reports it cheaply via `date +%z`; we parse ±HHMM.
fn local_offset_secs() -> i64 {
    use std::sync::OnceLock;
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| {
                let s = s.trim();
                if s.len() < 5 {
                    return None;
                }
                let sign = if s.starts_with('-') { -1 } else { 1 };
                let h: i64 = s[1..3].parse().ok()?;
                let m: i64 = s[3..5].parse().ok()?;
                Some(sign * (h * 3600 + m * 60))
            })
            .unwrap_or(0)
    })
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.display().to_string())
}

#[derive(PartialEq, Clone, Copy)]
pub enum Focus {
    Explorer,
    Editor,
    Terminal,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Mode {
    Normal,
    Insert,
    Command,
}

pub struct Entry {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

fn read_dir(path: &Path, depth: usize) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| {
            let p = e.path();
            let is_dir = p.is_dir();
            Entry {
                path: p,
                depth,
                is_dir,
                expanded: false,
            }
        })
        .collect();
    v.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.path.file_name().cmp(&b.path.file_name()))
    });
    v
}

pub struct Explorer {
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub scroll: usize,
}

impl Explorer {
    pub fn new(root: &Path) -> Self {
        Self {
            entries: read_dir(root, 0),
            selected: 0,
            scroll: 0,
        }
    }

    /// Children are read on first expand, so opening a large repo never walks
    /// the whole tree up front.
    pub fn toggle(&mut self, i: usize) {
        if i >= self.entries.len() || !self.entries[i].is_dir {
            return;
        }
        if self.entries[i].expanded {
            let d = self.entries[i].depth;
            let mut j = i + 1;
            while j < self.entries.len() && self.entries[j].depth > d {
                j += 1;
            }
            self.entries.drain(i + 1..j);
            self.entries[i].expanded = false;
        } else {
            let kids = read_dir(&self.entries[i].path, self.entries[i].depth + 1);
            self.entries[i].expanded = true;
            for (k, e) in kids.into_iter().enumerate() {
                self.entries.insert(i + 1 + k, e);
            }
        }
    }
}

pub struct App {
    pub view: View,
    pub focus: Focus,
    pub mode: Mode,
    pub cwd: PathBuf,
    pub explorer: Explorer,

    pub rope: Rope,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll: usize,

    pub cmd: String,
    pub status: String,
    pub should_quit: bool,
    pub server: Option<LiveServer>,
    pub terminal: Option<Terminal>,
    pub term_visible: bool,
}

impl App {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            view: View::Home,
            focus: Focus::Explorer,
            mode: Mode::Normal,
            explorer: Explorer::new(&cwd),
            cwd,
            rope: Rope::new(),
            path: None,
            dirty: false,
            cursor_line: 0,
            cursor_col: 0,
            scroll: 0,
            cmd: String::new(),
            status: String::new(),
            should_quit: false,
            server: None,
            terminal: None,
            term_visible: false,
        }
    }

    /// Show/hide the integrated terminal. Showing it focuses the shell (and
    /// spawns it on first use); hiding returns focus to the editor. The shell
    /// process is killed on hide, so re-opening starts a fresh session.
    pub fn toggle_terminal(&mut self) {
        if self.term_visible {
            self.term_visible = false;
            self.terminal = None;
            if self.focus == Focus::Terminal {
                self.focus = Focus::Editor;
            }
            return;
        }

        // Real size is corrected on the first render; these are just seeds.
        match Terminal::new(&self.cwd, 20, 80) {
            Ok(t) => {
                self.terminal = Some(t);
                self.term_visible = true;
                self.focus = Focus::Terminal;
                self.view = View::Workspace;
                self.status = "terminal — ctrl-t to close".into();
            }
            Err(e) => self.status = format!("terminal: {e}"),
        }
    }

    /// Called each frame: if the shell exited on its own (`exit`, Ctrl-D),
    /// tear the panel down.
    pub fn reap_terminal(&mut self) {
        if !self.term_visible {
            return;
        }
        let exited = self
            .terminal
            .as_mut()
            .map(|t| t.has_exited())
            .unwrap_or(true);
        if exited {
            self.terminal = None;
            self.term_visible = false;
            if self.focus == Focus::Terminal {
                self.focus = Focus::Editor;
            }
            self.status = "terminal closed".into();
        }
    }

    /// Rebuild the explorer from the current cwd, keeping the selection index
    /// in range. Expansion state is intentionally reset — cheap and predictable.
    pub fn refresh_explorer(&mut self) {
        let sel = self.explorer.selected;
        self.explorer = Explorer::new(&self.cwd);
        self.explorer.selected = sel.min(self.explorer.entries.len().saturating_sub(1));
    }

    /// Create a new file and open it. Any extension works — the buffer is
    /// UTF-8 text, so the type only matters to the (future) highlighter.
    /// Parent directories are created as needed.
    pub fn new_file(&mut self, arg: &str) {
        if arg.is_empty() {
            self.status = "usage: :new <name.ext>".into();
            return;
        }

        let path = if Path::new(arg).is_absolute() {
            PathBuf::from(arg)
        } else {
            self.cwd.join(arg)
        };

        if path.exists() {
            self.status = format!("error: already exists: {}", path.display());
            return;
        }

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.status = format!("error: {e}");
                return;
            }
        }

        if let Err(e) = std::fs::File::create(&path) {
            self.status = format!("error: {e}");
            return;
        }

        match self.open(&path) {
            Ok(_) => {
                self.refresh_explorer();
                self.mode = Mode::Insert;
                self.status = format!("new: {}", path.display());
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Create a directory (and any missing parents).
    pub fn new_dir(&mut self, arg: &str) {
        if arg.is_empty() {
            self.status = "usage: :mkdir <name>".into();
            return;
        }
        let path = if Path::new(arg).is_absolute() {
            PathBuf::from(arg)
        } else {
            self.cwd.join(arg)
        };
        match std::fs::create_dir_all(&path) {
            Ok(_) => {
                self.refresh_explorer();
                self.status = format!("mkdir: {}", path.display());
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Hand the running server's URL to the default browser.
    pub fn open_browser(&mut self) {
        match &self.server {
            Some(s) => {
                let url = format!("http://127.0.0.1:{}", s.port);
                match open_in_browser(&url) {
                    Ok(_) => self.status = format!("opened {url}"),
                    Err(e) => self.status = format!("{e} — open {url} manually"),
                }
            }
            None => self.status = "live server is not running".into(),
        }
    }

    fn serve(&mut self, arg: &str) {
        let tokens: Vec<&str> = arg.split_whitespace().collect();

        if tokens.first() == Some(&"stop") {
            match self.server.take() {
                Some(s) => {
                    s.stop();
                    self.status = "live server stopped".into();
                }
                None => self.status = "live server is not running".into(),
            }
            return;
        }

        let no_open = tokens.iter().any(|t| *t == "--no-open");

        // Re-running :serve while it's up just refocuses the browser.
        if let Some(s) = &self.server {
            let port = s.port;
            if no_open {
                self.status = format!("already serving on http://127.0.0.1:{port}");
            } else {
                self.open_browser();
            }
            return;
        }

        let port: u16 = match tokens.iter().find(|t| !t.starts_with("--")) {
            None => 3000,
            Some(t) => match t.parse() {
                Ok(p) => p,
                Err(_) => {
                    self.status = format!("error: bad port: {t}");
                    return;
                }
            },
        };

        // The listener is bound synchronously inside start(), so by the time
        // this returns the browser cannot beat the server to the port.
        match LiveServer::start(self.cwd.clone(), port) {
            Ok(s) => {
                self.server = Some(s);
                let url = format!("http://127.0.0.1:{port}");
                if no_open {
                    self.status = format!("serving {} on {url}", self.cwd.display());
                } else {
                    match open_in_browser(&url) {
                        Ok(_) => self.status = format!("serving {} → {url}", self.cwd.display()),
                        Err(e) => self.status = format!("serving on {url} ({e})"),
                    }
                }
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    pub fn open(&mut self, path: &Path) -> Result<()> {
        self.rope = if path.exists() {
            Rope::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))?
        } else {
            Rope::new()
        };
        self.path = Some(path.to_path_buf());
        self.cursor_line = 0;
        self.cursor_col = 0;
        self.scroll = 0;
        self.dirty = false;
        self.view = View::Workspace;
        self.focus = Focus::Editor;
        Ok(())
    }

    pub fn save(&mut self) {
        match self.path.clone() {
            Some(p) => {
                let res = std::fs::File::create(&p)
                    .map_err(anyhow::Error::from)
                    .and_then(|f| Ok(self.rope.write_to(std::io::BufWriter::new(f))?));
                match res {
                    Ok(_) => {
                        self.dirty = false;
                        self.status = format!("written: {}", p.display());
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
            }
            None => self.status = "error: no file name".into(),
        }
    }

    /// Called on a timer. Writes only when there are unsaved changes and a
    /// file is open, so it never spams the status line or touches disk idly.
    pub fn autosave(&mut self) {
        if !self.dirty || self.path.is_none() {
            return;
        }
        let p = self.path.clone().unwrap();
        let res = std::fs::File::create(&p)
            .map_err(anyhow::Error::from)
            .and_then(|f| Ok(self.rope.write_to(std::io::BufWriter::new(f))?));
        match res {
            Ok(_) => {
                self.dirty = false;
                self.status = format!("✓ autosaved {} at {}", name_of(&p), clock());
            }
            Err(e) => self.status = format!("autosave failed: {e}"),
        }
    }

    /// Change the explorer root. Handles `~`, absolute, and relative paths.
    pub fn cd(&mut self, arg: &str) {
        let target = if let Some(rest) = arg.strip_prefix('~') {
            match std::env::var("HOME") {
                Ok(h) => PathBuf::from(h).join(rest.trim_start_matches('/')),
                Err(_) => {
                    self.status = "error: $HOME not set".into();
                    return;
                }
            }
        } else {
            let p = PathBuf::from(arg);
            if p.is_absolute() {
                p
            } else {
                self.cwd.join(p)
            }
        };

        match target.canonicalize() {
            Ok(p) if p.is_dir() => {
                self.cwd = p;
                self.explorer = Explorer::new(&self.cwd);
                self.view = View::Workspace;
                self.status = format!("cwd: {}", self.cwd.display());
            }
            _ => self.status = format!("error: not a directory: {}", target.display()),
        }
    }

    pub fn exec_command(&mut self) {
        let raw = self.cmd.trim().to_string();
        self.cmd.clear();
        self.mode = Mode::Normal;

        let mut parts = raw.splitn(2, ' ');
        let verb = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();

        match verb {
            "q" | "quit" => self.should_quit = true,
            "w" | "write" => self.save(),
            "wq" => {
                self.save();
                self.should_quit = true;
            }
            "pwd" => self.status = self.cwd.display().to_string(),
            "cd" => {
                if arg.is_empty() {
                    self.status = "usage: :cd <path>".into();
                } else {
                    self.cd(arg);
                }
            }
            "e" | "edit" => {
                let p = self.cwd.join(arg);
                if let Err(e) = self.open(&p) {
                    self.status = format!("error: {e}");
                }
            }
            "new" | "n" | "touch" => self.new_file(arg),
            "mkdir" => self.new_dir(arg),
            "serve" => self.serve(arg),
            "open" | "o" => self.open_browser(),
            "r" | "refresh" => {
                self.refresh_explorer();
                self.status = "explorer refreshed".into();
            }
            "term" | "terminal" | "t" => self.toggle_terminal(),
            "?" | "help" | "h" => self.view = View::Help,
            "back" | "b" => {
                self.view = View::Home;
                self.status.clear();
            }
            "" => {}
            other => self.status = format!("unknown command: {other}"),
        }
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines().max(1)
    }

    pub fn line_len(&self, line: usize) -> usize {
        if line >= self.rope.len_lines() {
            return 0;
        }
        let s = self.rope.line(line);
        let mut n = s.len_chars();
        if n > 0 && s.char(n - 1) == '\n' {
            n -= 1;
            if n > 0 && s.char(n - 1) == '\r' {
                n -= 1;
            }
        }
        n
    }

    fn clamp(&mut self) {
        self.cursor_line = self.cursor_line.min(self.line_count().saturating_sub(1));
        self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_line));
    }

    fn char_idx(&self) -> usize {
        self.rope.line_to_char(self.cursor_line) + self.cursor_col
    }

    pub fn adjust_scroll(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.cursor_line < self.scroll {
            self.scroll = self.cursor_line;
        } else if self.cursor_line >= self.scroll + height {
            self.scroll = self.cursor_line - height + 1;
        }
    }

    pub fn move_cursor(&mut self, dl: isize, dc: isize) {
        if dl < 0 {
            self.cursor_line = self.cursor_line.saturating_sub((-dl) as usize);
        } else {
            self.cursor_line += dl as usize;
        }
        if dc < 0 {
            self.cursor_col = self.cursor_col.saturating_sub((-dc) as usize);
        } else {
            self.cursor_col += dc as usize;
        }
        self.clamp();
    }

    pub fn insert_char(&mut self, c: char) {
        let idx = self.char_idx();
        self.rope.insert_char(idx, c);
        self.dirty = true;
        if c == '\n' {
            self.cursor_line += 1;
            self.cursor_col = 0;
        } else {
            self.cursor_col += 1;
        }
    }

    pub fn backspace(&mut self) {
        let idx = self.char_idx();
        if idx == 0 {
            return;
        }
        self.rope.remove(idx - 1..idx);
        self.dirty = true;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else {
            self.cursor_line -= 1;
            self.cursor_col = self.line_len(self.cursor_line);
        }
    }
}
