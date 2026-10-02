use std::io::{BufRead, Write};
type ToolHandler = Box<dyn Fn(&serde_json::Value) -> Result<serde_json::Value, String>>;

use super::now_iso;
use super::types::*;

struct McpTool {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
    handler: ToolHandler,
}

fn write_response(id: Option<&serde_json::Value>, result: serde_json::Value) {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{payload}");
    let _ = stdout.flush();
}

fn write_error(id: Option<serde_json::Value>, code: i64, message: &str) {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    });
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{payload}");
    let _ = stdout.flush();
}

fn required_string(args: &serde_json::Value, key: &str, tool_name: &str) -> Result<String, String> {
    let value = args
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Tool {tool_name} requires a non-empty '{key}' argument."))?;
    Ok(value.to_string())
}

fn resolve_project_path(alias: &str) -> Result<String, String> {
    Ok(super::project_registry::get_project(alias, None)?.path)
}

const VALID_KINDS: &[&str] = &[
    "feature",
    "bugfix",
    "test",
    "refactor",
    "docs",
    "lint-fix",
    "type-fix",
    "localized-test-fix",
    "ci-heal",
    "discovery",
    "scope-proposal",
];
const VALID_RISKS: &[&str] = &["low-risk", "medium-risk", "high-risk"];

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in title.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(64).collect();
    if slug.is_empty() {
        format!("task-{}", super::now_millis())
    } else {
        slug
    }
}

fn make_task(args: &serde_json::Value) -> Result<ProjectTask, String> {
    let title = required_string(args, "title", "openloop_add_task")?;
    let kind = args
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("feature");
    let risk = args
        .get("risk")
        .and_then(|v| v.as_str())
        .unwrap_or("medium-risk");
    if !VALID_KINDS.contains(&kind) {
        return Err(format!("Invalid kind: {kind}"));
    }
    if !VALID_RISKS.contains(&risk) {
        return Err(format!("Invalid risk: {risk}"));
    }
    let now = now_iso();
    Ok(ProjectTask {
        id: slugify(&title),
        title,
        kind: TaskKind::parse(kind).ok_or_else(|| format!("Invalid kind: {kind}"))?,
        status: TaskStatus::Proposed,
        risk: TaskRisk::parse(risk).ok_or_else(|| format!("Invalid risk: {risk}"))?,
        scope: None,
        source: TaskSource {
            source_type: "human".to_string(),
            r#ref: "openloop mcp".to_string(),
        },
        spec_id: None,
        branch: None,
        owner: Some("openloop".to_string()),
        acceptance_criteria: vec!["To be defined during planning.".to_string()],
        attempts: 0,
        last_failure_signature: None,
        promotion: TaskPromotion::PullRequest,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: Some(vec!["Created via openloop MCP.".to_string()]),
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    })
}

