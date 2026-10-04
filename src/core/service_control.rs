// Daemon lifecycle control shared by the CLI `service` subcommands and the
// TUI service view: one implementation of pid-file inspection, start, stop,
// and restart so both surfaces behave identically.

use std::path::PathBuf;

use crate::core as c;

pub enum DaemonInspection {
    Missing,
    Stale(Option<i32>),
    Foreign(i32),
    Running(i32),
}

pub fn inspect_daemon_process() -> Result<DaemonInspection, String> {
    let pid_path = c::paths::daemon_pid_path(None);
    if !c::fsutil::file_exists(&pid_path) {
        return Ok(DaemonInspection::Missing);
    }
    let raw = std::fs::read_to_string(&pid_path).unwrap_or_default();
    let pid = raw.trim().parse::<i32>().ok().filter(|p| *p > 0);
    let Some(pid) = pid else {
        return Ok(DaemonInspection::Stale(None));
    };
    if !is_process_alive(pid) {
        return Ok(DaemonInspection::Stale(Some(pid)));
    }
    if !is_likely_openloop_daemon(pid) {
        return Ok(DaemonInspection::Foreign(pid));
    }
    Ok(DaemonInspection::Running(pid))
}

fn is_process_alive(pid: i32) -> bool {
    unsafe {
        let result = libc::kill(pid, 0);
        if result == 0 {
            return true;
        }
        let errno = *libc::__errno_location();
        if errno == libc::EPERM {
            return true;
        }
        false
    }
}

fn is_likely_openloop_daemon(pid: i32) -> bool {
    if cfg!(target_os = "linux") {
        return std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
            .map(|cmdline| cmdline.replace('\0', " ").trim().contains("daemon worker"))
            .unwrap_or(false);
    }
    if cfg!(target_os = "macos") {
        return std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .contains("daemon worker")
            })
            .unwrap_or(false);
    }
    true
}

fn spawn_daemon_worker() {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("openloop"));
    let _ = std::process::Command::new(exe)
        .arg("daemon")
        .arg("worker")
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn poll_for_daemon_ready(timeout_ms: u64) -> bool {
    let deadline = c::now_millis() + timeout_ms as i64;
    while c::now_millis() < deadline {
        if matches!(inspect_daemon_process(), Ok(DaemonInspection::Running(_))) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

pub fn assert_default_provider_available() -> Result<(), String> {
    let config = c::global_config::load_global_config(None)?;
    c::pi::assert_provider_available(None, config.default_provider.as_deref())
}

/// Start the daemon. Returns a human-readable outcome line.
pub fn start_service() -> Result<String, String> {
    assert_default_provider_available()?;
    match inspect_daemon_process()? {
        DaemonInspection::Running(_) => Ok("Daemon appears to be running already.".into()),
        DaemonInspection::Foreign(pid) => Err(format!(
            "Refusing to start: pid file points to live non-openloop process {pid}."
        )),
        DaemonInspection::Stale(_) => {
            let _ = std::fs::remove_file(c::paths::daemon_pid_path(None));
            start_and_wait()
        }
        DaemonInspection::Missing => start_and_wait(),
    }
}

fn start_and_wait() -> Result<String, String> {
    spawn_daemon_worker();
    if poll_for_daemon_ready(5000) {
        let pid = match inspect_daemon_process()? {
            DaemonInspection::Running(pid) => format!(" (PID {pid})"),
            _ => String::new(),
        };
        Ok(format!(
            "Daemon started{pid}. Log: {}",
            c::paths::daemon_log_path(None).display()
        ))
    } else {
        Err(format!(
            "Daemon may not have started. Check logs: {}",
            c::paths::daemon_log_path(None).display()
        ))
    }
}

/// Stop the daemon (SIGTERM; the worker escalates and reaps itself).
pub fn stop_service() -> Result<String, String> {
    match inspect_daemon_process()? {
        DaemonInspection::Missing => Ok("Daemon is not running.".into()),
        DaemonInspection::Stale(_) => {
            let _ = std::fs::remove_file(c::paths::daemon_pid_path(None));
            Ok("Removed stale daemon pid file.".into())
        }
        DaemonInspection::Foreign(pid) => Err(format!(
            "Refusing to stop: pid file points to live non-openloop process {pid}."
        )),
        DaemonInspection::Running(pid) => {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            let _ = std::fs::remove_file(c::paths::daemon_pid_path(None));
            Ok("Daemon stopped.".into())
        }
    }
}

/// Restart: stop, wait for exit, then start.
pub fn restart_service() -> Result<String, String> {
    assert_default_provider_available()?;
    match inspect_daemon_process()? {
        DaemonInspection::Missing => {}
        DaemonInspection::Stale(_) => {
            let _ = std::fs::remove_file(c::paths::daemon_pid_path(None));
        }
        DaemonInspection::Foreign(pid) => {
            return Err(format!(
                "Refusing to restart: pid file points to live non-openloop process {pid}."
            ));
        }
        DaemonInspection::Running(pid) => {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            // W2: Wait for the process to actually exit (up to 10 seconds).
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if !is_process_alive(pid) {
                    break;
                }
                if let Ok(DaemonInspection::Running(new_pid)) = inspect_daemon_process() {
                    if new_pid != pid {
                        break; // a fresh daemon already took over the pid file
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            let _ = std::fs::remove_file(c::paths::daemon_pid_path(None));
        }
    }
    start_and_wait()
}

/// True when the daemon process is alive right now.
pub fn daemon_running() -> bool {
    matches!(inspect_daemon_process(), Ok(DaemonInspection::Running(_)))
}
