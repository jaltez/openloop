use std::path::{Path, PathBuf};

use super::dispatch2::*;
use super::*;
use crate::core as c;
use c::types::*;

pub fn dispatch(command: Command) -> Result<(), String> {
    match command {
        Command::Project(cmd) => project(cmd),
        Command::Promotion(cmd) => promotion(cmd),
        Command::Config(cmd) => config(cmd),
        Command::Service(cmd) => service(cmd),
        Command::Logs(args) => logs(args),
        Command::Run(args) => run(args),
        Command::RunOnce(args) => run_once(args),
        Command::Enqueue(args) => enqueue(args),
        Command::Task(cmd) => task(cmd),
        Command::Approval(cmd) => approval(cmd),
        Command::Status(args) => status(args),
        Command::Events(args) => events(args),
        Command::Watch => watch(),
        Command::Doctor(args) => doctor(args),
        Command::Report(args) => report(args),
        Command::Digest(args) => digest(args),
        Command::Setup => setup(),
        Command::Issue(cmd) => issue(cmd),
        Command::Dashboard(cmd) => dashboard(cmd),
        Command::Mcp => c::mcp_server::run_mcp_server(),
        Command::Daemon(cmd) => match cmd.command {
            super::DaemonSubcommand::Worker => c::worker::start_worker_loop(false),
        },
    }
}

fn package_root() -> PathBuf {
    // Templates are embedded in the binary; repo root is only needed for
    // template extraction parity (not used at runtime).
    PathBuf::from(".")
}

// ---------------------------------------------------------------------------
// project
// ---------------------------------------------------------------------------

