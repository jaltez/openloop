//! Integration tests for provider argv construction and process handling:
//! resolve_provider dispatch per agent type, exit-code passthrough, capture of
//! late output written by grandchildren outliving the direct child, run
//! timeouts, and the availability gate.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use openloop::core::pi::{assert_provider_available, is_timeout_error, run_agent, PiRunOptions};
use openloop::core::providers::{resolve_provider, AgentRunOptions, ProviderName, PROVIDER_NAMES};
use openloop::core::types::{
    AgentConfig, LinkedProject, PiConfig, ProjectConfig, ProjectMeta, ProjectRisk, ProjectRuntime,
    ProjectValidation,
};

// PATH mutations are process-global and cargo test runs these tests in
// threads: every test that spawns a provider (or checks availability) holds
// this lock for its whole body.
static PATH_LOCK: Mutex<()> = Mutex::new(());

struct PathScope {
    saved: String,
    _guard: MutexGuard<'static, ()>,
}

impl Drop for PathScope {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.saved);
    }
}

/// Prepend `dir` to PATH for the lifetime of the returned scope.
fn prepend_to_path(dir: &Path) -> PathScope {
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let saved = std::env::var("PATH").unwrap_or_default();
    let dir = dir.to_string_lossy();
    std::env::set_var("PATH", format!("{dir}:{saved}"));
    PathScope { saved, _guard }
}

/// Replace PATH with `dir` alone (for "binary not found on PATH" cases).
fn replace_path(dir: &Path) -> PathScope {
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let saved = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", dir);
    PathScope { saved, _guard }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("ol-providers-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

/// Write a fake provider binary (a shell script) into `dir`.
fn fake_bin(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    make_executable(&path);
}

fn run_options<'a>(prompt: &'a str, project_path: &'a Path) -> AgentRunOptions<'a> {
    AgentRunOptions {
        prompt,
        model: None,
        project_path,
        project_alias: None,
        timeout_ms: None,
        extra_args: None,
    }
}

fn fake_project(path: &Path) -> LinkedProject {
    LinkedProject {
        alias: "test".into(),
        path: path.to_string_lossy().to_string(),
        default_branch: None,
        initialized: true,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    }
}

fn agent_config(agent_type: &str, command: Option<&str>) -> AgentConfig {
    AgentConfig {
        agent_type: Some(agent_type.into()),
        command: command.map(str::to_string),
        extra_args: None,
    }
}

fn base_config(agent: Option<AgentConfig>) -> ProjectConfig {
    ProjectConfig {
        version: 1,
        project: ProjectMeta {
            alias: Some("test".into()),
            repo_root: None,
            initialized_at: None,
        },
        pi: PiConfig {
            model: None,
            prompt_files: vec![],
        },
        agent,
        runtime: ProjectRuntime {
            use_worktree: false,
            branch_prefix: "openloop/".into(),
            pr_command: None,
            worktree_setup_command: None,
        },
        validation: ProjectValidation {
            lint_command: None,
            test_command: None,
            typecheck_command: None,
        },
        risk: ProjectRisk {
            default_unknown_area_classification: "medium-risk".into(),
            require_policy_for_auto_merge: true,
        },
        issue_source: None,
        hooks: None,
        schedule: None,
        schedule_state: None,
        review: None,
        verification: None,
    }
}

// --- Provider registry ---

#[test]
fn provider_names_cover_every_builtin() {
    let names: Vec<&str> = PROVIDER_NAMES.iter().map(|p| p.as_str()).collect();
    for expected in ["pi", "claude", "aider", "codex", "opencode", "ka", "omp"] {
        assert!(
            names.contains(&expected),
            "missing builtin {expected} in {names:?}"
        );
    }
    assert_eq!(names.len(), 7);
}

#[test]
fn builtin_provider_labels_match_registry() {
    assert_eq!(ProviderName::parse("pi").unwrap().label(), "Pi");
    assert_eq!(
        ProviderName::parse("claude").unwrap().label(),
        "Claude Code"
    );
    assert_eq!(
        ProviderName::parse("codex").unwrap().label(),
        "OpenAI Codex"
    );
    assert_eq!(ProviderName::parse("opencode").unwrap().label(), "OpenCode");
    assert_eq!(ProviderName::parse("ka").unwrap().label(), "ka");
    assert_eq!(ProviderName::parse("omp").unwrap().label(), "Oh My Pi");
}

#[test]
fn unknown_provider_name_fails_to_parse() {
    assert!(ProviderName::parse("nonexistent").is_none());
}

// --- resolve_provider ---

#[test]
fn resolve_provider_falls_back_to_pi_when_no_type_specified() {
    let provider = resolve_provider(None, None, None);
    assert_eq!(provider.name, ProviderName::Pi);
}

#[test]
fn resolve_provider_uses_specified_agent_type() {
    let provider = resolve_provider(Some("claude"), None, None);
    assert_eq!(provider.name, ProviderName::Claude);
}

#[test]
fn resolve_provider_uses_default_provider_when_agent_type_is_undefined() {
    let provider = resolve_provider(None, None, Some("aider"));
    assert_eq!(provider.name, ProviderName::Aider);
}

#[test]
fn resolve_provider_creates_custom_provider_when_type_is_custom() {
    let provider = resolve_provider(Some("custom"), Some("my-script.sh"), None);
    assert_eq!(provider.name, ProviderName::Custom);
    assert_eq!(provider.custom_command.as_deref(), Some("my-script.sh"));
}

#[test]
fn resolve_provider_custom_with_empty_command_falls_back_to_pi() {
    // TS truthiness parity: an empty custom command resolves to pi.
    let provider = resolve_provider(Some("custom"), Some(""), None);
    assert_eq!(provider.name, ProviderName::Pi);
}

#[test]
fn custom_provider_is_always_available() {
    let provider = resolve_provider(Some("custom"), Some("echo hello"), None);
    assert!(provider.check_available());
}

// --- argv construction per provider ---

#[test]
fn ka_provider_runs_guarded_mode_with_prompt_then_extra_args_last() {
    let dir = TempDir::new("ka-argv");
    fake_bin(dir.path(), "ka", "#!/bin/sh\necho \"$@\"\n");
    let _path = prepend_to_path(dir.path());

    let provider = resolve_provider(Some("ka"), None, None);
    assert!(provider.check_available());

    let extra = ["--trust".to_string()];
    let mut options = run_options("do the thing", dir.path());
    options.model = Some("test-model");
    options.extra_args = Some(&extra);

    let result = provider.run(&options).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.stdout.trim(),
        "run --mode guarded --model test-model do the thing --trust"
    );
}

#[test]
fn omp_provider_runs_print_mode_with_json_event_stream() {
    let dir = TempDir::new("omp-argv");
    fake_bin(
        dir.path(),
        "omp",
        concat!(
            "#!/bin/sh\n",
            "echo \"$@\"\n",
            "echo '{\"usage\":{\"input_tokens\":7,\"output_tokens\":3},\"total_cost_usd\":0.11}'\n",
        ),
    );
    let _path = prepend_to_path(dir.path());

    let provider = resolve_provider(Some("omp"), None, None);
    let mut options = run_options("hello", dir.path());
    options.model = Some("test-model");

    let result = provider.run(&options).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.stdout.lines().next(),
        Some("-p hello --mode json --model test-model")
    );
    let usage = result.usage.expect("usage parsed from the last JSON line");
    assert_eq!(usage.input_tokens, Some(7.0));
    assert_eq!(usage.output_tokens, Some(3.0));
    assert_eq!(usage.cost_usd, Some(0.11));
}

