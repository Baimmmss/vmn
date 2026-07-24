use std::time::Duration;
use vimini::terminal::Terminal;

/// Poll the emulator screen until `needle` appears or we give up.
fn wait_for(term: &Terminal, needle: &str, tries: usize) -> bool {
    for _ in 0..tries {
        std::thread::sleep(Duration::from_millis(100));
        let binding = term.parser();
        let parser = binding.lock().unwrap();
        if parser.screen().contents().contains(needle) {
            return true;
        }
    }
    false
}

#[test]
fn shell_runs_a_command_and_output_reaches_the_screen() {
    let dir = std::env::temp_dir();
    let mut term = Terminal::new(&dir, 24, 80).expect("spawn terminal");

    // Give the shell a moment to print its prompt.
    std::thread::sleep(Duration::from_millis(400));

    term.send(b"echo vmn_terminal_ok\r");

    assert!(
        wait_for(&term, "vmn_terminal_ok", 40),
        "command output never appeared on the emulated screen"
    );
}

#[test]
fn shell_inherits_a_working_path() {
    let dir = std::env::temp_dir();
    let mut term = Terminal::new(&dir, 24, 80).expect("spawn terminal");
    std::thread::sleep(Duration::from_millis(400));

    // `ls` must resolve via PATH; if the environment were empty this fails.
    term.send(b"ls / && echo VMN_LS_DONE\r");

    assert!(
        wait_for(&term, "VMN_LS_DONE", 40),
        "external command did not run — PATH not inherited?"
    );
}

#[test]
fn exit_is_detected() {
    let dir = std::env::temp_dir();
    let mut term = Terminal::new(&dir, 24, 80).expect("spawn terminal");
    std::thread::sleep(Duration::from_millis(400));

    assert!(!term.has_exited(), "shell should be alive right after spawn");

    term.send(b"exit\r");

    let mut exited = false;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        if term.has_exited() {
            exited = true;
            break;
        }
    }
    assert!(exited, "shell exit was not detected");
}
