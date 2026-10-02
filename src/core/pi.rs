use std::path::Path;

use super::providers::{self, AgentRunOptions, ResolvedProvider};
use super::types::{AgentRunResult, LinkedProject, ProjectConfig};

pub struct PiRunOptions<'a> {
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub project: &'a LinkedProject,
    pub timeout_ms: Option<u64>,
}

/// Gate a command on the availability of the resolved provider's binary.
/// Replaces the Pi-only gate: whichever agent the project config resolves to
/// (claude, codex, custom, …) must be on PATH before the command proceeds.
pub fn assert_provider_available(
    project_config: Option<&ProjectConfig>,
    default_provider: Option<&str>,
) -> Result<(), String> {
    let provider = providers::resolve_provider(
        project_config
            .and_then(|c| c.agent.as_ref())
            .and_then(|a| a.agent_type.as_deref()),
        project_config
            .and_then(|c| c.agent.as_ref())
            .and_then(|a| a.command.as_deref()),
        default_provider,
    );
    if !provider.check_available() {
        return Err(format!(
            "'{}' binary not found on PATH",
            provider.name.as_str()
        ));
    }
    Ok(())
}

pub fn run_pi(options: &PiRunOptions<'_>) -> Result<AgentRunResult, String> {
    let provider: ResolvedProvider = providers::resolve_provider(Some("pi"), None, None);
    provider.run(&AgentRunOptions {
        prompt: options.prompt,
        model: options.model,
        project_path: Path::new(&options.project.path),
        project_alias: Some(&options.project.alias),
        timeout_ms: options.timeout_ms,
        extra_args: None,
    })
}

/// D6 / A1: Model-agnostic agent runner dispatching to the configured provider.
/// Timeout is signalled through the `__run_timeout__:<ms>` stdout sentinel.
pub fn run_agent(
    options: &PiRunOptions<'_>,
    project_config: Option<&ProjectConfig>,
    default_provider: Option<&str>,
) -> Result<AgentRunResult, String> {
    let provider = providers::resolve_provider(
        project_config
            .and_then(|c| c.agent.as_ref())
            .and_then(|a| a.agent_type.as_deref()),
        project_config
            .and_then(|c| c.agent.as_ref())
            .and_then(|a| a.command.as_deref()),
        default_provider,
    );
    let result = provider.run(&AgentRunOptions {
        prompt: options.prompt,
        model: options.model,
        project_path: Path::new(&options.project.path),
        project_alias: Some(&options.project.alias),
        timeout_ms: options.timeout_ms,
        extra_args: project_config
            .and_then(|c| c.agent.as_ref())
            .and_then(|a| a.extra_args.as_deref()),
    })?;
    // Translate the timeout sentinel into an Err(RunTimeoutError) signal.
    if let Some(rest) = result.stdout.strip_prefix("__run_timeout__") {
        let ms = rest.trim().parse::<u64>().unwrap_or(0);
        return Err(format!("__timeout__:{ms}"));
    }
    Ok(result)
}

/// True when the spawn-level error string encodes a run timeout.
pub fn is_timeout_error(error: &str) -> bool {
    error.starts_with("__timeout__:")
}

pub fn timeout_message(error: &str) -> String {
    let ms = error
        .strip_prefix("__timeout__:")
        .unwrap_or("0")
        .parse::<u64>()
        .unwrap_or(0);
    format!("Agent run exceeded timeout of {ms}ms.")
}
