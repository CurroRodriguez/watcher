//! Integration tests for watchr.
//!
//! These tests exercise the public API end-to-end: CLI resolution, the
//! filesystem watcher, and the command runner.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tempfile::TempDir;
use watchr::cli::Args;
use watchr::{runner, watcher};

// ── CLI resolution ───────────────────────────────────────────────────────────

fn parse(args: &[&str]) -> Args {
    Args::try_parse_from(std::iter::once("watchr").chain(args.iter().copied())).unwrap()
}

use clap::Parser;

#[test]
fn cli_positional_dirs_and_command_resolve() {
    let config = parse(&["./src", "./lib", "cargo test"]).resolve().unwrap();
    assert_eq!(config.command, "cargo test");
    assert_eq!(
        config.dirs,
        vec![PathBuf::from("./src"), PathBuf::from("./lib")]
    );
}

#[test]
fn cli_exec_flag_with_multiple_watch_flags() {
    let config = parse(&["--watch", "./a", "--watch", "./b", "--exec", "make"])
        .resolve()
        .unwrap();
    assert_eq!(config.command, "make");
    assert_eq!(
        config.dirs,
        vec![PathBuf::from("./a"), PathBuf::from("./b")]
    );
}

#[test]
fn cli_debounce_default_and_override() {
    let default = parse(&["./src", "echo hi"]).resolve().unwrap();
    assert_eq!(default.debounce_ms, 500);

    let custom = parse(&["--debounce", "250", "./src", "echo hi"])
        .resolve()
        .unwrap();
    assert_eq!(custom.debounce_ms, 250);
}

#[test]
fn cli_ignore_patterns_include_git_by_default() {
    let config = parse(&["./src", "echo hi"]).resolve().unwrap();
    assert!(config.ignore_patterns.iter().any(|p| p == ".git/**"));
}

#[test]
fn cli_user_ignore_patterns_are_preserved() {
    let config = parse(&["--ignore", "**/*.log", "--ignore", "dist/**", "./src", "echo hi"])
        .resolve()
        .unwrap();
    assert!(config.ignore_patterns.contains(&"**/*.log".to_string()));
    assert!(config.ignore_patterns.contains(&"dist/**".to_string()));
}

#[test]
fn cli_error_on_missing_command() {
    let args = Args::try_parse_from(["watchr"]).unwrap();
    assert!(args.resolve().is_err());
}

#[test]
fn cli_error_on_missing_directory() {
    let args = parse(&["--exec", "make"]);
    assert!(args.resolve().is_err());
}

// ── Watcher ──────────────────────────────────────────────────────────────────

#[test]
fn watcher_detects_new_file() {
    let dir = TempDir::new().unwrap();
    let (tx, rx) = mpsc::channel();
    let ignore_set = watcher::build_ignore_set(&[".git/**".to_string()]).unwrap();

    let _w = watcher::create_watcher(&[dir.path().to_path_buf()], ignore_set, tx).unwrap();

    // Give the watcher time to initialize before making changes.
    thread::sleep(Duration::from_millis(100));

    fs::write(dir.path().join("hello.txt"), b"world").unwrap();

    let result = rx.recv_timeout(Duration::from_secs(3));
    assert!(result.is_ok(), "Expected a file-change event, got nothing");
}

#[test]
fn watcher_detects_modified_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("existing.txt");
    fs::write(&file, b"initial").unwrap();

    let (tx, rx) = mpsc::channel();
    let ignore_set = watcher::build_ignore_set(&[".git/**".to_string()]).unwrap();
    let _w = watcher::create_watcher(&[dir.path().to_path_buf()], ignore_set, tx).unwrap();

    thread::sleep(Duration::from_millis(100));
    fs::write(&file, b"updated").unwrap();

    assert!(
        rx.recv_timeout(Duration::from_secs(3)).is_ok(),
        "Expected event for modified file"
    );
}