fn project(cmd: ProjectCommand) -> Result<(), String> {
    match cmd.command {
        ProjectSubcommand::Add {
            alias,
            project_path,
            init,
            force,
        } => {
            // TS path.resolve parity: absolute + lexically normalized
            // (drops `..`, `.`, and trailing slashes before persisting).
            let resolved = std::path::absolute(&project_path)
                .map(|p| normalize_lexical(&p).to_string_lossy().to_string())
                .unwrap_or_else(|_| project_path.clone());
            if c::project_registry::project_exists(&alias, None)? {
                if !force {
                    let existing = c::project_registry::get_project(&alias, None)?;
                    eprintln!(
                        "Error: Project alias '{alias}' already exists (path: {}). Use '--force' to update.",
                        existing.path
                    );
                    std::process::exit(1);
                }
                let project = c::project_registry::update_project_path(&alias, &resolved, None)?;
                println!("Updated project {} -> {}", project.alias, project.path);
                return Ok(());
            }
            let project = c::project_registry::add_project(&alias, &resolved, None)?;
            println!("Registered project {} -> {}", project.alias, project.path);
            if init {
                c::templates::initialize_project_from_templates(&package_root(), &project, false)?;
                c::project_registry::mark_project_initialized(&alias, None)?;
                let project_config =
                    c::project_config::load_project_config(Path::new(&project.path))?;
                println!("Initialized templates for {}", project.alias);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "lintCommand": project_config.validation.lint_command,
                        "testCommand": project_config.validation.test_command,
                        "typecheckCommand": project_config.validation.typecheck_command,
                    }))
                    .unwrap_or_default()
                );
            }
            Ok(())
        }
        ProjectSubcommand::List { format } => {
            let projects = c::project_registry::list_projects(None)?;
            match c::table::resolve_output_format(format.as_deref()) {
                c::table::OutputFormat::Table => {
                    let rows: Vec<c::table::Row> = projects
                        .iter()
                        .map(|p| {
                            c::table::Row(vec![
                                p.alias.clone(),
                                p.initialized.to_string(),
                                p.path.clone(),
                            ])
                        })
                        .collect();
                    c::table::print_table(&["alias", "initialized", "path"], &rows);
                }
                c::table::OutputFormat::Json => {
                    for project in projects {
                        println!(
                            "{}\t{}\tinitialized={}",
                            project.alias, project.path, project.initialized
                        );
                    }
                }
            }
            Ok(())
        }
        ProjectSubcommand::Activate { alias } => {
            let project = c::project_registry::get_project(&alias, None)?;
            let mut config = c::global_config::load_global_config(None)?;
            config.active_project_alias = Some(project.alias.clone());
            c::global_config::save_global_config(&config, None)?;
            println!("Active project set to {}", project.alias);
            Ok(())
        }
        ProjectSubcommand::Show { alias } => {
            let project = c::project_registry::get_project(&alias, None)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&project).unwrap_or_default()
            );
            Ok(())
        }
        ProjectSubcommand::Init { alias, force } => {
            let project = c::project_registry::get_project(&alias, None)?;
            c::templates::initialize_project_from_templates(&package_root(), &project, force)?;
            c::project_registry::mark_project_initialized(&alias, None)?;
            if force {
                println!("Re-initialized {alias} (existing files backed up to .openloop/backup/)");
            } else {
                println!("Initialized {alias}");
            }
            let project_config = c::project_config::load_project_config(Path::new(&project.path))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "lintCommand": project_config.validation.lint_command,
                    "testCommand": project_config.validation.test_command,
                    "typecheckCommand": project_config.validation.typecheck_command,
                }))
                .unwrap_or_default()
            );
            Ok(())
        }
        ProjectSubcommand::Remove { alias } => {
            let project = c::project_registry::get_project(&alias, None)?;
            c::project_registry::remove_project(&project.alias, None)?;
            println!(
                "Removed project {}. Control-plane files in {} are preserved.",
                project.alias, project.path
            );
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// promotion
// ---------------------------------------------------------------------------

fn promotion(cmd: PromotionCommand) -> Result<(), String> {
    match cmd.command {
        PromotionSubcommand::List {
            project,
            task,
            status,
            format,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            let artifacts = match &task {
                Some(task_id) => c::promotion_queue::list_promotion_artifacts_for_task(
                    Path::new(&project.path),
                    task_id,
                )?,
                None => c::promotion_queue::list_promotion_artifacts(Path::new(&project.path))?,
            };
            let filtered: Vec<_> = artifacts
                .into_iter()
                .filter(|item| {
                    status
                        .as_deref()
                        .map(|s| item.artifact.status.as_str() == s)
                        .unwrap_or(true)
                })
                .collect();
            match c::table::resolve_output_format(format.as_deref()) {
                c::table::OutputFormat::Table => {
                    let rows: Vec<c::table::Row> = filtered
                        .iter()
                        .map(|item| {
                            c::table::Row(vec![
                                item.artifact.task_id.clone(),
                                item.artifact.decision.as_str().to_string(),
                                item.artifact.action.as_str().to_string(),
                                item.artifact.status.as_str().to_string(),
                            ])
                        })
                        .collect();
                    c::table::print_table(&["taskId", "decision", "action", "status"], &rows);
                }
                c::table::OutputFormat::Json => {
                    let mapped: Vec<serde_json::Value> = filtered
                        .iter()
                        .map(|item| {
                            serde_json::json!({
                                "taskId": item.artifact.task_id,
                                "decision": item.artifact.decision.as_str(),
                                "action": item.artifact.action.as_str(),
                                "status": item.artifact.status.as_str(),
                                "path": item.artifact_path,
                            })
                        })
                        .collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&mapped).unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        PromotionSubcommand::Show { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            let detail = c::promotion_queue::get_promotion_detail(Path::new(&project.path), &task)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&detail).unwrap_or_default()
            );
            Ok(())
        }
        PromotionSubcommand::History { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            let history =
                c::promotion_queue::get_promotion_history(Path::new(&project.path), &task)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&history).unwrap_or_default()
            );
            Ok(())
        }
        PromotionSubcommand::Apply {
            project,
            task,
            note,
            dry_run,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            if dry_run {
                let preview =
                    c::promotion_queue::dry_run_promotion_apply(Path::new(&project.path), &task)?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "wouldApply": preview.would_apply,
                        "action": preview.action,
                        "reason": preview.reason,
                    }))
                    .unwrap_or_default()
                );
                return Ok(());
            }
            let item = c::promotion_queue::apply_promotion_artifact(
                Path::new(&project.path),
                &task,
                note.as_deref(),
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "taskId": item.artifact.task_id,
                    "status": item.artifact.status.as_str(),
                    "path": item.artifact_path,
                    "note": item.artifact.note,
                }))
                .unwrap_or_default()
            );
            Ok(())
        }
        PromotionSubcommand::Reject {
            project,
            task,
            note,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            let item = c::promotion_queue::update_promotion_artifact(
                Path::new(&project.path),
                &task,
                PromotionArtifactStatus::Rejected,
                note.as_deref(),
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "taskId": item.artifact.task_id,
                    "status": item.artifact.status.as_str(),
                    "path": item.artifact_path,
                }))
                .unwrap_or_default()
            );
            Ok(())
        }
        PromotionSubcommand::Refresh { project, task } => {
            crate::cli::promotion_refresh::refresh(&project, &task)
        }
    }
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

