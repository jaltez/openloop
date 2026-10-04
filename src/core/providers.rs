use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::active_runs;

use super::types::{AgentRunResult, AgentUsage};

#[derive(Debug, Clone)]
pub struct AgentRunOptions<'a> {
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub project_path: &'a Path,
    pub project_alias: Option<&'a str>,
    pub timeout_ms: Option<u64>,
    pub extra_args: Option<&'a [String]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderName {
    Pi,
    Claude,
    Aider,
    Codex,
    Opencode,
    Ka,
    Omp,
    Custom,
}

impl ProviderName {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderName::Pi => "pi",
            ProviderName::Claude => "claude",
            ProviderName::Aider => "aider",
            ProviderName::Codex => "codex",
            ProviderName::Opencode => "opencode",
            ProviderName::Ka => "ka",
            ProviderName::Omp => "omp",
            ProviderName::Custom => "custom",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ProviderName::Pi => "Pi",
            ProviderName::Claude => "Claude Code",
            ProviderName::Aider => "Aider",
            ProviderName::Codex => "OpenAI Codex",
            ProviderName::Opencode => "OpenCode",
            ProviderName::Ka => "ka",
            ProviderName::Omp => "Oh My Pi",
            ProviderName::Custom => "Custom",
        }
    }

    pub fn parse(s: &str) -> Option<ProviderName> {
        Some(match s {
            "pi" => ProviderName::Pi,
            "claude" => ProviderName::Claude,
            "aider" => ProviderName::Aider,
            "codex" => ProviderName::Codex,
            "opencode" => ProviderName::Opencode,
            "ka" => ProviderName::Ka,
            "omp" => ProviderName::Omp,
            "custom" => ProviderName::Custom,
            _ => return None,
        })
    }
}

pub const PROVIDER_NAMES: &[ProviderName] = &[
    ProviderName::Pi,
    ProviderName::Claude,
    ProviderName::Aider,
    ProviderName::Codex,
    ProviderName::Opencode,
    ProviderName::Ka,
    ProviderName::Omp,
];

pub fn binary_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Which built-in provider (or custom) a run resolves to.
#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub name: ProviderName,
    pub custom_command: Option<String>,
}

impl ResolvedProvider {
    pub fn check_available(&self) -> bool {
        match self.name {
            ProviderName::Custom => true,
            other => binary_exists(other.as_str()),
        }
    }

