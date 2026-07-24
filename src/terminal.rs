//! A real pseudo-terminal running the user's shell.
//!
//! `portable-pty` gives us a PTY + child process; a background thread pumps the
//! PTY's output into a `vt100::Parser`, which turns the raw byte stream (with
//! all its escape sequences) into a renderable screen grid. The UI reads that
//! grid; keystrokes are written straight back to the PTY.

use anyhow::{anyhow, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

pub struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    rows: u16,
    cols: u16,
}

impl Terminal {
    /// Spawn `$SHELL` (falling back to zsh) in a PTY rooted at `cwd`.
    pub fn new(cwd: &Path, rows: u16, cols: u16) -> Result<Self> {
        let rows = rows.max(1);
        let cols = cols.max(1);

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow!("openpty failed: {e}"))?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let mut cmd = CommandBuilder::new(shell);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| anyhow!("could not start shell: {e}"))?;
        // The parent no longer needs the slave side; dropping it prevents a
        // stray fd from keeping the PTY open after the shell exits.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| anyhow!("clone reader failed: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| anyhow!("take writer failed: {e}"))?;

        // 1000 lines of scrollback kept by the emulator.
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 1000)));

        {
            let parser = parser.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break, // EOF or PTY closed
                        Ok(n) => {
                            if let Ok(mut p) = parser.lock() {
                                p.process(&buf[..n]);
                            }
                        }
                    }
                }
            });
        }

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            rows,
            cols,
        })
    }

    /// Write bytes (already translated from key events) to the shell.
    pub fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Keep the PTY and emulator sized to the visible pane. No-op if unchanged.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
    }

    pub fn parser(&self) -> Arc<Mutex<vt100::Parser>> {
        self.parser.clone()
    }

    /// True once the shell process has exited (e.g. the user typed `exit`).
    pub fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Kill the shell so closing the panel doesn't leak a background process.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