fn parse_events(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn build_hook_config(
    hook_type: &str,
    events: &str,
    command: Option<&str>,
    url: Option<&str>,
    timeout_seconds: u64,
) -> Result<LifecycleHookConfig, String> {
    let events = parse_events(if events.is_empty() { "*" } else { events });
    if timeout_seconds < 1 {
        eprintln!("Error: --timeout-seconds must be a number >= 1");
        std::process::exit(1);
    }
    if hook_type == "command" {
        let Some(command) = command else {
            eprintln!("Error: --command is required for command hooks");
            std::process::exit(1);
        };
        return Ok(LifecycleHookConfig {
            hook_type: "command".to_string(),
            events,
            command: Some(command.to_string()),
            url: None,
            timeout_seconds: Some(timeout_seconds),
            disabled: None,
        });
    }
    let Some(url) = url else {
        eprintln!("Error: --url is required for webhook hooks");
        std::process::exit(1);
    };
    if url::is_valid(url) {
        Ok(LifecycleHookConfig {
            hook_type: "webhook".to_string(),
            events,
            command: None,
            url: Some(url.to_string()),
            timeout_seconds: Some(timeout_seconds),
            disabled: None,
        })
    } else {
        eprintln!("Error: --url must be a valid URL");
        std::process::exit(1);
    }
}

mod url {
    /// TS `new URL(url)` parity: any parseable absolute URL passes.
    pub fn is_valid(url: &str) -> bool {
        url::Url::parse(url).is_ok()
    }
}

fn format_hook(hook: &LifecycleHookConfig, index: usize) -> String {
    let target = hook
        .command
        .as_deref()
        .or(hook.url.as_deref())
        .unwrap_or("-");
    let events = if hook.events.is_empty() {
        "*".to_string()
    } else {
        hook.events.join(", ")
    };
    let timeout = hook.timeout_seconds.unwrap_or(10);
    format!(
        "[{index}] {} → {target} (events: {events}; timeout: {timeout}s)",
        hook.hook_type
    )
}

fn config(cmd: ConfigCommand) -> Result<(), String> {
    match cmd.command {
        ConfigSubcommand::Show => {
            let config = c::global_config::load_global_config(None)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&config).unwrap_or_default()
            );
            Ok(())
        }
        ConfigSubcommand::Set { key, value } => {
            const SETTABLE: [(&str, &str, Option<f64>, Option<f64>); 11] = [
                ("budgets.dailyCostUsd", "number", Some(0.0), None),
                ("budgets.estimatedCostPerRunUsd", "number", Some(0.0), None),
                ("runtime.runTimeoutSeconds", "number", Some(1.0), None),
                ("runtime.maxAttemptsPerTask", "number", Some(1.0), None),
                ("runtime.noProgressRepeatLimit", "number", Some(1.0), None),
                ("runtime.tickIntervalSeconds", "number", Some(1.0), None),
                ("runtime.projectSelectionStrategy", "string", None, None),
                ("notifications.onTaskComplete", "string", None, None),
                ("notifications.onTaskFailed", "string", None, None),
                ("notifications.onBudgetBlocked", "string", None, None),
                ("notifications.onAllTasksDone", "string", None, None),
            ];
            let Some(&(_key, type_, min, max)) = SETTABLE.iter().find(|(k, _, _, _)| *k == key)
            else {
                eprintln!("Error: '{key}' is not a settable config key.");
                let valid: Vec<&str> = SETTABLE.iter().map(|(k, _, _, _)| *k).collect();
                eprintln!("Valid keys: {}", valid.join(", "));
                std::process::exit(1);
            };
            let parsed: serde_json::Value =
                match super::coerce_config_set_value(type_, &value, min, max) {
                    Ok(parsed) => parsed,
                    Err(reason) => {
                        eprintln!("Error: {reason} for {key}");
                        std::process::exit(1);
                    }
                };
            // Integer-typed config fields (u64/u32) cannot round-trip a
            // fractional value; reject it cleanly instead of leaking the raw
            // serde error. budgets.* fields keep f64 and accept fractions.
            if parsed.is_f64() && !key.starts_with("budgets.") {
                eprintln!("Error: '{value}' is not a whole number for {key}");
                std::process::exit(1);
            }
            let mut raw = serde_json::to_value(c::global_config::load_global_config(None)?)
                .unwrap_or_else(|_| serde_json::json!({}));
            c::global_config::set_nested_json_value(&mut raw, &key, parsed.clone());
            let config: GlobalConfig = serde_json::from_value(raw).map_err(|e| e.to_string())?;
            c::global_config::save_global_config(&config, None)?;
            println!(
                "Set {key} = {}",
                serde_json::to_string(&parsed).unwrap_or_default()
            );
            Ok(())
        }
        ConfigSubcommand::SetModel { model } => {
            let mut config = c::global_config::load_global_config(None)?;
            config.model = Some(model.clone());
            c::global_config::save_global_config(&config, None)?;
            println!("Global model set to {model}");
            Ok(())
        }
        ConfigSubcommand::ProjectShow { project } => {
            let project = c::project_registry::get_project(&project, None)?;
            let config = c::project_config::load_project_config(Path::new(&project.path))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&config).unwrap_or_default()
            );
            Ok(())
        }
        ConfigSubcommand::ProjectSetModel { project, model } => {
            let project = c::project_registry::get_project(&project, None)?;
            let mut config = c::project_config::load_project_config(Path::new(&project.path))?;
            config.pi.model = Some(model.clone());
            c::project_config::save_project_config(Path::new(&project.path), &config)?;
            println!("Project {} model set to {model}", project.alias);
            Ok(())
        }
        ConfigSubcommand::SetProvider { provider } => {
            let Some(name) = c::providers::ProviderName::parse(&provider) else {
                eprintln!(
                    "Error: unknown provider '{provider}'. Available: {}",
                    c::providers::PROVIDER_NAMES
                        .iter()
                        .map(|p| p.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                std::process::exit(1);
            };
            let mut config = c::global_config::load_global_config(None)?;
            config.default_provider = Some(name.as_str().to_string());
            c::global_config::save_global_config(&config, None)?;
            println!(
                "Global provider set to {} ({})",
                name.label(),
                name.as_str()
            );
            Ok(())
        }
        ConfigSubcommand::ListProviders => {
            let config = c::global_config::load_global_config(None)?;
            let default_name = config.default_provider.as_deref().unwrap_or("pi");
            for provider in c::providers::PROVIDER_NAMES {
                let available = c::providers::binary_exists(provider.as_str());
                let icon = if available { "✅" } else { "❌" };
                let tag = if provider.as_str() == default_name {
                    " (default)"
                } else {
                    ""
                };
                println!("{icon} {} — {}{tag}", provider.as_str(), provider.label());
            }
            println!("  + custom — Run any shell command (configure per-project)");
            Ok(())
        }
        ConfigSubcommand::ProjectSetAgent {
            project,
            provider,
            command,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            if provider == "custom" && command.is_none() {
                eprintln!("Error: --command is required when provider is 'custom'");
                std::process::exit(1);
            }
            let mut config = c::project_config::load_project_config(Path::new(&project.path))?;
            config.agent = Some(AgentConfig {
                agent_type: Some(provider.clone()),
                command: if provider == "custom" {
                    command.clone()
                } else {
                    None
                },
                extra_args: None,
            });
            c::project_config::save_project_config(Path::new(&project.path), &config)?;
            println!("Project {} agent set to {provider}", project.alias);
            Ok(())
        }
        ConfigSubcommand::AddHook {
            hook_type,
            events,
            command,
            url,
            timeout_seconds,
        } => {
            let hook = build_hook_config(
                &hook_type,
                &events,
                command.as_deref(),
                url.as_deref(),
                timeout_seconds,
            )?;
            let mut config = c::global_config::load_global_config(None)?;
            let mut hooks = config.hooks.clone().unwrap_or_default();
            let events_joined = hook.events.join(", ");
            hooks.push(hook);
            config.hooks = Some(hooks);
            c::global_config::save_global_config(&config, None)?;
            println!("Added global {hook_type} hook (events: {events_joined})");
            Ok(())
        }
        ConfigSubcommand::RemoveHook { index } => {
            let mut config = c::global_config::load_global_config(None)?;
            let mut hooks = config.hooks.clone().unwrap_or_default();
            if index >= hooks.len() {
                eprintln!(
                    "Error: index {index} out of range ({} hooks configured)",
                    hooks.len()
                );
                std::process::exit(1);
            }
            let removed = hooks.remove(index);
            config.hooks = Some(hooks);
            c::global_config::save_global_config(&config, None)?;
            println!("Removed global {} hook at index {index}", removed.hook_type);
            Ok(())
        }
        ConfigSubcommand::ListHooks => {
            let config = c::global_config::load_global_config(None)?;
            let hooks = config.hooks.clone().unwrap_or_default();
            if hooks.is_empty() {
                println!("No global hooks configured.");
                return Ok(());
            }
            for (i, hook) in hooks.iter().enumerate() {
                println!("{}", format_hook(hook, i));
            }
            Ok(())
        }
        ConfigSubcommand::ProjectAddHook {
            project,
            hook_type,
            events,
            command,
            url,
            timeout_seconds,
        } => {
            let hook = build_hook_config(
                &hook_type,
                &events,
                command.as_deref(),
                url.as_deref(),
                timeout_seconds,
            )?;
            let project = c::project_registry::get_project(&project, None)?;
            let mut config = c::project_config::load_project_config(Path::new(&project.path))?;
            let mut hooks = config.hooks.clone().unwrap_or_default();
            let events_joined = hook.events.join(", ");
            hooks.push(hook);
            config.hooks = Some(hooks);
            c::project_config::save_project_config(Path::new(&project.path), &config)?;
            println!(
                "Added project hook for {} (events: {events_joined})",
                project.alias
            );
            Ok(())
        }
        ConfigSubcommand::ProjectRemoveHook { project, index } => {
            let project = c::project_registry::get_project(&project, None)?;
            let mut config = c::project_config::load_project_config(Path::new(&project.path))?;
            let mut hooks = config.hooks.clone().unwrap_or_default();
            if index >= hooks.len() {
                eprintln!(
                    "Error: index {index} out of range ({} hooks configured)",
                    hooks.len()
                );
                std::process::exit(1);
            }
            let removed = hooks.remove(index);
            config.hooks = Some(hooks);
            c::project_config::save_project_config(Path::new(&project.path), &config)?;
            println!(
                "Removed project {} hook at index {index} for {}",
                removed.hook_type, project.alias
            );
            Ok(())
        }
        ConfigSubcommand::ProjectListHooks { project } => {
            let project = c::project_registry::get_project(&project, None)?;
            let config = c::project_config::load_project_config(Path::new(&project.path))?;
            let hooks = config.hooks.clone().unwrap_or_default();
            if hooks.is_empty() {
                println!("No project hooks configured for {}.", project.alias);
                return Ok(());
            }
            for (i, hook) in hooks.iter().enumerate() {
                println!("{}", format_hook(hook, i));
            }
            Ok(())
        }
        ConfigSubcommand::AddChannel {
            channel_type,
            url,
            events,
        } => {
            let events = parse_events(&events);
            if channel_type == "webhook" {
                let Some(url) = url else {
                    eprintln!("Error: --url is required for webhook channels");
                    std::process::exit(1);
                };
                if !url::is_valid(&url) {
                    eprintln!("Error: --url must be a valid URL");
                    std::process::exit(1);
                }
                let mut config = c::global_config::load_global_config(None)?;
                let mut channels = config.notification_channels.clone().unwrap_or_default();
                channels.push(NotificationChannelConfig::Webhook {
                    url: url.clone(),
                    events: events.clone(),
                });
                config.notification_channels = Some(channels);
                c::global_config::save_global_config(&config, None)?;
                println!(
                    "Added webhook notification channel (events: {})",
                    events.join(", ")
                );
            } else if channel_type == "desktop" {
                let mut config = c::global_config::load_global_config(None)?;
                let mut channels = config.notification_channels.clone().unwrap_or_default();
                channels.push(NotificationChannelConfig::Desktop {
                    events: events.clone(),
                });
                config.notification_channels = Some(channels);
                c::global_config::save_global_config(&config, None)?;
                println!(
                    "Added desktop notification channel (events: {})",
                    events.join(", ")
                );
            } else {
                eprintln!("Error: --type must be 'webhook' or 'desktop'");
                std::process::exit(1);
            }
            Ok(())
        }
        ConfigSubcommand::RemoveChannel { index } => {
            let mut config = c::global_config::load_global_config(None)?;
            let mut channels = config.notification_channels.clone().unwrap_or_default();
            if index >= channels.len() {
                eprintln!(
                    "Error: index {index} out of range ({} channels configured)",
                    channels.len()
                );
                std::process::exit(1);
            }
            let removed = channels.remove(index);
            config.notification_channels = Some(channels);
            c::global_config::save_global_config(&config, None)?;
            println!(
                "Removed {} channel at index {index}",
                removed.channel_type()
            );
            Ok(())
        }
        ConfigSubcommand::ListChannels => {
            let config = c::global_config::load_global_config(None)?;
            let channels = config.notification_channels.clone().unwrap_or_default();
            if channels.is_empty() {
                println!("No notification channels configured.");
                println!("Add one with: openloop config add-channel --type webhook --url <URL>");
                return Ok(());
            }
            for (i, channel) in channels.iter().enumerate() {
                let events = if channel.events().is_empty() {
                    "*".to_string()
                } else {
                    channel.events().join(", ")
                };
                match channel {
                    NotificationChannelConfig::Webhook { url, .. } => {
                        println!("[{i}] webhook → {url} (events: {events})");
                    }
                    NotificationChannelConfig::Desktop { .. } => {
                        println!("[{i}] desktop (events: {events})");
                    }
                }
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// service
// ---------------------------------------------------------------------------

use crate::core::service_control::{
    assert_default_provider_available, inspect_daemon_process, restart_service, start_service,
    stop_service, DaemonInspection,
};

fn service(cmd: ServiceCommand) -> Result<(), String> {
    match cmd.command {
        ServiceSubcommand::Start => {
            println!("{}", start_service()?);
            Ok(())
        }
        ServiceSubcommand::Stop => {
            println!("{}", stop_service()?);
            Ok(())
        }
        ServiceSubcommand::Status => {
            let state_path = c::paths::daemon_state_path(None);
            if !c::fsutil::file_exists(&state_path) {
                println!("Daemon has not written state yet.");
                return Ok(());
            }
            let state = c::daemon_state::load_daemon_state(None)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&state).unwrap_or_default()
            );
            Ok(())
        }
        ServiceSubcommand::Restart => {
            println!("{}", restart_service()?);
            Ok(())
        }
        ServiceSubcommand::Pause => {
            let state = c::daemon_state::pause_daemon(None)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "paused": state.paused,
                    "pausedAt": state.paused_at,
                }))
                .unwrap_or_default()
            );
            Ok(())
        }
        ServiceSubcommand::Resume => {
            let state = c::daemon_state::resume_daemon(None)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "paused": state.paused,
                    "pausedAt": state.paused_at,
                }))
                .unwrap_or_default()
            );
            Ok(())
        }
        ServiceSubcommand::Run => {
            assert_default_provider_available()?;
            match inspect_daemon_process()? {
                DaemonInspection::Running(_) => {
                    return Err(
                        "Daemon appears to be running already. Stop it first with 'service stop'."
                            .to_string(),
                    );
                }
                DaemonInspection::Stale(_) => {
                    let _ = std::fs::remove_file(c::paths::daemon_pid_path(None));
                }
                _ => {}
            }
            println!("Starting daemon in foreground mode. Press Ctrl+C to stop.");
            c::worker::start_worker_loop(true)
        }
    }
}