    pub fn run(&self, options: &AgentRunOptions<'_>) -> Result<AgentRunResult, String> {
        let mut args: Vec<String> = Vec::new();
        let binary: String;
        let mut parse_mode = ParseMode::None;
        let mut shell_wrap: Option<String> = None;

        match self.name {
            ProviderName::Pi => {
                binary = "pi".into();
                args.push("-p".into());
                args.push(options.prompt.to_string());
                if let Some(model) = options.model {
                    args.push("--model".into());
                    args.push(model.to_string());
                }
                parse_mode = ParseMode::LastLine;
            }
            ProviderName::Claude => {
                binary = "claude".into();
                args.push("-p".into());
                args.push(options.prompt.to_string());
                args.push("--output-format".into());
                args.push("json".into());
                args.push("--permission-mode".into());
                args.push("acceptEdits".into());
                if let Some(model) = options.model {
                    args.push("--model".into());
                    args.push(model.to_string());
                }
                parse_mode = ParseMode::WholeObject;
            }
            ProviderName::Aider => {
                binary = "aider".into();
                args.push("--message".into());
                args.push(options.prompt.to_string());
                args.push("--yes".into());
                if let Some(model) = options.model {
                    args.push("--model".into());
                    args.push(model.to_string());
                }
            }
            ProviderName::Codex => {
                binary = "codex".into();
                args.push("exec".into());
                args.push("--json".into());
                args.push(options.prompt.to_string());
                if let Some(model) = options.model {
                    args.push("--model".into());
                    args.push(model.to_string());
                }
                parse_mode = ParseMode::LastLine;
            }
            ProviderName::Opencode => {
                binary = "opencode".into();
                args.push("run".into());
                args.push(options.prompt.to_string());
                if let Some(model) = options.model {
                    args.push("--model".into());
                    args.push(model.to_string());
                }
            }
            ProviderName::Ka => {
                binary = "ka".into();
                args.push("run".into());
                args.push("--mode".into());
                args.push("guarded".into());
                if let Some(model) = options.model {
                    args.push("--model".into());
                    args.push(model.to_string());
                }
                args.push(options.prompt.to_string());
                parse_mode = ParseMode::LastLine;
            }
            ProviderName::Omp => {
                binary = "omp".into();
                args.push("-p".into());
                args.push(options.prompt.to_string());
                args.push("--mode".into());
                args.push("json".into());
                if let Some(model) = options.model {
                    args.push("--model".into());
                    args.push(model.to_string());
                }
                parse_mode = ParseMode::LastLine;
            }
            ProviderName::Custom => {
                let command = self.custom_command.clone().unwrap_or_default();
                let mut shell_command = format!("{command} \"$OPENLOOP_PROMPT\"");
                if let Some(extra) = options.extra_args {
                    for arg in extra {
                        shell_command.push(' ');
                        shell_command.push_str(&shell_quote_posix(arg));
                    }
                }
                binary = "sh".into();
                args.push("-c".into());
                args.push(shell_command.clone());
                shell_wrap = Some(shell_command);
            }
        }

        if self.name != ProviderName::Custom {
            if let Some(extra) = options.extra_args {
                args.extend(extra.iter().cloned());
            }
        }

        let raw = spawn_and_wait(
            &binary,
            &args,
            options.project_path,
            SpawnOptions {
                timeout_ms: options.timeout_ms,
                project_alias: options.project_alias.map(|s| s.to_string()),
                env_prompt: if shell_wrap.is_some() {
                    Some(options.prompt.to_string())
                } else {
                    None
                },
                env_model: if shell_wrap.is_some() {
                    Some(options.model.unwrap_or("").to_string())
                } else {
                    None
                },
            },
        );

        with_usage(raw, parse_mode)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ParseMode {
    None,
    LastLine,
    WholeObject,
}

fn shell_quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub struct SpawnOptions {
    pub timeout_ms: Option<u64>,
    pub project_alias: Option<String>,
    pub env_prompt: Option<String>,
    pub env_model: Option<String>,
}

struct RawRunResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

const CAPTURE_LIMIT: usize = 4 * 1024 * 1024;
// Trim in quanta: only rebuild the buffer once it drifts this far past the
// cap, so a chatty agent costs one ~4 MiB copy per quantum instead of one
// per 16 KiB chunk (~256x fewer memcpys).
const CAPTURE_TRIM_QUANTUM: usize = 256 * 1024;

fn append_capped(buffer: &Arc<Mutex<String>>, text: &str) {
    let mut guard = buffer.lock();
    guard.push_str(text);
    if guard.len() > CAPTURE_LIMIT + CAPTURE_TRIM_QUANTUM {
        // Keep the tail; avoid slicing mid-UTF-8 boundary.
        let mut start = guard.len() - CAPTURE_LIMIT;
        while start < guard.len() && !guard.is_char_boundary(start) {
            start += 1;
        }
        let tail = guard[start..].to_string();
        *guard = tail;
    }
}

/// Spawn a detached (own process group) subprocess, capture piped output with
/// a rolling 4 MiB cap per stream, and enforce the wall-clock timeout by
/// killing the whole process group (SIGTERM, escalating to SIGKILL).
fn spawn_and_wait(
    binary: &str,
    args: &[String],
    cwd: &Path,
    options: SpawnOptions,
) -> Result<RawRunResult, String> {
    use std::os::unix::process::CommandExt;

    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // detached: the agent gets its own process group so a timeout kill
        // reaches the whole subprocess tree, not just the launcher binary.
        .process_group(0);

    if options.env_prompt.is_some() || options.env_model.is_some() {
        if let Some(prompt) = &options.env_prompt {
            command.env("OPENLOOP_PROMPT", prompt);
        }
        if let Some(model) = &options.env_model {
            command.env("OPENLOOP_MODEL", model);
        }
    }

    let mut child = command.spawn().map_err(|e| format!("{binary}: {e}"))?;
    let child_pid = child.id() as i32;
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    let stdout_buf = Arc::new(Mutex::new(String::new()));
    let stderr_buf = Arc::new(Mutex::new(String::new()));

    fn spawn_reader<R: std::io::Read + Send + 'static>(
        handle: Option<R>,
        buf: Arc<Mutex<String>>,
        eof: std::sync::mpsc::Sender<()>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut handle = handle;
            let Some(handle) = handle.as_mut() else {
                let _ = eof.send(());
                return;
            };
            let mut chunk = [0u8; 16384];
            loop {
                match handle.read(&mut chunk) {
                    Ok(0) | Err(_) => {
                        let _ = eof.send(());
                        break;
                    }
                    Ok(n) => append_capped(&buf, &String::from_utf8_lossy(&chunk[..n])),
                }
            }
        })
    }
    let (eof_tx_out, rx_out) = std::sync::mpsc::channel::<()>();
    let (eof_tx_err, rx_err) = std::sync::mpsc::channel::<()>();
    let readers: Vec<(std::thread::JoinHandle<()>, std::sync::mpsc::Receiver<()>)> = vec![
        (
            spawn_reader(stdout_handle, stdout_buf.clone(), eof_tx_out),
            rx_out,
        ),
        (
            spawn_reader(stderr_handle, stderr_buf.clone(), eof_tx_err),
            rx_err,
        ),
    ];

    let spawn_started = Instant::now();
    let unregister = options
        .project_alias
        .as_deref()
        .map(|alias| active_runs::register_active_run(alias, child_pid));

    let deadline = options
        .timeout_ms
        .map(|ms| Instant::now() + Duration::from_millis(ms));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Drain until pipe EOF (TS 'close' semantics: capture all
                // output, including grandchildren writing after the child
                // exits) — but BOUNDED: TS kept its timeout armed until
                // 'close', so a hung grandchild holding the pipe open can
                // only consume the remaining budget, never park the daemon
                // thread forever.
                let drain_cap = match options.timeout_ms {
                    Some(ms) => {
                        let elapsed_ms = spawn_started.elapsed().as_millis() as u64;
                        Duration::from_millis(ms.saturating_sub(elapsed_ms))
                    }
                    None => Duration::from_secs(5),
                }
                .max(Duration::from_millis(100));
                let drain_started = Instant::now();
                let mut drained = true;
                for (_, rx) in readers.iter() {
                    let left = drain_cap.saturating_sub(drain_started.elapsed());
                    match rx.recv_timeout(left) {
                        Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            drained = false;
                            break;
                        }
                    }
                    // The try_wait loop below cannot see shutdown once the
                    // child is gone; honor the flag here so the daemon can
                    // still exit promptly.
                    if super::worker::SHUTDOWN.load(std::sync::atomic::Ordering::SeqCst) {
                        drained = false;
                        break;
                    }
                }
                if !drained {
                    // Close late pipes the hard way (kills in-group
                    // grandchildren), then give stragglers one second to
                    // observe EOF.
                    kill_process_group(child_pid, libc::SIGKILL);
                    let kill_started = Instant::now();
                    for (_, rx) in readers.iter() {
                        let _ = rx.recv_timeout(
                            Duration::from_secs(1).saturating_sub(kill_started.elapsed()),
                        );
                    }
                }
                drop(readers);
                if let Some(id) = unregister {
                    active_runs::unregister_active_run(id);
                }
                return Ok(RawRunResult {
                    exit_code: status.code().unwrap_or(1),
                    stdout: stdout_buf.lock().clone(),
                    stderr: stderr_buf.lock().clone(),
                });
            }
            Ok(None) => {
                // Shutdown flag: daemon SIGTERM/SIGINT must interrupt an active
                // agent run promptly (TS parity: killActiveRuns + immediate exit).
                if super::worker::SHUTDOWN.load(std::sync::atomic::Ordering::SeqCst) {
                    kill_process_group(child_pid, libc::SIGTERM);
                    escalate_and_reap(child, child_pid);
                    if let Some(id) = unregister {
                        active_runs::unregister_active_run(id);
                    }
                    return Err("__shutdown__:".to_string());
                }
                if let Some(deadline) = deadline {
                    if Instant::now() >= deadline {
                        kill_process_group(child_pid, libc::SIGTERM);
                        // Escalate after a grace period and reap the zombie
                        // (std does not auto-reap children like libuv does).
                        escalate_and_reap(child, child_pid);
                        if let Some(id) = unregister {
                            active_runs::unregister_active_run(id);
                        }
                        return Err(format!("__timeout__:{}", options.timeout_ms.unwrap_or(0)));
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                if let Some(id) = unregister {
                    active_runs::unregister_active_run(id);
                }
                return Err(e.to_string());
            }
        }
    }
}

