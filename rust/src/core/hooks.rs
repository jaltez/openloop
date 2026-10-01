use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::event_log::{append_event, now_event};
use super::now_iso;
use super::types::{GlobalConfig, LifecycleHookConfig, ProjectConfig, ValidationSummary};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct LifecycleHookPayload {
    pub event: String,
    pub project: String,
    #[serde(rename = "taskId")]
    pub task_id: String,
    pub message: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub promotion_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_snapshot_usd: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub validation: Vec<ValidationSummary>,
    #[serde(rename = "approvalPacketPath", skip_serializing_if = "Option::is_none")]
    pub approval_packet_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_promotions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_checks: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct LifecycleHookResult {
    pub notes: Vec<String>,
    pub require_manual_review: bool,
}

#[derive(Debug, Clone, Default)]
struct HookResponse {
    note: Option<String>,
    require_manual_review: bool,
}

pub fn run_lifecycle_hooks(
    global_config: &GlobalConfig,
    project_config: Option<&ProjectConfig>,
    payload: &LifecycleHookPayload,
    daemon_log: Option<&str>,
) -> LifecycleHookResult {
    let mut entries: Vec<(&str, LifecycleHookConfig)> = Vec::new(); // (scope, hook)
    if let Some(hooks) = &global_config.hooks {
        for hook in hooks {
            entries.push(("global", hook.clone()));
        }
    }
    if let Some(hooks) = project_config.and_then(|c| c.hooks.as_ref()) {
        for hook in hooks.iter() {
            entries.push(("project", hook.clone()));
        }
    }

    let mut result = LifecycleHookResult::default();

    for (scope, hook) in entries {
        if hook.disabled.unwrap_or(false) {
            continue;
        }
        if !hook.events.is_empty()
            && !hook.events.iter().any(|e| e == "*" || e == &payload.event)
        {
            continue;
        }

        let response = if hook.hook_type == "command" {
            run_command_hook(&hook, payload)
        } else {
            run_webhook_hook(&hook, payload)
        };

        match response {
            Ok(response) => {
                if let Some(note) = &response.note {
                    result.notes.push(format!("[{scope}:{}] {note}", hook.hook_type));
                }
                if response.require_manual_review {
                    result.require_manual_review = true;
                }
            }
            Err(error) => {
                if let Some(log) = daemon_log {
                    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(log) {
                        let _ = writeln!(
                            f,
                            "[{}] hook failure ({scope}:{}/{}): {error}",
                            now_iso(),
                            hook.hook_type,
                            payload.event
                        );
                    }
                }
                let mut event = now_event("hook_failed");
                event.project = Some(payload.project.clone());
                event.task_id = if payload.task_id.is_empty() { None } else { Some(payload.task_id.clone()) };
                event.hook_event = Some(payload.event.clone());
                event.hook_type = Some(hook.hook_type.clone());
                event.hook_scope = Some(scope.to_string());
                event.error = Some(error);
                let _ = append_event(&event, None);
            }
        }
    }

    result
}

fn run_command_hook(hook: &LifecycleHookConfig, payload: &LifecycleHookPayload) -> Result<HookResponse, String> {
    let Some(command) = &hook.command else {
        return Err("Command hook is missing a command.".to_string());
    };

    let timeout_seconds = hook.timeout_seconds.unwrap_or(10);
    let body = serde_json::to_string(payload).unwrap_or_default();

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("OPENLOOP_EVENT", &payload.event)
        .env("OPENLOOP_PROJECT", &payload.project)
        .env("OPENLOOP_TASK_ID", &payload.task_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(body.as_bytes());
    }
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();
    let stdout_buf = read_to_string_async(stdout_handle);
    let stderr_buf = read_to_string_async(stderr_handle);

    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = kill_pid(child.id() as i32);
                    return Err(format!("Hook timed out after {timeout_seconds}s."));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e.to_string()),
        }
    };

    let stdout = stdout_buf.join().unwrap_or_default();
    let stderr = stderr_buf.join().unwrap_or_default();

    if !status.map(|s| s.success()).unwrap_or(false) {
        let message = stderr.trim();
        if message.is_empty() {
            return Err(format!(
                "Hook exited with status {}.",
                status.and_then(|s| s.code()).unwrap_or(1)
            ));
        }
        return Err(message.to_string());
    }
    Ok(parse_hook_response(stdout.as_bytes()))
}

fn read_to_string_async<R: Read + Send + 'static>(
    handle: Option<R>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut out = String::new();
        if let Some(mut handle) = handle {
            let _ = handle.read_to_string(&mut out);
        }
        out
    })
}

fn parse_hook_response(raw: &[u8]) -> HookResponse {
    let trimmed = String::from_utf8_lossy(raw).trim().to_string();
    if trimmed.is_empty() {
        return HookResponse::default();
    }
    match serde_json::from_str::<serde_json::Value>(&trimmed) {
        Ok(parsed) => HookResponse {
            note: parsed
                .get("note")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            require_manual_review: parsed.get("requireManualReview").and_then(|v| v.as_bool()) == Some(true),
        },
        Err(_) => HookResponse {
            note: Some(trimmed),
            require_manual_review: false,
        },
    }
}

fn kill_pid(pid: i32) -> bool {
    unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
}

fn run_webhook_hook(hook: &LifecycleHookConfig, payload: &LifecycleHookPayload) -> Result<HookResponse, String> {
    let Some(url) = &hook.url else {
        return Err("Webhook hook is missing a URL.".to_string());
    };
    let timeout_seconds = hook.timeout_seconds.unwrap_or(10);
    let body = serde_json::to_string(payload).unwrap_or_default();

    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
        .post(url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "openloop-hooks/1.0")
        .send_string(&body)
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let mut body = String::new();
                let _ = resp.into_reader().take(200).read_to_string(&mut body);
                format!("Webhook hook returned {code}: {body}")
            }
            other => format!("Webhook hook timed out or failed: {other}"),
        })?;

    let mut response_body = String::new();
    let _ = response
        .into_reader()
        .take(1_000_000)
        .read_to_string(&mut response_body);
    Ok(parse_hook_response(response_body.as_bytes()))
}

pub fn _unused(_p: &Path) {}
