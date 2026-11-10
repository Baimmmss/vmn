# Developing vmn

This guide is for people who want to **build, change, or package** vmn.
If you only want to use it, you don't need any of this — see
[Install](README.md#install) in the README.

---

## Requirements

### Build toolchain

| Tool      | Version           | Notes                                        |
| --------- | ----------------- | -------------------------------------------- |
| **Rust**  | 1.96.0 (or newer) | Edition 2021. Install via [rustup](https://rustup.rs). |
| **Cargo** | ships with Rust   | Used to build and install.                    |

#### How to Install the Toolchain

If you do not have Rust and Cargo installed, you can set them up using the following commands:

1. **Install rustup** (the official Rust toolchain installer):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
   Follow the on-screen prompts. Once completed, restart your terminal or run:
   ```bash
   source "$HOME/.cargo/env"
   ```

2. **Alternative installation (Homebrew for macOS)**:
   ```bash
   brew install rustup
   rustup-init
   ```

3. **Verify the installation**:
   ```bash
   rustc --version    # should be >= 1.96.0
   cargo --version
   ```

### Rust dependencies

These are fetched automatically by Cargo on first build — you don't install them
by hand. Listed here for reference (versions from `Cargo.lock`):

| Crate         | Version | Purpose                                  |
| ------------- | ------- | ---------------------------------------- |
| `crossterm`   | 0.28.1  | Terminal raw mode, input events, screen  |
| `ratatui`     | 0.29.0  | TUI layout and rendering                 |
| `ropey`       | 1.6.1   | Rope text buffer (fast edits, big files) |
| `anyhow`      | 1.0.103 | Error handling                           |
| `tiny_http`   | 0.12.0  | Live-server HTTP                         |
| `notify`      | 6.1.1   | Filesystem watching for live reload      |
| `portable-pty`| 0.8.1   | Pseudo-terminal for the integrated shell |
| `vt100`       | 0.15.2  | ANSI/VT terminal emulation               |
| `tui-term`    | 0.2.0   | Renders the emulated screen in the TUI   |

### Network

- Cargo needs internet access **once** to download crates on the first build.
  After that, building and running are fully offline.

---

## Build and install from source

Requires the [Rust toolchain](https://rustup.rs) (`cargo`) — see [Requirements](#requirements) above.

```bash
cd vimini
cargo install --path .
```

This installs the `vmn` binary to `~/.cargo/bin`. Make sure that's on your `PATH`
(add this to `~/.zshrc` if needed):

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Now `vmn` works from anywhere.

### Run from source without installing

```bash
cargo run --release           # opens the current directory
cargo run --release -- file   # opens a file
```

---

## Building the prebuilt download

Produces the universal `dist/vmn` and the `dist/vmn-macos.zip` that users install
from (see [Install](README.md#install)):

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
lipo -create -output dist/vmn \
  target/aarch64-apple-darwin/release/vmn \
  target/x86_64-apple-darwin/release/vmn
cd dist && mkdir -p vmn-macos && cp vmn install.sh vmn-macos/ \
  && zip -r vmn-macos.zip vmn-macos && rm -r vmn-macos
```

Give users `dist/vmn-macos.zip` (e.g. attach it to a GitHub release). They unzip
it and run `./install.sh` once — no Rust needed on their side.

---

## Project layout

```
vimini/
├── Cargo.toml            # crate + [[bin]] name = "vmn"
├── README.md             # for users: install, tutorial, commands
├── DEVELOPMENT.md        # for developers: this file
├── dist/
│   ├── install.sh        # one-time installer for the prebuilt binary
│   ├── vmn               # universal (arm64 + x86_64) binary, built locally
│   └── vmn-macos.zip     # vmn + install.sh, the file you hand to users
├── src/
│   ├── main.rs           # entry point, event loop, key handling
│   ├── lib.rs            # library surface (exposes the server for tests)
│   ├── app.rs            # editor state: buffer, explorer, cwd, modes, commands
│   ├── ui.rs             # rendering: home, workspace, Markdown preview, terminal, help, status bar
│   ├── server.rs         # live server + file watcher + browser launch
│   └── terminal.rs       # PTY + shell process + vt100 emulation
└── tests/
    ├── live_server.rs    # integration tests for the live server
    └── terminal.rs       # integration tests for the PTY terminal
```

Built with [ratatui](https://ratatui.rs) (TUI), [crossterm](https://github.com/crossterm-rs/crossterm)
(terminal), [ropey](https://github.com/cessen/ropey) (text buffer),
[tiny_http](https://github.com/tiny-http/tiny-http) and
[notify](https://github.com/notify-rs/notify) (live server).

---

## Running the tests

```bash
cargo test --release
```

Covers two subsystems end-to-end:

- **Live server** — HTML serving, reload-script injection, the file-change reload
  signal, path-traversal rejection, and 404s.
- **Terminal** — spawning a real shell, running a command and seeing its output
  reach the emulated screen, `PATH` inheritance, and exit detection.
