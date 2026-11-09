use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::types::{
    CustomValidation, ProjectConfig, RunTimeoutError, ValidationName, ValidationSummary,
};

/// Ordered, configured validation steps for a project: the built-in
/// lint/test/typecheck trio (when configured) followed by the project's
/// custom gates in listed order. One source of truth for the real command
/// path, the scheduler's mock-runner path, and auto-merge readiness.
pub fn configured_steps(project_config: &ProjectConfig) -> Vec<(ValidationName, String)> {
    let mut steps = Vec::new();
    for (name, command) in [
        (
            ValidationName::Lint,
            &project_config.validation.lint_command,
        ),
        (
            ValidationName::Test,
            &project_config.validation.test_command,
        ),
        (
            ValidationName::Typecheck,
            &project_config.validation.typecheck_command,
        ),
    ] {
        // TS truthiness: null AND "" both mean "not configured".
        if let Some(command) = command.as_deref().filter(|c| !c.is_empty()) {
            steps.push((name, command.to_string()));
        }
    }
    for custom in project_config.validation.custom.clone().unwrap_or_default() {
        let CustomValidation { name, command } = custom;
        if !command.is_empty() {
            steps.push((ValidationName::Custom(name), command));
        }
    }
    steps
}

pub fn run_configured_validations(
    project_path: &Path,
    project_config: &ProjectConfig,
    get_timeout_ms: impl Fn() -> Option<u64>,
) -> Result<Vec<ValidationSummary>, ValidationError> {
    let mut results = Vec::new();
    for (name, command) in configured_steps(project_config) {
        let timeout_ms = get_timeout_ms();
        if let Some(ms) = timeout_ms {
            if ms == 0 {
                return Err(ValidationError::Timeout(RunTimeoutError(
                    "Run exceeded timeout before validation completed.".to_string(),
                )));
            }
        }
        let exit_code = run_shell_command(project_path, &command, timeout_ms)?;
        results.push(ValidationSummary {
            name,
            command,
            exit_code,
        });
        if exit_code != 0 {
            break;
        }
    }
    Ok(results)
}

#[derive(Debug)]
pub enum ValidationError {
    Timeout(RunTimeoutError),
    Other(String),
}

fn run_shell_command(
    project_path: &Path,
    command: &str,
    timeout_ms: Option<u64>,
) -> Result<i32, ValidationError> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(project_path)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ValidationError::Other(e.to_string()))?;

    let deadline = timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(status.code().unwrap_or(1));
            }
            Ok(None) => {
                // Daemon shutdown during a long validation must not park the
                // tick until the command's own timeout elapses (TS exited in
                // the signal handler); the sentinel flows into the scheduler
                // shutdown arm, which books nothing.
                if super::worker::SHUTDOWN.load(std::sync::atomic::Ordering::SeqCst) {
                    let pid = child.id() as i32;
                    let _ = kill_pid(pid);
                    let kill_deadline = Instant::now() + Duration::from_millis(5000);
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => break,
                            Ok(None) if Instant::now() >= kill_deadline => {
                                let _ = kill_pid_kill(pid);
                                let _ = child.wait();
                                break;
                            }
                            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                            Err(_) => break,
                        }
                    }
                    return Err(ValidationError::Other("__shutdown__:".to_string()));
                }
                if let Some(deadline) = deadline {
                    if Instant::now() >= deadline {
                        let pid = child.id() as i32;
                        let _ = kill_pid(pid);
                        // Bounded SIGTERM grace, then SIGKILL; always reap so
                        // no zombie outlives the run (std never auto-reaps).
                        let kill_deadline = Instant::now() + Duration::from_millis(5000);
                        loop {
                            match child.try_wait() {
                                Ok(Some(_)) => break,
                                Ok(None) if Instant::now() >= kill_deadline => {
                                    let _ = kill_pid_kill(pid);
                                    let _ = child.wait();
                                    break;
                                }
                                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                                Err(_) => break,
                            }
                        }
                        return Err(ValidationError::Timeout(RunTimeoutError(format!(
                            "Validation command exceeded timeout of {}ms.",
                            timeout_ms.unwrap_or(0)
                        ))));
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(ValidationError::Other(e.to_string())),
        }
    }
}

fn kill_pid(pid: i32) -> bool {
    unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
}

fn kill_pid_kill(pid: i32) -> bool {
    unsafe { libc::kill(pid, libc::SIGKILL) == 0 }
}