#[test]
fn pi_provider_passes_prompt_then_model() {
    let dir = TempDir::new("pi-argv");
    fake_bin(dir.path(), "pi", "#!/bin/sh\necho \"$@\"\n");
    let _path = prepend_to_path(dir.path());

    let provider = resolve_provider(Some("pi"), None, None);
    let mut options = run_options("do the thing", dir.path());
    options.model = Some("test-model");

    let result = provider.run(&options).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "-p do the thing --model test-model");
}

#[test]
fn claude_provider_parses_usage_and_cost_from_json_stdout() {
    let dir = TempDir::new("claude-usage");
    fake_bin(
        dir.path(),
        "claude",
        concat!(
            "#!/bin/sh\n",
            "echo '{\"type\":\"result\",\"total_cost_usd\":0.42,",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}'\n",
        ),
    );
    let _path = prepend_to_path(dir.path());

    let provider = resolve_provider(Some("claude"), None, None);
    let result = provider.run(&run_options("hello", dir.path())).unwrap();
    assert_eq!(result.exit_code, 0);
    let usage = result.usage.expect("usage parsed from whole-object stdout");
    assert_eq!(usage.input_tokens, Some(10.0));
    assert_eq!(usage.output_tokens, Some(5.0));
    assert_eq!(usage.cost_usd, Some(0.42));
}

#[test]
fn provider_emitting_garbage_stdout_reports_no_usage() {
    let dir = TempDir::new("claude-garbage");
    fake_bin(dir.path(), "claude", "#!/bin/sh\necho 'not json at all'\n");
    let _path = prepend_to_path(dir.path());

    let provider = resolve_provider(Some("claude"), None, None);
    let result = provider.run(&run_options("hello", dir.path())).unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.usage.is_none());
    assert!(result.stdout.contains("not json at all"));
}

#[test]
fn custom_provider_runs_shell_wrapped_command_with_prompt_env() {
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = TempDir::new("custom-env");
    fake_bin(
        dir.path(),
        "custom-agent",
        concat!(
            "#!/bin/sh\n",
            "echo \"prompt=$OPENLOOP_PROMPT\"\n",
            "echo \"model=$OPENLOOP_MODEL\"\n",
        ),
    );
    // No PATH mutation needed: the custom command is an absolute path run
    // through `sh -c '<cmd> \"$OPENLOOP_PROMPT\"'`.

    let command = dir
        .path()
        .join("custom-agent")
        .to_string_lossy()
        .to_string();
    let provider = resolve_provider(Some("custom"), Some(&command), None);
    let mut options = run_options("hello world", dir.path());
    options.model = Some("test-model");

    let result = provider.run(&options).unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "prompt=hello world\nmodel=test-model");
}

