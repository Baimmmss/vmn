use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use vimini::server::LiveServer;

/// Minimal HTTP/1.1 GET so the test needs no client dependency.
fn get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).expect("read");
    buf
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("vmn_test_{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn serves_html_and_injects_reload_script() {
    let dir = scratch("html");
    std::fs::write(
        dir.join("index.html"),
        "<html><body><h1>hello vmn</h1></body></html>",
    )
    .unwrap();

    let port = 39217;
    let _srv = LiveServer::start(dir.clone(), port).expect("start");
    std::thread::sleep(Duration::from_millis(300));

    let res = get(port, "/");
    assert!(res.starts_with("HTTP/1.1 200"), "status line: {res:?}");
    assert!(res.contains("hello vmn"), "body missing original content");
    assert!(res.contains("__vmn_reload"), "reload script was not injected");
    assert!(res.contains("text/html"), "wrong content-type");
}

#[test]
fn reload_endpoint_bumps_when_a_file_changes() {
    let dir = scratch("reload");
    std::fs::write(dir.join("index.html"), "<html><body>v1</body></html>").unwrap();

    let port = 39218;
    let _srv = LiveServer::start(dir.clone(), port).expect("start");
    std::thread::sleep(Duration::from_millis(300));

    let before = get(port, "/__vmn_reload");
    let before_v: u64 = before.rsplit("\r\n\r\n").next().unwrap().trim().parse().unwrap();

    std::fs::write(dir.join("index.html"), "<html><body>v2</body></html>").unwrap();
    std::thread::sleep(Duration::from_millis(800));

    let after = get(port, "/__vmn_reload");
    let after_v: u64 = after.rsplit("\r\n\r\n").next().unwrap().trim().parse().unwrap();

    assert!(
        after_v > before_v,
        "version did not advance after edit: {before_v} -> {after_v}"
    );
}

#[test]
fn rejects_path_traversal_outside_root() {
    let dir = scratch("traversal");
    std::fs::write(dir.join("index.html"), "<html><body>ok</body></html>").unwrap();

    let port = 39219;
    let _srv = LiveServer::start(dir.clone(), port).expect("start");
    std::thread::sleep(Duration::from_millis(300));

    let res = get(port, "/../../../../../../etc/passwd");
    assert!(
        res.starts_with("HTTP/1.1 403") || res.starts_with("HTTP/1.1 404"),
        "traversal was not blocked: {res:?}"
    );
    assert!(!res.contains("root:"), "leaked /etc/passwd contents");
}

#[test]
fn missing_file_is_404() {
    let dir = scratch("missing");
    let port = 39220;
    let _srv = LiveServer::start(dir, port).expect("start");
    std::thread::sleep(Duration::from_millis(300));

    let res = get(port, "/nope.js");
    assert!(res.starts_with("HTTP/1.1 404"), "expected 404: {res:?}");
}