/// SIGKILL the process group after a grace period and reap the child so no
/// zombie is left behind. Owns the Child; blocks in its own thread.
fn escalate_and_reap(mut child: std::process::Child, child_pid: i32) {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(5000);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return, // already reaped
                Ok(None) => {
                    if Instant::now() >= deadline {
                        kill_process_group(child_pid, libc::SIGKILL);
                        let _ = child.wait(); // final reap
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return,
            }
        }
    });
}

pub fn kill_process_group(pgid: i32, signal: i32) {
    unsafe {
        if libc::kill(-pgid, signal) != 0 {
            let _ = libc::kill(pgid, signal);
        }
    }
}

/// Map spawn_and_wait's timeout sentinel into a RunTimeoutError at the
/// provider boundary; every other error propagates as-is.
fn with_usage(
    raw: Result<RawRunResult, String>,
    parse_mode: ParseMode,
) -> Result<AgentRunResult, String> {
    let raw = match raw {
        Ok(r) => r,
        Err(e) => {
            if e.starts_with("__shutdown__:") {
                // Shutdown must surface as a hard error so the tick unwinds
                // and the daemon loop observes the flag immediately.
                return Err(e);
            }
            if let Some(ms) = e.strip_prefix("__timeout__:") {
                let ms: u64 = ms.parse().unwrap_or(0);
                // Encode the timeout for callers through a convention: exit
                // code -1 with empty output. Handled by run_agent wrapper.
                return Ok(AgentRunResult {
                    exit_code: if ms == u64::MAX { 0 } else { -1 },
                    stdout: format!("__run_timeout__:{ms}"),
                    stderr: String::new(),
                    usage: None,
                });
            }
            return Ok(AgentRunResult {
                exit_code: -1,
                stdout: e,
                stderr: String::new(),
                usage: None,
            });
        }
    };

    let usage = match parse_mode {
        ParseMode::None => None,
        ParseMode::WholeObject => serde_json::from_str::<serde_json::Value>(&raw.stdout)
            .ok()
            .and_then(|v| extract_usage(&v)),
        ParseMode::LastLine => raw
            .stdout
            .trim()
            .lines()
            .last()
            .and_then(|last| serde_json::from_str::<serde_json::Value>(last).ok())
            .and_then(|v| extract_usage(&v)),
    };

    Ok(AgentRunResult {
        exit_code: raw.exit_code,
        stdout: raw.stdout,
        stderr: raw.stderr,
        usage,
    })
}