// --- process handling ---

#[test]
fn provider_exit_code_passes_through() {
    let dir = TempDir::new("exit-code");
    fake_bin(dir.path(), "pi", "#!/bin/sh\nexit 42\n");
    let _path = prepend_to_path(dir.path());

    let provider = resolve_provider(Some("pi"), None, None);
    let result = provider.run(&run_options("hello", dir.path())).unwrap();
    assert_eq!(result.exit_code, 42);
}

#[test]
fn custom_provider_captures_output_written_after_direct_child_exits() {
    let _guard = PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = TempDir::new("late-output");
    let config = base_config(Some(agent_config(
        "custom",
        Some("(sleep 0.3; echo late-line) & true"),
    )));
    let project = fake_project(dir.path());

    // The backgrounded subshell keeps the stdout pipe open after the direct
    // child exits; settling on child exit would truncate the late line.
    let result = run_agent(
        &PiRunOptions {
            prompt: "hello",
            model: None,
            project: &project,
            timeout_ms: None,
        },
        Some(&config),
        None,
    )
    .unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("late-line"),
        "late grandchild output not captured: {:?}",
        result.stdout
    );
}

#[test]
fn run_timeout_produces_sentinel_result() {
    let dir = TempDir::new("timeout");
    fake_bin(dir.path(), "pi", "#!/bin/sh\nsleep 30\n");
    let _path = prepend_to_path(dir.path());

    let provider = resolve_provider(Some("pi"), None, None);
    let mut options = run_options("hello", dir.path());
    options.timeout_ms = Some(150);

    let result = provider.run(&options).unwrap();
    assert_eq!(result.exit_code, -1);
    assert_eq!(result.stdout, "__run_timeout__:150");
}

#[test]
fn run_agent_translates_timeout_sentinel_to_timeout_error() {
    let dir = TempDir::new("timeout-agent");
    fake_bin(dir.path(), "pi", "#!/bin/sh\nsleep 30\n");
    let _path = prepend_to_path(dir.path());

    let config = base_config(Some(agent_config("pi", None)));
    let project = fake_project(dir.path());
    let error = run_agent(
        &PiRunOptions {
            prompt: "hello",
            model: None,
            project: &project,
            timeout_ms: Some(150),
        },
        Some(&config),
        None,
    )
    .unwrap_err();

    assert_eq!(error, "__timeout__:150");
    assert!(is_timeout_error(&error));
}

// --- run_agent dispatch ---

#[test]
fn run_agent_dispatches_to_configured_provider_binary() {
    let dir = TempDir::new("dispatch-claude");
    fake_bin(dir.path(), "claude", "#!/bin/sh\nexit 0\n");
    let _path = prepend_to_path(dir.path());

    let config = base_config(Some(agent_config("claude", None)));
    let project = fake_project(dir.path());
    let result = run_agent(
        &PiRunOptions {
            prompt: "hello",
            model: None,
            project: &project,
            timeout_ms: None,
        },
        Some(&config),
        None,
    )
    .unwrap();
    assert_eq!(result.exit_code, 0);
}

#[test]
fn run_agent_uses_default_provider_when_project_has_no_agent_config() {
    let dir = TempDir::new("dispatch-aider");
    fake_bin(dir.path(), "aider", "#!/bin/sh\nexit 0\n");
    let _path = prepend_to_path(dir.path());

    let config = base_config(None);
    let project = fake_project(dir.path());
    let result = run_agent(
        &PiRunOptions {
            prompt: "hello",
            model: None,
            project: &project,
            timeout_ms: None,
        },
        Some(&config),
        Some("aider"),
    )
    .unwrap();
    assert_eq!(result.exit_code, 0);
}

// --- availability gate ---

#[test]
fn assert_provider_available_rejects_missing_binary_with_message() {
    let dir = TempDir::new("gate-missing");
    let _path = replace_path(dir.path());

    // No agent config + no default resolves to pi, which is not on the
    // stubbed PATH.
    let error = assert_provider_available(None, None).unwrap_err();
    assert_eq!(error, "'pi' binary not found on PATH");
}

#[test]
fn assert_provider_available_allows_custom_without_path_lookup() {
    let dir = TempDir::new("gate-custom");
    let _path = replace_path(dir.path());

    // Custom providers always shell out; availability never consults PATH.
    let config = base_config(Some(agent_config("custom", Some("echo hi"))));
    assert_provider_available(Some(&config), None).unwrap();
}

#[test]
fn assert_provider_available_passes_when_binary_on_path() {
    let dir = TempDir::new("gate-present");
    fake_bin(dir.path(), "pi", "#!/bin/sh\nexit 0\n");
    let _path = prepend_to_path(dir.path());

    assert_provider_available(None, None).unwrap();
}