// ---------------------------------------------------------------------------
// logs / run / run-once / enqueue
// ---------------------------------------------------------------------------

fn logs(args: LogsArgs) -> Result<(), String> {
    if let Some(project_alias) = &args.project {
        let project = c::project_registry::get_project(project_alias, None)?;
        let runs_dir = Path::new(&project.path).join(".openloop").join("runs");
        if !c::fsutil::file_exists(&runs_dir) {
            println!("No run summaries found.");
            return Ok(());
        }
        let mut md_files: Vec<String> = std::fs::read_dir(&runs_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|name| name.ends_with(".md"))
                    .collect()
            })
            .unwrap_or_default();
        md_files.sort();
        md_files.reverse();
        md_files.truncate(args.lines);
        for file in md_files {
            let content = std::fs::read_to_string(runs_dir.join(&file)).unwrap_or_default();
            println!("--- {file} ---");
            println!("{content}");
        }
        return Ok(());
    }

    let log_path = c::paths::daemon_log_path(None);
    if !c::fsutil::file_exists(&log_path) {
        println!("No daemon log found.");
        return Ok(());
    }
    let content = std::fs::read_to_string(&log_path).unwrap_or_default();
    let lines: Vec<&str> = content.split('\n').collect();
    // TS slice(-lines): --lines 0 is slice(-0) === slice(0) → everything.
    let start = if args.lines == 0 {
        0
    } else {
        lines.len().saturating_sub(args.lines)
    };
    println!("{}", lines[start..].join("\n"));
    Ok(())
}