fn to_finite(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|v| v.as_f64()).filter(|n| n.is_finite())
}

pub fn extract_usage(obj: &serde_json::Value) -> Option<AgentUsage> {
    let record = obj.as_object()?;
    let nested = record.get("usage").filter(|v| v.is_object());
    let get_num = |container: Option<&serde_json::Value>, keys: &[&str]| -> Option<f64> {
        for key in keys {
            if let Some(v) = container.and_then(|c| c.get(*key)) {
                if let Some(n) = to_finite(Some(v)) {
                    return Some(n);
                }
            }
        }
        None
    };
    let usage = AgentUsage {
        input_tokens: get_num(nested, &["input_tokens", "inputTokens"]),
        output_tokens: get_num(nested, &["output_tokens", "outputTokens"]),
        total_tokens: get_num(nested, &["total_tokens", "totalTokens"])
            .or_else(|| get_num(Some(obj), &["total_tokens"])),
        cost_usd: get_num(Some(obj), &["total_cost_usd", "cost_usd"])
            .or_else(|| get_num(nested, &["total_cost_usd", "cost_usd"])),
    };
    let any = usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.total_tokens.is_some()
        || usage.cost_usd.is_some();
    if any {
        Some(usage)
    } else {
        None
    }
}

/// Resolve the provider for a given project configuration and optional custom
/// command. Falls back to the supplied default (usually from global config).
pub fn resolve_provider(
    agent_type: Option<&str>,
    custom_command: Option<&str>,
    default_provider: Option<&str>,
) -> ResolvedProvider {
    let name = agent_type.or(default_provider).unwrap_or("pi").to_string();

    if name == "custom" {
        // TS truthiness: an empty-string command falls through to pi.
        if let Some(command) = custom_command.filter(|c| !c.is_empty()) {
            return ResolvedProvider {
                name: ProviderName::Custom,
                custom_command: Some(command.to_string()),
            };
        }
        // TS parity: resolveProvider fell back to the pi provider when the
        // type was "custom" but no command was configured (its provider map
        // has no "custom" entry). Spawning `sh -c` with the prompt as the
        // command name would just fail with exit 127.
    }

    let effective_name = if name == "custom" {
        "pi"
    } else {
        name.as_str()
    };
    ResolvedProvider {
        name: ProviderName::parse(effective_name).unwrap_or(ProviderName::Pi),
        custom_command: None,
    }
}

pub fn _unused(_p: PathBuf) {}