#[test]
fn watcher_ignores_matching_patterns() {
    let dir = TempDir::new().unwrap();
    let (tx, rx) = mpsc::channel();
    let ignore_set =
        watcher::build_ignore_set(&[".git/**".to_string(), "**/*.tmp".to_string()]).unwrap();

    let _w = watcher::create_watcher(&[dir.path().to_path_buf()], ignore_set, tx).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Write an ignored file.
    fs::write(dir.path().join("cache.tmp"), b"temp").unwrap();

    // No event should arrive within the timeout.
    let result = rx.recv_timeout(Duration::from_millis(600));
    assert!(
        result.is_err(),
        "Expected no event for ignored .tmp file, but got one"
    );
}

#[test]
fn watcher_watches_multiple_directories() {
    let dir_a = TempDir::new().unwrap();
    let dir_b = TempDir::new().unwrap();

    let (tx, rx) = mpsc::channel();
    let ignore_set = watcher::build_ignore_set(&[".git/**".to_string()]).unwrap();
    let _w = watcher::create_watcher(
        &[dir_a.path().to_path_buf(), dir_b.path().to_path_buf()],
        ignore_set,
        tx,
    )
    .unwrap();

    thread::sleep(Duration::from_millis(100));

    // Change in the second directory.
    fs::write(dir_b.path().join("b.txt"), b"b").unwrap();

    assert!(
        rx.recv_timeout(Duration::from_secs(3)).is_ok(),
        "Expected event from second watched directory"
    );
}

// ── Runner ───────────────────────────────────────────────────────────────────

#[test]
fn runner_executes_command_after_debounce() {
    let dir = TempDir::new().unwrap();
    let output = dir.path().join("ran.txt");

    #[cfg(unix)]
    let cmd = format!("touch '{}'", output.display()); // single-quotes handle spaces
    #[cfg(windows)]
    let cmd = format!("New-Item -Force -ItemType File '{}' | Out-Null", output.display());

    let (tx, rx) = mpsc::channel();
    runner::start(rx, cmd, 80);

    tx.send(()).unwrap();
    // PowerShell startup on Windows can take several seconds; sh is fast.
    #[cfg(unix)]
    thread::sleep(Duration::from_millis(600));
    #[cfg(windows)]
    thread::sleep(Duration::from_secs(6));

    assert!(output.exists(), "Command should have created the output file");
}

#[test]
fn runner_skips_while_command_running() {
    // Send two events quickly so the second arrives while the first run is
    // still in progress. We verify no panic and the runner stays responsive.
    let (tx, rx) = mpsc::channel();
    runner::start(rx, "echo watchr_skip_test".to_string(), 30);

    tx.send(()).unwrap();
    thread::sleep(Duration::from_millis(10)); // before debounce expires
    tx.send(()).unwrap();

    thread::sleep(Duration::from_millis(400));
    // No assertion beyond "didn't deadlock / panic".
}

#[test]
fn runner_handles_rapid_burst_of_events() {
    let (tx, rx) = mpsc::channel();
    runner::start(rx, "echo watchr_burst".to_string(), 50);

    for _ in 0..50 {
        let _ = tx.send(());
    }

    thread::sleep(Duration::from_millis(500));
}

// ── Watcher + Runner end-to-end ──────────────────────────────────────────────

#[test]
fn end_to_end_file_change_triggers_command() {
    let watch_dir = TempDir::new().unwrap();
    let output_dir = TempDir::new().unwrap();
    let output = output_dir.path().join("triggered.txt");

    #[cfg(unix)]
    let cmd = format!("touch '{}'", output.display()); // single-quotes handle spaces
    #[cfg(windows)]
    let cmd = format!("New-Item -Force -ItemType File '{}' | Out-Null", output.display());

    let (tx, rx) = mpsc::channel();
    let ignore_set = watcher::build_ignore_set(&[".git/**".to_string()]).unwrap();
    let _w =
        watcher::create_watcher(&[watch_dir.path().to_path_buf()], ignore_set, tx).unwrap();

    runner::start(rx, cmd, 80);

    thread::sleep(Duration::from_millis(100));
    fs::write(watch_dir.path().join("trigger.txt"), b"go").unwrap();

    // Allow debounce + command execution to complete; PowerShell needs more time.
    #[cfg(unix)]
    thread::sleep(Duration::from_millis(600));
    #[cfg(windows)]
    thread::sleep(Duration::from_secs(6));

    assert!(
        output.exists(),
        "End-to-end: file change should have triggered the command"
    );
}