fn run(args: RunArgs) -> Result<(), String> {
    let project = c::project_registry::get_project(&args.project, None)?;
    let project_config = c::project_config::load_project_config(Path::new(&project.path))?;
    let global_config = c::global_config::load_global_config(None)?;
    c::pi::assert_provider_available(
        Some(&project_config),
        global_config.default_provider.as_deref(),
    )?;
    if args.verbose {
        eprintln!("[prompt]\n{}\n[/prompt]", args.prompt);
    }
    let result = c::pi::run_pi(&c::pi::PiRunOptions {
        prompt: &args.prompt,
        model: args.model.as_deref(),
        project: &project,
        timeout_ms: None,
    })?;
    if !result.stdout.is_empty() {
        print!("{}", result.stdout);
    }
    if !result.stderr.is_empty() {
        eprint!("{}", result.stderr);
    }
    std::process::exit(result.exit_code);
}

fn run_once(args: RunOnceArgs) -> Result<(), String> {
    let project = c::project_registry::get_project(&args.project, None)?;
    let project_path = Path::new(&project.path);

    if args.dry_run {
        let ledger = c::task_ledger::load_task_ledger(project_path)?;
        let selection = c::scheduler::select_next_task(&ledger);
        let Some(task) = selection.task else {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "mode": "idle",
                    "reason": selection.reason,
                }))
                .unwrap_or_default()
            );
            return Ok(());
        };
        let role = c::scheduler::determine_worker_role(&task, &selection.mode);
        let prompt = c::scheduler::build_prompt(&task, &selection.mode, role, None, true);
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mode": selection.mode,
                "role": role.as_str(),
                "taskId": task.id,
                "reason": selection.reason,
                "prompt": prompt,
            }))
            .unwrap_or_default()
        );
        return Ok(());
    }

    let project_config = c::project_config::load_project_config(project_path)?;
    let global_config = c::global_config::load_global_config(None)?;
    c::pi::assert_provider_available(
        Some(&project_config),
        global_config.default_provider.as_deref(),
    )?;
    let mut hooks = c::scheduler::RunHooks {
        pi_runner: None,
        reviewer_runner: None,
        verifier_runner: None,
        validation_runner: None,
    };
    let result = c::scheduler::run_project_iteration(
        &project,
        &c::scheduler::IterationOptions {
            model_override: args.model.as_deref(),
            timeout_ms: None,
            max_attempts_per_task: None,
            no_progress_repeat_limit: None,
        },
        &mut hooks,
    )?;
    if args.verbose {
        if let Some(prompt) = &result.prompt {
            eprintln!("[prompt]\n{prompt}\n[/prompt]");
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );
    Ok(())
}

