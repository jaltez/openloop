// Integration tests for the dashboard HTTP server: HTML + JSON endpoints,
// the non-GET guard, cache headers, and dashboard config persistence.
// Port of tests/integration/dashboard.test.ts.

use parking_lot::{Mutex, MutexGuard};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use openloop::core::daemon_state::{create_default_daemon_state, save_daemon_state};
use openloop::core::dashboard::{start_dashboard, DashboardHandle};
use openloop::core::global_config::{load_global_config, save_global_config};
use openloop::core::project_registry::add_project;
use openloop::core::types::*;

// The dashboard resolves OPENLOOP_HOME per request; env mutation must be
// serialized across tests in this process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock()
}

struct Sandbox {
    home: PathBuf,
    target: PathBuf,
    saved_home: Option<String>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        match self.saved_home.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        let _ = fs::remove_dir_all(&self.home);
        let _ = fs::remove_dir_all(&self.target);
    }
}

fn sandbox(name: &str) -> Sandbox {
    let home = std::env::temp_dir().join(format!("ol-dash-home-{name}-{}", std::process::id()));
    let target = std::env::temp_dir().join(format!("ol-dash-target-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(home.join("run")).unwrap();
    fs::create_dir_all(&target).unwrap();

    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);

    Sandbox {
        home,
        target,
        saved_home,
    }
}

fn make_task(
    id: &str,
    title: &str,
    status: TaskStatus,
    risk: TaskRisk,
    kind: TaskKind,
) -> ProjectTask {
    let now = "2026-01-01T00:00:00.000Z".to_string();
    ProjectTask {
        id: id.into(),
        title: title.into(),
        kind,
        status,
        risk,
        scope: None,
        source: TaskSource {
            source_type: "human".into(),
            r#ref: "test".into(),
        },
        spec_id: None,
        branch: None,
        owner: None,
        acceptance_criteria: vec![],
        attempts: 0,
        last_failure_signature: None,
        promotion: TaskPromotion::ManualOnly,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: None,
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

/// Seed daemon state (with the current pid), global config (dashboard on a
/// free port), one linked project, and a two-task ledger.
fn seed_state(sandbox: &Sandbox) -> u16 {
    let mut state = create_default_daemon_state();
    state.pid = std::process::id() as i32;
    save_daemon_state(&state, None).unwrap();

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut config = load_global_config(None).unwrap();
    config.dashboard = Some(DashboardConfig {
        port,
        enabled: true,
    });
    save_global_config(&config, None).unwrap();

    add_project("test-app", &sandbox.target.to_string_lossy(), None).unwrap();

    let ledger = TaskLedger {
        version: 1,
        updated_at: "2026-01-01T00:00:00.000Z".into(),
        tasks: vec![
            make_task(
                "task-1",
                "Fix login bug",
                TaskStatus::Ready,
                TaskRisk::LowRisk,
                TaskKind::Bugfix,
            ),
            make_task(
                "task-2",
                "Add tests",
                TaskStatus::Done,
                TaskRisk::MediumRisk,
                TaskKind::Test,
            ),
        ],
    };
    fs::create_dir_all(sandbox.target.join(".openloop")).unwrap();
    fs::write(
        sandbox.target.join(".openloop").join("tasks.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();

    port
}

fn start_server() -> DashboardHandle {
    // Bind an ephemeral listener to reserve a free port, then hand it to the
    // dashboard. Retry a few times in case another process grabs it in between.
    for _ in 0..5 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        if let Ok(handle) = start_dashboard(port) {
            return handle;
        }
    }
    panic!("could not bind the dashboard to any free port");
}

struct HttpResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: String,
}

fn http_request(port: u16, method: &str, path: &str) -> HttpResponse {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let request =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();

    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();

    let (head, body) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {raw:?}"));
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("malformed status line: {status_line}"));
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();

    HttpResponse {
        status,
        headers,
        body: body.to_string(),
    }
}

fn http_get(port: u16, path: &str) -> HttpResponse {
    http_request(port, "GET", path)
}

fn json_body(response: &HttpResponse) -> serde_json::Value {
    serde_json::from_str(&response.body).expect("valid JSON body")
}

#[test]
fn serves_html_dashboard_on_root() {
    let _guard = env_lock();
    let sandbox = sandbox("html-root");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_get(server.port(), "/");
    assert_eq!(res.status, 200);
    assert!(
        res.headers
            .get("content-type")
            .is_some_and(|v| v.contains("text/html")),
        "content-type: {:?}",
        res.headers.get("content-type")
    );
    assert!(res.body.contains("OpenLoop Dashboard"));
    assert!(res.body.contains("test-app"));

    server.stop();
}

#[test]
fn serves_json_snapshot_on_api_snapshot() {
    let _guard = env_lock();
    let sandbox = sandbox("api-snapshot");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_get(server.port(), "/api/snapshot");
    assert_eq!(res.status, 200);
    assert!(
        res.headers
            .get("content-type")
            .is_some_and(|v| v.contains("application/json")),
        "content-type: {:?}",
        res.headers.get("content-type")
    );
    let data = json_body(&res);
    assert!(data.get("daemon").is_some(), "missing daemon: {data}");
    assert!(data.get("budget").is_some(), "missing budget: {data}");
    assert!(data.get("projects").is_some(), "missing projects: {data}");
    assert!(
        data.get("generatedAt").is_some_and(|v| !v.is_null()),
        "missing generatedAt: {data}"
    );

    server.stop();
}

#[test]
fn serves_daemon_state_on_api_daemon() {
    let _guard = env_lock();
    let sandbox = sandbox("api-daemon");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_get(server.port(), "/api/daemon");
    assert_eq!(res.status, 200);
    let data = json_body(&res);
    assert_eq!(data["pid"].as_i64(), Some(i64::from(std::process::id())));

    server.stop();
}

#[test]
fn serves_projects_data_on_api_projects() {
    let _guard = env_lock();
    let sandbox = sandbox("api-projects");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_get(server.port(), "/api/projects");
    assert_eq!(res.status, 200);
    let data = json_body(&res);
    let projects = data.as_array().expect("projects is an array");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["alias"], "test-app");
    assert_eq!(projects[0]["tasks"].as_array().map(Vec::len), Some(2));
    assert_eq!(projects[0]["taskSummary"]["total"].as_u64(), Some(2));

    server.stop();
}

#[test]
fn serves_budget_data_on_api_budget() {
    let _guard = env_lock();
    let sandbox = sandbox("api-budget");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_get(server.port(), "/api/budget");
    assert_eq!(res.status, 200);
    let data = json_body(&res);
    assert_eq!(data["limit"].as_f64(), Some(25.0));
    assert_eq!(data["blocked"].as_bool(), Some(false));

    server.stop();
}

#[test]
fn serves_events_on_api_events() {
    let _guard = env_lock();
    let sandbox = sandbox("api-events");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_get(server.port(), "/api/events");
    assert_eq!(res.status, 200);
    assert!(
        res.headers
            .get("content-type")
            .is_some_and(|v| v.contains("application/json")),
        "content-type: {:?}",
        res.headers.get("content-type")
    );
    let data = json_body(&res);
    assert!(data.is_array(), "events is not an array: {data}");

    server.stop();
}

#[test]
fn includes_task_data_in_html_response() {
    let _guard = env_lock();
    let sandbox = sandbox("html-tasks");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_get(server.port(), "/");
    assert_eq!(res.status, 200);
    assert!(res.body.contains("Fix login bug"));
    assert!(res.body.contains("task-1"));
    assert!(res.body.contains("task-2"));

    server.stop();
}

#[test]
fn sets_no_store_cache_control() {
    let _guard = env_lock();
    let sandbox = sandbox("cache-control");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_get(server.port(), "/");
    assert_eq!(
        res.headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );

    server.stop();
}

#[test]
fn rejects_non_get_requests_with_405() {
    let _guard = env_lock();
    let sandbox = sandbox("method-guard");
    seed_state(&sandbox);
    let server = start_server();

    let res = http_request(server.port(), "POST", "/");
    assert_eq!(res.status, 405);

    server.stop();
}

#[test]
fn dashboard_config_persists_in_global_config() {
    let _guard = env_lock();
    let sandbox = sandbox("config-persist");
    let port = seed_state(&sandbox);

    let config = load_global_config(None).unwrap();
    let dashboard = config.dashboard.expect("dashboard config present");
    assert_eq!(dashboard.port, port);
    assert!(dashboard.enabled);
}
