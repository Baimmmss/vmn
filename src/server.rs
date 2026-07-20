use anyhow::{anyhow, Result};
use notify::{RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server};

/// Polled by the browser instead of a WebSocket. A 400ms fetch loop costs
/// nothing locally and saves us an async runtime plus a WS implementation.
const RELOAD_JS: &str = r#"
<script>
(function () {
  let seen = null;
  setInterval(async function () {
    try {
      const r = await fetch('/__vmn_reload', { cache: 'no-store' });
      const v = await r.text();
      if (seen === null) seen = v;
      else if (seen !== v) location.reload();
    } catch (e) {}
  }, 400);
})();
</script>
"#;

fn mime_for(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn hdr(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("static header is valid")
}

/// Hand a URL to macOS's launch services via `open(1)`, which reuses the
/// running default browser if there is one and launches it otherwise.
/// Fire-and-forget: we reap the child on a thread so it can't zombie, and
/// never block the render loop waiting on it.
pub fn open_in_browser(url: &str) -> Result<()> {
    let child = std::process::Command::new("open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("could not launch browser: {e}"))?;

    thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(())
}

pub struct LiveServer {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl LiveServer {
    pub fn start(root: PathBuf, port: u16) -> Result<Self> {
        let server = Server::http(("127.0.0.1", port))
            .map_err(|e| anyhow!("could not bind 127.0.0.1:{port}: {e}"))?;

        let stop = Arc::new(AtomicBool::new(false));
        let version = Arc::new(AtomicU64::new(0));

        // Watcher thread: bumps `version` whenever anything under root changes.
        // The watcher must stay alive for events to fire, hence the park loop.
        {
            let version = version.clone();
            let stop = stop.clone();
            let root = root.clone();
            thread::spawn(move || {
                let v = version.clone();
                let mut watcher = match notify::recommended_watcher(
                    move |res: notify::Result<notify::Event>| {
                        if let Ok(ev) = res {
                            if ev.kind.is_modify() || ev.kind.is_create() || ev.kind.is_remove() {
                                v.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    },
                ) {
                    Ok(w) => w,
                    Err(_) => return,
                };
                if watcher.watch(&root, RecursiveMode::Recursive).is_err() {
                    return;
                }
                while !stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(200));
                }
            });
        }

        // HTTP thread. recv_timeout lets us notice the stop flag instead of
        // blocking forever on recv().
        {
            let stop = stop.clone();
            let version = version.clone();
            let root = root.clone();
            thread::spawn(move || loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match server.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(req)) => {
                        let _ = handle(req, &root, &version);
                    }
                    Ok(None) => continue,
                    Err(_) => break,
                }
            });
        }

        Ok(Self { port, stop })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn respond_str(req: tiny_http::Request, code: u16, body: &str) -> Result<()> {
    let res = Response::from_string(body)
        .with_status_code(code)
        .with_header(hdr("Content-Type", "text/plain; charset=utf-8"));
    req.respond(res)?;
    Ok(())
}

fn handle(req: tiny_http::Request, root: &Path, version: &AtomicU64) -> Result<()> {
    let url = req.url().split('?').next().unwrap_or("/").to_string();

    if url == "/__vmn_reload" {
        let body = version.load(Ordering::Relaxed).to_string();
        let res = Response::from_string(body)
            .with_header(hdr("Content-Type", "text/plain"))
            .with_header(hdr("Cache-Control", "no-store"));
        req.respond(res)?;
        return Ok(());
    }

    let rel = url.trim_start_matches('/');
    let mut path = root.join(rel);
    if path.is_dir() {
        path = path.join("index.html");
    }

    // Resolve symlinks and `..` before checking containment, so a crafted URL
    // can't escape the served root.
    let canon = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return respond_str(req, 404, "404 not found"),
    };
    let root_canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !canon.starts_with(&root_canon) {
        return respond_str(req, 403, "403 forbidden");
    }

    let data = match std::fs::read(&canon) {
        Ok(d) => d,
        Err(_) => return respond_str(req, 404, "404 not found"),
    };

    let ext = canon
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mime = mime_for(&ext);

    if ext == "html" || ext == "htm" {
        let mut s = String::from_utf8_lossy(&data).into_owned();
        match s.rfind("</body>") {
            Some(i) => s.insert_str(i, RELOAD_JS),
            None => s.push_str(RELOAD_JS),
        }
        let res = Response::from_data(s.into_bytes())
            .with_header(hdr("Content-Type", mime))
            .with_header(hdr("Cache-Control", "no-store"));
        req.respond(res)?;
        return Ok(());
    }

    let res = Response::from_data(data)
        .with_header(hdr("Content-Type", mime))
        .with_header(hdr("Cache-Control", "no-store"));
    req.respond(res)?;
    Ok(())
}