fn build_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            name: "openloop_list_projects",
            description: "List projects linked to the openloop control plane.",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            handler: Box::new(|_args| {
                serde_json::to_value(super::project_registry::list_projects(None)?)
                    .map_err(|e| e.to_string())
            }),
        },
        McpTool {
            name: "openloop_list_tasks",
            description: "List tasks for a linked project.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "project": { "type": "string", "description": "Project alias" } },
                "required": ["project"]
            }),
            handler: Box::new(|args| {
                let alias = required_string(args, "project", "openloop_list_tasks")?;
                let project_path = resolve_project_path(&alias)?;
                let ledger =
                    super::task_ledger::load_task_ledger(std::path::Path::new(&project_path))?;
                serde_json::to_value(ledger.tasks).map_err(|e| e.to_string())
            }),
        },
        McpTool {
            name: "openloop_add_task",
            description: "Add a task to a linked project.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project alias" },
                    "title": { "type": "string", "description": "Task title" },
                    "kind": { "type": "string", "description": "Task kind (default feature)" },
                    "risk": { "type": "string", "description": "low-risk | medium-risk | high-risk (default medium-risk)" }
                },
                "required": ["project", "title"]
            }),
            handler: Box::new(|args| {
                let alias = required_string(args, "project", "openloop_add_task")?;
                let project_path = resolve_project_path(&alias)?;
                let mut task = make_task(args)?;
                let task_id = task.id.clone();
                super::task_ledger::add_task(std::path::Path::new(&project_path), &mut task)?;
                Ok(serde_json::json!({ "id": task_id, "project": alias }))
            }),
        },
        McpTool {
            name: "openloop_task_show",
            description: "Show a single task by id.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project alias" },
                    "taskId": { "type": "string", "description": "Task id" }
                },
                "required": ["project", "taskId"]
            }),
            handler: Box::new(|args| {
                let alias = required_string(args, "project", "openloop_task_show")?;
                let task_id = required_string(args, "taskId", "openloop_task_show")?;
                let project_path = resolve_project_path(&alias)?;
                let task =
                    super::task_ledger::get_task(std::path::Path::new(&project_path), &task_id)?;
                serde_json::to_value(task).map_err(|e| e.to_string())
            }),
        },
        McpTool {
            name: "openloop_promotion_queue",
            description: "List pending promotion artifacts for a linked project.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "project": { "type": "string", "description": "Project alias" } },
                "required": ["project"]
            }),
            handler: Box::new(|args| {
                let alias = required_string(args, "project", "openloop_promotion_queue")?;
                let project_path = resolve_project_path(&alias)?;
                let artifacts = super::promotion_queue::list_promotion_artifacts(
                    std::path::Path::new(&project_path),
                )?;
                let pending: Vec<&super::promotion_queue::PromotionQueueItem> = artifacts
                    .iter()
                    .filter(|item| item.artifact.status == PromotionArtifactStatus::Pending)
                    .collect();
                let mapped: Vec<serde_json::Value> = pending
                    .iter()
                    .map(|item| serde_json::json!({ "artifactPath": item.artifact_path, "artifact": item.artifact }))
                    .collect();
                Ok(serde_json::Value::Array(mapped))
            }),
        },
        McpTool {
            name: "openloop_digest",
            description: "Fleet confidence digest: activity, spend split, review queue.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "sinceHours": { "type": "number", "description": "Window in hours (default 24)" } }
            }),
            handler: Box::new(|args| {
                let hours = args
                    .get("sinceHours")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(24.0);
                let digest =
                    super::digest::build_digest(Some((hours * 60.0 * 60.0 * 1000.0) as i64), None)?;
                serde_json::to_value(digest).map_err(|e| e.to_string())
            }),
        },
        McpTool {
            name: "openloop_budget_status",
            description: "Daemon budget state plus the configured daily budget.",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            handler: Box::new(|_args| {
                let daemon = super::daemon_state::load_daemon_state(None)?;
                let config = super::global_config::load_global_config(None)?;
                Ok(serde_json::json!({
                    "dailyCostUsd": config.budgets.daily_cost_usd,
                    "estimatedCostPerRunUsd": config.budgets.estimated_cost_per_run_usd,
                    "budgetSpentUsd": daemon.budget_spent_usd,
                    "totalBudgetSpentUsd": daemon.total_budget_spent_usd,
                    "budgetDate": daemon.budget_date,
                    "budgetBlocked": daemon.budget_blocked,
                }))
            }),
        },
        McpTool {
            name: "openloop_pause",
            description: "Pause the daemon (no new runs).",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            handler: Box::new(|_args| {
                let state = super::daemon_state::pause_daemon(None)?;
                serde_json::to_value(state).map_err(|e| e.to_string())
            }),
        },
        McpTool {
            name: "openloop_resume",
            description: "Resume the daemon.",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            handler: Box::new(|_args| {
                let state = super::daemon_state::resume_daemon(None)?;
                serde_json::to_value(state).map_err(|e| e.to_string())
            }),
        },
    ]
}

fn handle_message(message: &serde_json::Value) {
    let method = message.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = message.get("id").cloned();

    // JSON-RPC notifications carry no id and MUST NOT be answered. Known
    // notifications are handled silently; unknown ones are ignored.
    let Some(id) = id else {
        return;
    };

    if method == "initialize" {
        write_response(
            Some(&id),
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "openloop", "version": VERSION }
            }),
        );
        return;
    }
    if method == "tools/list" {
        let tools = build_tools();
        let list: Vec<serde_json::Value> = tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": tool.input_schema,
                })
            })
            .collect();
        write_response(Some(&id), serde_json::json!({ "tools": list }));
        return;
    }
    if method == "tools/call" {
        let params = message
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let tools = build_tools();
        let Some(tool) = tools.iter().find(|t| t.name == name) else {
            write_response(
                Some(&id),
                serde_json::json!({
                    "content": [{ "type": "text", "text": format!("Unknown tool: {name}") }],
                    "isError": true
                }),
            );
            return;
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));
        match (tool.handler)(&arguments) {
            Ok(result) => {
                write_response(
                    Some(&id),
                    serde_json::json!({
                        "content": [{ "type": "text", "text": serde_json::to_string(&result).unwrap_or_default() }]
                    }),
                );
            }
            Err(error) => {
                write_response(
                    Some(&id),
                    serde_json::json!({
                        "content": [{ "type": "text", "text": error }],
                        "isError": true
                    }),
                );
            }
        }
        return;
    }

    write_error(Some(id), -32601, &format!("Method not found: {method}"));
}

/// Stdio JSON-RPC 2.0 server: one message per line, responses likewise.
/// Nothing else may write to stdout while this loop runs.
pub fn run_mcp_server() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<serde_json::Value>(trimmed);
        match parsed {
            Ok(message) => handle_message(&message),
            Err(_) => write_error(None, -32700, "Parse error"),
        }
    }
    Ok(())
}