fn enqueue(args: EnqueueArgs) -> Result<(), String> {
    eprintln!("Warning: 'enqueue' is deprecated. Use 'task add --from-ref <ref> --project <alias>' instead.");
    let project = c::project_registry::get_project(&args.project, None)?;
    let now = c::now_iso();
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in args.ref_.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            slug.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(64).collect();
    let mut task = ProjectTask {
        id: if slug.is_empty() {
            format!("task-{}", c::now_millis())
        } else {
            slug
        },
        title: format!("Queued work from {}", args.ref_),
        kind: TaskKind::Feature,
        status: TaskStatus::Proposed,
        risk: TaskRisk::MediumRisk,
        scope: None,
        source: TaskSource {
            source_type: "issue".to_string(),
            r#ref: args.ref_.clone(),
        },
        spec_id: None,
        branch: None,
        owner: None,
        acceptance_criteria: vec!["To be defined during planning.".to_string()],
        attempts: 0,
        last_failure_signature: None,
        promotion: TaskPromotion::PullRequest,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: Some(vec!["Created by openloop enqueue.".to_string()]),
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    };
    let task_id = task.id.clone();
    c::task_ledger::add_task(Path::new(&project.path), &mut task)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "project": project.alias,
            "taskId": task_id,
            "ref": args.ref_,
            "status": "proposed",
        }))
        .unwrap_or_default()
    );
    Ok(())
}

/// Lexically normalize a path (resolve `.`/`..` components, drop trailing
/// slashes) without touching the filesystem — mirrors Node's path.resolve.
fn normalize_lexical(path: &std::path::Path) -> std::path::PathBuf {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
