use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::types::{ProjectConfig, RunTimeoutError, ValidationSummary};

pub fn run_configured_validations(
    project_path: &Path,
    project_config: &ProjectConfig,
    get_timeout_ms: impl Fn() -> Option<u64>,
) -> Result<Vec<ValidationSummary>, ValidationError> {
    let steps: [(&str, &Option<String>); 3] = [
        ("lint", &project_config.validation.lint_command),
        ("test", &project_config.validation.test_command),
        ("typecheck", &project_config.validation.typecheck_command),
    ];

    let mut results = Vec::new();
    for (name, command) in steps {
        let Some(command) = command else {
            continue;
        };
        let timeout_ms = get_timeout_ms();
        if let Some(ms) = timeout_ms {
            if ms == 0 {
                return Err(ValidationError::Timeout(RunTimeoutError(
                    "Run exceeded timeout before validation completed.".to_string(),
                )));
            }
        }
        let exit_code = run_shell_command(project_path, command, timeout_ms)?;
        results.push(ValidationSummary {
            name: super::types::ValidationName::from_str(name).unwrap(),
            command: command.clone(),
            exit_code,
        });
        if exit_code != 0 {
            break;
        }
    }
    Ok(results)
}

pub enum ValidationError {
    Timeout(RunTimeoutError),
    Other(String),
}

fn run_shell_command(project_path: &Path, command: &str, timeout_ms: Option<u64>) -> Result<i32, ValidationError> {
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
                            "Validation command exceeded timeout of {}ms.", timeout_ms.unwrap_or(0)
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
