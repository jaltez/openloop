use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskSnapshot {
    id: String,
    title: String,
    status: String,
    risk: String,
    kind: String,
    updated_at: String,
    // TS emits explicit nulls for never-run tasks (key always present).
    estimated_cost_usd: Option<f64>,
    attempts: u32,
    last_run: Option<LastRunSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LastRunSnapshot {
    outcome: String,
    validation: Vec<super::types::ValidationSummary>,
    promotion_action: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectSnapshot {
    alias: String,
    initialized: bool,
    path: String,
    task_summary: super::task_ledger::TaskListSummary,
    tasks: Vec<TaskSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardSnapshot {
    daemon: super::types::DaemonState,
    budget: BudgetSnapshot,
    projects: Vec<ProjectSnapshot>,
    recent_events: Vec<super::types::OpenLoopEvent>,
    generated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BudgetSnapshot {
    spent: f64,
    limit: f64,
    blocked: bool,
}

pub struct DashboardHandle {
    port: u16,
    stop_flag: Arc<AtomicBool>,
    #[allow(dead_code)] // joined on drop; retained to keep handles alive
    threads: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
}

impl DashboardHandle {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        // Poke the listener so accept() unblocks and observes the flag.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

impl Drop for DashboardHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn build_snapshot() -> DashboardSnapshot {
    let daemon = super::daemon_state::load_daemon_state(None).unwrap_or_default();
    let config = super::global_config::load_global_config(None)
        .unwrap_or_else(|_| super::global_config::default_global_config());
    let projects = super::project_registry::list_projects(None).unwrap_or_default();
    let events = super::event_log::read_recent_events(&super::event_log::ReadEventsOptions {
        limit: Some(100),
        ..Default::default()
    });

    let project_snapshots = projects
        .iter()
        .map(|project| {
            let ledger = super::task_ledger::load_task_ledger(Path::new(&project.path)).unwrap_or(
                super::types::TaskLedger {
                    version: 1,
                    updated_at: String::new(),
                    tasks: Vec::new(),
                },
            );
            ProjectSnapshot {
                alias: project.alias.clone(),
                initialized: project.initialized,
                path: project.path.clone(),
                task_summary: super::task_ledger::summarize_tasks(&ledger.tasks),
                tasks: ledger
                    .tasks
                    .iter()
                    .map(|t| TaskSnapshot {
                        id: t.id.clone(),
                        title: t.title.clone(),
                        status: t.status.as_str().to_string(),
                        risk: t.risk.as_str().to_string(),
                        kind: t.kind.as_str().to_string(),
                        updated_at: t.updated_at.clone(),
                        estimated_cost_usd: t.estimated_cost_usd,
                        attempts: t.attempts,
                        last_run: t.last_run.as_ref().map(|lr| LastRunSnapshot {
                            outcome: lr.outcome.as_str().to_string(),
                            validation: lr.validation.clone(),
                            promotion_action: lr.promotion_action.as_str().to_string(),
                        }),
                    })
                    .collect(),
            }
        })
        .collect();

    DashboardSnapshot {
        budget: BudgetSnapshot {
            spent: daemon.budget_spent_usd,
            limit: config.budgets.daily_cost_usd,
            blocked: daemon.budget_blocked,
        },
        daemon,
        projects: project_snapshots,
        recent_events: events,
        generated_at: super::now_iso(),
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_dashboard_html(snapshot: &DashboardSnapshot) -> String {
    let daemon = &snapshot.daemon;
    let budget = &snapshot.budget;

    let project_rows = snapshot
        .projects
        .iter()
        .map(|p| {
            let s = &p.task_summary;
            format!(
                "<tr>\n      <td><strong>{}</strong></td>\n      <td>{}</td>\n      <td>{}</td>\n      <td>{}</td>\n      <td>{}</td>\n      <td>{}</td>\n      <td>{}</td>\n    </tr>",
                escape_html(&p.alias),
                if p.initialized { "✅" } else { "❌" },
                s.total,
                s.by_status.get("ready").copied().unwrap_or(0),
                s.by_status.get("in_progress").copied().unwrap_or(0),
                s.by_status.get("done").copied().unwrap_or(0)
                    + s.by_status.get("promoted").copied().unwrap_or(0),
                s.by_status.get("blocked").copied().unwrap_or(0)
                    + s.by_status.get("failed").copied().unwrap_or(0),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let task_rows = snapshot
        .projects
        .iter()
        .flat_map(|p| {
            let mut rows: Vec<String> = p
                .tasks
                .iter()
                .take(20)
                .map(|t| {
                    let validation_html = match &t.last_run {
                        Some(lr) => lr
                            .validation
                            .iter()
                            .map(|v| {
                                let icon = if v.exit_code == 0 { "✅" } else { "❌" };
                                format!("{icon} {}", escape_html(&v.name.as_str()))
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                        None => "--".to_string(),
                    };
                    format!(
                        "<tr>\n      <td>{}</td>\n      <td>{}</td>\n      <td><span class=\"badge badge-{}\">{}</span></td>\n      <td><span class=\"risk-{}\">{}</span></td>\n      <td>{}</td>\n      <td>{}</td>\n      <td>{}</td>\n      <td>{}</td>\n      <td>{}</td>\n      <td>{}</td>\n    </tr>",
                        escape_html(&p.alias),
                        escape_html(&t.id),
                        t.status,
                        escape_html(&t.status),
                        t.risk,
                        escape_html(&t.risk),
                        escape_html(&t.kind),
                        escape_html(&t.title.chars().take(60).collect::<String>()),
                        t.estimated_cost_usd.map(|c| format!("${c:.4}")).unwrap_or_else(|| "--".into()),
                        t.attempts,
                        validation_html,
                        escape_html(&t.updated_at.chars().take(19).collect::<String>().replacen('T', " ", 1)),
                    )
                })
                .collect();
            let hidden = p.tasks.len().saturating_sub(20);
            if hidden > 0 {
                rows.push(format!(
                    "<tr><td>{}</td><td colspan=\"10\">… and {hidden} more task(s) — \
                     'openloop task list -p {alias}'</td></tr>",
                    escape_html(&p.alias),
                    alias = escape_html(&p.alias),
                ));
            }
            rows
        })
        .collect::<Vec<_>>()
        .join("\n");

    let event_rows = snapshot
        .recent_events
        .iter()
        .rev()
        .take(30)
        .map(|e| {
            format!(
                "<tr>\n    <td>{}</td>\n    <td>{}</td>\n    <td>{}</td>\n    <td>{}</td>\n    <td>{}</td>\n  </tr>",
                escape_html(&e.ts.chars().skip(11).take(8).collect::<String>()),
                escape_html(&e.event),
                escape_html(e.project.as_deref().unwrap_or("--")),
                escape_html(e.task_id.as_deref().unwrap_or("--")),
                e.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "--".into()),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let run_history_rows = snapshot
        .projects
        .iter()
        .flat_map(|p| {
            p.tasks
                .iter()
                .filter(|t| t.last_run.is_some())
                .map(move |t| {
                    let lr = t.last_run.as_ref().unwrap();
                    let outcome_emoji = match lr.outcome.as_str() {
                        "completed" => "✅",
                        "validation-failed" => "⚠️",
                        "pi-failed" => "❌",
                        "error" => "💥",
                        "planned" => "📝",
                        _ => "ℹ️",
                    };
                    let validation_cells = if lr.validation.is_empty() {
                        "--".to_string()
                    } else {
                        lr.validation
                            .iter()
                            .map(|v| {
                                let icon = if v.exit_code == 0 { "✅" } else { "❌" };
                                format!("{icon} {}", escape_html(&v.name.as_str()))
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    format!(
                        "<tr>\n        <td>{}</td>\n        <td>{}</td>\n        <td>{} {}</td>\n        <td>{}</td>\n        <td>{}</td>\n        <td>{}</td>\n        <td>{}</td>\n      </tr>",
                        escape_html(&p.alias),
                        escape_html(&t.id),
                        outcome_emoji,
                        escape_html(&lr.outcome),
                        t.attempts,
                        validation_cells,
                        escape_html(&lr.promotion_action),
                        escape_html(&t.updated_at.chars().take(19).collect::<String>().replacen('T', " ", 1)),
                    )
                })
        })
        .collect::<Vec<_>>()
        .join("\n");

    let budget_class = if budget.blocked {
        "red"
    } else if budget.spent / budget.limit.max(f64::EPSILON) > 0.8 {
        "yellow"
    } else {
        "green"
    };
    let budget_pct = if budget.limit > 0.0 {
        format!("{:.1}", ((budget.spent / budget.limit) * 100.0).min(100.0))
    } else {
        "0".to_string()
    };
    let daemon_status = if daemon.paused {
        "⏸ Paused".to_string()
    } else if daemon.pid > 0 {
        format!("🟢 Running (PID {})", daemon.pid)
    } else {
        "⚫ Stopped".to_string()
    };
    let current_run_html = match &daemon.current_run {
        Some(run) => format!(
            "<p><strong>Current:</strong> {} / {} ({}, attempt {})</p>",
            escape_html(&run.project_alias),
            escape_html(run.task_id.as_deref().unwrap_or("--")),
            escape_html(&run.mode),
            run.attempt_number
        ),
        None => "<p><strong>Current:</strong> idle</p>".to_string(),
    };
    let total_tasks: usize = snapshot.projects.iter().map(|p| p.task_summary.total).sum();
    let progress_color = if budget.blocked {
        "#f85149"
    } else if budget_pct.parse::<f64>().unwrap_or(0.0) > 80.0 {
        "#d29922"
    } else {
        "#3fb950"
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>OpenLoop Dashboard</title>
<meta http-equiv="refresh" content="10">
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #0d1117; color: #c9d1d9; padding: 20px; }}
  h1 {{ color: #58a6ff; margin-bottom: 4px; }}
  h2 {{ color: #8b949e; margin: 24px 0 12px; font-size: 1.1em; text-transform: uppercase; letter-spacing: 0.05em; }}
  .subtitle {{ color: #8b949e; font-size: 0.85em; margin-bottom: 20px; }}
  .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; margin-bottom: 24px; }}
  .card {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px; }}
  .card-label {{ color: #8b949e; font-size: 0.85em; }}
  .card-value {{ font-size: 1.5em; font-weight: 600; margin-top: 4px; }}
  .card-value.green {{ color: #3fb950; }}
  table {{ width: 100%; border-collapse: collapse; background: #161b22; border-radius: 8px; overflow: hidden; }}
  th {{ background: #21262d; color: #8b949e; text-align: left; padding: 8px 12px; font-size: 0.85em; text-transform: uppercase; }}
  td {{ padding: 8px 12px; border-top: 1px solid #21262d; font-size: 0.9em; }}
  .badge {{ padding: 2px 8px; border-radius: 12px; font-size: 0.8em; font-weight: 500; }}
  .badge-done, .badge-promoted {{ background: #238636; color: #fff; }}
  .badge-ready {{ background: #1f6feb; color: #fff; }}
  .badge-in_progress {{ background: #d29922; color: #000; }}
  .badge-proposed, .badge-planned {{ background: #30363d; color: #c9d1d9; }}
  .badge-blocked, .badge-failed {{ background: #da3633; color: #fff; }}
  .badge-awaiting-approval {{ background: #a371f7; color: #fff; }}
  .badge-cancelled {{ background: #484f58; color: #c9d1d9; }}
  .progress-bar {{ background: #21262d; border-radius: 4px; height: 8px; margin-top: 8px; overflow: hidden; }}
  .progress-fill {{ height: 100%; border-radius: 4px; transition: width 0.3s; }}
  .section {{ margin-bottom: 32px; }}
  footer {{ color: #484f58; font-size: 0.8em; margin-top: 40px; text-align: center; }}
</style>
</head>
<body>
<h1>OpenLoop Dashboard</h1>
<p class="subtitle">Auto-refreshes every 10 seconds &middot; {}</p>

<div class="grid">
  <div class="card">
    <div class="card-label">Daemon</div>
    <div class="card-value">{daemon_status}</div>
    {current_run_html}
  </div>
  <div class="card">
    <div class="card-label">Budget Today</div>
    <div class="card-value {budget_class}">${:.2} / ${:.2}</div>
    <div class="progress-bar"><div class="progress-fill" style="width: {budget_pct}%; background: {progress_color};"></div></div>
  </div>
  <div class="card">
    <div class="card-label">Projects</div>
    <div class="card-value green">{}</div>
  </div>
  <div class="card">
    <div class="card-label">Total Tasks</div>
    <div class="card-value">{total_tasks}</div>
  </div>
</div>

<div class="section">
<h2>Projects</h2>
<table>
  <thead><tr><th>Alias</th><th>Init</th><th>Total</th><th>Ready</th><th>Active</th><th>Done</th><th>Issues</th></tr></thead>
  <tbody>{}</tbody>
</table>
</div>

<div class="section">
<h2>Tasks</h2>
<table>
  <thead><tr><th>Project</th><th>ID</th><th>Status</th><th>Risk</th><th>Kind</th><th>Title</th><th>Cost</th><th>Att.</th><th>Validation</th><th>Updated</th></tr></thead>
  <tbody>{}</tbody>
</table>
</div>

<div class="section">
<h2>Run History &amp; Validation Results</h2>
<table>
  <thead><tr><th>Project</th><th>Task</th><th>Outcome</th><th>Attempts</th><th>Validation</th><th>Promotion</th><th>Updated</th></tr></thead>
  <tbody>{}</tbody>
</table>
</div>

<div class="section">
<h2>Recent Events</h2>
<table>
  <thead><tr><th>Time</th><th>Event</th><th>Project</th><th>Task</th><th>Exit</th></tr></thead>
  <tbody>{}</tbody>
</table>
</div>

<footer>OpenLoop &middot; Ship while you sleep</footer>
</body>
</html>"#,
        escape_html(
            &snapshot
                .generated_at
                .chars()
                .take(19)
                .collect::<String>()
                .replacen('T', " ", 1)
        ),
        budget.spent,
        budget.limit,
        snapshot.projects.len(),
        if project_rows.is_empty() {
            "<tr><td colspan=\"7\">No projects linked</td></tr>".to_string()
        } else {
            project_rows
        },
        if task_rows.is_empty() {
            "<tr><td colspan=\"10\">No tasks</td></tr>".to_string()
        } else {
            task_rows
        },
        if run_history_rows.is_empty() {
            "<tr><td colspan=\"7\">No run history</td></tr>".to_string()
        } else {
            run_history_rows
        },
        if event_rows.is_empty() {
            "<tr><td colspan=\"5\">No events</td></tr>".to_string()
        } else {
            event_rows
        },
    )
}

fn json_of(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn handle_api_request(
    pathname: &str,
    snapshot: &DashboardSnapshot,
) -> Option<(u16, &'static str, String)> {
    if pathname == "/api/snapshot" {
        return Some((200, "application/json", json_of(snapshot)));
    }
    if pathname == "/api/daemon" {
        return Some((200, "application/json", json_of(&snapshot.daemon)));
    }
    if pathname == "/api/projects" {
        return Some((200, "application/json", json_of(&snapshot.projects)));
    }
    if pathname == "/api/events" {
        return Some((200, "application/json", json_of(&snapshot.recent_events)));
    }
    if pathname == "/api/budget" {
        return Some((200, "application/json", json_of(&snapshot.budget)));
    }
    None
}

fn handle_connection(mut stream: TcpStream) {
    // Drop connections that connect and never send (node's requestTimeout
    // parity): without this, idle clients hold threads and fds forever.
    // Linux clears O_NONBLOCK in accept(2), but macOS inherits it from the
    // listener — without this reset every read would return WouldBlock
    // before request bytes arrive and every request would 405.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    // A client that stops reading mid-response must not hold the thread
    // once the send buffer fills (localhost-only, but still a leak).
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));

    let mut buffer = [0u8; 8192];
    let read = stream.read(&mut buffer).unwrap_or(0);
    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
    let request_line = request.lines().next().unwrap_or_default().to_string();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    // Only allow GET requests
    if method != "GET" {
        let _ = write_response(&mut stream, 405, "text/plain", "Method Not Allowed");
        return;
    }

    // Only allow connections from localhost
    if let Ok(peer) = stream.peer_addr() {
        let ip = peer.ip().to_string();
        if !["127.0.0.1", "::1"].contains(&ip.as_str()) {
            let _ = write_response(&mut stream, 403, "text/plain", "Forbidden");
            return;
        }
    }

    let pathname = path.split('?').next().unwrap_or("/").to_string();
    let snapshot = build_snapshot();

    if let Some((status, content_type, body)) = handle_api_request(&pathname, &snapshot) {
        let _ = write_response(&mut stream, status, content_type, &body);
        return;
    }

    let html = render_dashboard_html(&snapshot);
    let _ = write_response(&mut stream, 200, "text/html; charset=utf-8", &html);
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

pub fn start_dashboard(port: u16) -> Result<DashboardHandle, String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop_flag = Arc::new(AtomicBool::new(false));
    let threads = Arc::new(Mutex::new(Vec::new()));

    let flag = stop_flag.clone();
    let thread_registry = threads.clone();
    let accept_thread = std::thread::spawn(move || {
        for stream in listener.incoming() {
            if flag.load(Ordering::SeqCst) {
                break;
            }
            match stream {
                Ok(stream) => {
                    let flag = flag.clone();
                    let registry = thread_registry.clone();
                    let handle = std::thread::spawn(move || {
                        if !flag.load(Ordering::SeqCst) {
                            handle_connection(stream);
                        }
                    });
                    registry.lock().push(handle);
                    // Bound the thread count: join finished handlers opportunistically.
                    let mut guard = registry.lock();
                    guard.retain(|h| !h.is_finished());
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                // Transient accept errors (EMFILE under fd pressure,
                // ECONNABORTED on client resets) must not kill the server
                // permanently (node's http.Server survived them); back off
                // and keep serving.
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    });
    threads.lock().push(accept_thread);

    Ok(DashboardHandle {
        port,
        stop_flag,
        threads,
    })
}
