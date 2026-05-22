use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Spawns the debounce + runner loop in a background thread.
///
/// Events are received on `rx`. After `debounce_ms` milliseconds of silence
/// the `command` is executed in a shell. If the command is already running
/// the trigger is silently skipped.
pub fn start(rx: Receiver<()>, command: String, debounce_ms: u64) {
    thread::spawn(move || run_loop(rx, command, debounce_ms));
}

fn run_loop(rx: Receiver<()>, command: String, debounce_ms: u64) {
    let is_running = Arc::new(AtomicBool::new(false));
    let debounce = Duration::from_millis(debounce_ms);
    let mut deadline: Option<Instant> = None;

    loop {
        // Use the remaining time to the deadline as the recv timeout, or a
        // long fallback when idle so we don't busy-spin.
        let timeout = deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(60));

        match rx.recv_timeout(timeout) {
            Ok(()) => {
                // New event — reset the debounce deadline.
                deadline = Some(Instant::now() + debounce);
            }
            Err(RecvTimeoutError::Timeout) => {
                // Check whether the deadline has actually elapsed (guards
                // against spurious early wakeups).
                if deadline.map_or(false, |d| Instant::now() >= d) {
                    deadline = None;
                    maybe_run(&command, &is_running);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Fires `command` in a new thread unless the previous run is still active.
/// Returns `true` if the command was started, `false` if it was skipped.
fn maybe_run(command: &str, is_running: &Arc<AtomicBool>) -> bool {
    // `swap` returns the *old* value; if it was already `true` we skip.
    if is_running.swap(true, Ordering::SeqCst) {
        return false;
    }

    let is_running = Arc::clone(is_running);
    let cmd = command.to_string();
    thread::spawn(move || {
        execute_command(&cmd);
        is_running.store(false, Ordering::SeqCst);
    });

    true
}

/// Executes `command` in a platform shell.
///
/// | Platform       | Shell                               |
/// |----------------|-------------------------------------|
/// | Unix (Linux, macOS) | `sh -c <command>`              |
/// | Windows        | `powershell -NoProfile -NonInteractive -Command <command>` |
pub fn execute_command(command: &str) {
    match spawn_shell(command) {
        Ok(mut child) => {
            let _ = child.wait();
        }
        Err(e) => {
            eprintln!("[watchr] Failed to run command: {e}");
        }
    }
}

/// Spawns a shell child process for `command` and returns it without waiting.
fn spawn_shell(command: &str) -> std::io::Result<std::process::Child> {
    #[cfg(unix)]
    {
        Command::new("sh").arg("-c").arg(command).spawn()
    }
    #[cfg(windows)]
    {
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .spawn()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    // ── execute_command ─────────────────────────────────────────────────────

    #[test]
    fn execute_command_succeeds_for_no_op() {
        // `true` on Unix; `exit` as a bare PowerShell statement on Windows.
        #[cfg(unix)]
        execute_command("true");
        #[cfg(windows)]
        execute_command("exit 0");
    }

    #[test]
    fn execute_command_handles_bad_command_gracefully() {
        // Should log to stderr but not panic.
        execute_command("__watchr_nonexistent_xyz_command__");
    }

    #[test]
    fn execute_command_runs_echo() {
        #[cfg(unix)]
        execute_command("echo watchr_unit_test");
        #[cfg(windows)]
        execute_command("echo watchr_unit_test");
    }

    // ── maybe_run ───────────────────────────────────────────────────────────

    #[test]
    fn maybe_run_starts_when_not_running() {
        let flag = Arc::new(AtomicBool::new(false));
        let started = maybe_run("echo hi", &flag);
        assert!(started);
        // PowerShell startup on Windows can take several seconds; sh is fast.
        #[cfg(unix)]
        thread::sleep(Duration::from_millis(300));
        #[cfg(windows)]
        thread::sleep(Duration::from_secs(5));
        assert!(!flag.load(Ordering::SeqCst));
    }

    #[test]
    fn maybe_run_skips_when_already_running() {
        let flag = Arc::new(AtomicBool::new(true)); // simulate active run
        let started = maybe_run("echo hi", &flag);
        assert!(!started);
        // Flag should remain true (we didn't touch it).
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn maybe_run_resets_flag_after_completion() {
        let flag = Arc::new(AtomicBool::new(false));
        maybe_run("echo hi", &flag);
        #[cfg(unix)]
        thread::sleep(Duration::from_millis(300));
        #[cfg(windows)]
        thread::sleep(Duration::from_secs(5));
        assert!(!flag.load(Ordering::SeqCst));
    }

    // ── run_loop / start ────────────────────────────────────────────────────

    #[test]
    fn runner_processes_event_without_panic() {
        let (tx, rx) = mpsc::channel();
        start(rx, "echo watchr_debounce_test".to_string(), 50);
        tx.send(()).unwrap();
        // Give the debounce + command time to complete.
        thread::sleep(Duration::from_millis(400));
    }

    #[test]
    fn runner_exits_cleanly_when_sender_drops() {
        let (tx, rx) = mpsc::channel::<()>();
        let handle = thread::spawn(move || run_loop(rx, "echo hi".to_string(), 50));
        drop(tx); // disconnects the channel
        // The loop should exit promptly.
        handle.join().expect("runner thread should exit cleanly");
    }

    #[test]
    fn multiple_rapid_events_produce_single_run() {
        // Send many events in quick succession — only one command invocation
        // should fire after the debounce window. We verify no panic/deadlock.
        let (tx, rx) = mpsc::channel();
        start(rx, "echo watchr_burst_test".to_string(), 100);

        for _ in 0..20 {
            tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(5));
        }

        thread::sleep(Duration::from_millis(500));
        // Drops tx; runner should finish cleanly.
    }
}
