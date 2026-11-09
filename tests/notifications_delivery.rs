// Integration tests for notification delivery reliability: chat presets,
// the delivery ledger, and the bounded retry queue drained by the daemon.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use openloop::core::types::*;
use openloop::core::{
    daemon_state, global_config, hooks::LifecycleHookPayload, notifications,
    worker::run_worker_tick,
};
use parking_lot::{Mutex, MutexGuard};
use std::fs;

// Notifications resolve the app home from OPENLOOP_HOME; env mutation must
// be serialized across tests in this process.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock()
}

struct Sandbox {
    home: PathBuf,
    saved_home: Option<String>,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        match self.saved_home.take() {
            Some(prev) => std::env::set_var("OPENLOOP_HOME", prev),
            None => std::env::remove_var("OPENLOOP_HOME"),
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn sandbox(name: &str) -> Sandbox {
    let home = std::env::temp_dir().join(format!("ol-notif-home-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    let saved_home = std::env::var("OPENLOOP_HOME").ok();
    std::env::set_var("OPENLOOP_HOME", &home);
    Sandbox { home, saved_home }
}

/// Local listener that records raw requests and fails the first
/// `fail_first` requests with HTTP 500 before serving 200s.
fn stateful_listener(fail_first: usize) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let fails_left = std::cell::Cell::new(fail_first);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut conn, _)) => {
                    conn.set_read_timeout(Some(Duration::from_millis(300))).ok();
                    let mut request = String::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        match conn.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => request.push_str(&String::from_utf8_lossy(&buf[..n])),
                        }
                    }
                    let response = if fails_left.get() > 0 {
                        fails_left.set(fails_left.get() - 1);
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n"
                    } else {
                        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
                    };
                    let _ = conn.write_all(response.as_bytes());
                    let _ = tx.send(request);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://{addr}/hook"), rx)
}

fn slack_config(url: &str) -> GlobalConfig {
    let mut config = global_config::default_global_config();
    config.notification_channels = Some(vec![NotificationChannelConfig::Slack {
        url: url.to_string(),
        events: vec!["*".to_string()],
    }]);
    config
}

fn payload(event: &str) -> LifecycleHookPayload {
    LifecycleHookPayload {
        event: event.to_string(),
        project: "demo".to_string(),
        task_id: "fix-login".to_string(),
        message: "Task fix-login completed".to_string(),
        timestamp: openloop::core::now_iso(),
        ..Default::default()
    }
}

fn queue_len() -> usize {
    daemon_state::load_daemon_state(None)
        .unwrap()
        .notification_queue
        .map(|q| q.len())
        .unwrap_or(0)
}

fn status_for(prefix: &str) -> NotificationDeliveryStatus {
    let state = daemon_state::load_daemon_state(None).unwrap();
    let statuses = state.notification_status.clone().unwrap_or_default();
    statuses
        .iter()
        .find(|s| s.channel_key.starts_with(prefix))
        .expect("status entry present")
        .clone()
}

fn force_queue_due() {
    daemon_state::with_daemon_state(
        |state| {
            let past = "2020-01-01T00:00:00.000Z".to_string();
            if let Some(queue) = state.notification_queue.as_mut() {
                for entry in queue.iter_mut() {
                    entry.next_attempt_at = past.clone();
                }
            }
        },
        None,
    )
    .unwrap();
}

#[test]
fn slack_preset_posts_plain_text_payload() {
    let _env = env_lock();
    let _s = sandbox("slack");
    let (url, rx) = stateful_listener(0);

    notifications::fire_notifications(&slack_config(&url), &payload("task-complete")).unwrap();
    let request = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("slack channel must fire");
    assert!(request.contains("POST /hook"), "request: {request}");
    assert!(request.contains("\"text\""), "slack body: {request}");
    assert!(request.contains("fix-login"), "slack body: {request}");

    // Delivered: ledger records ok and nothing is queued for retry.
    let status = status_for("slack:");
    assert!(status.ok);
    assert_eq!(queue_len(), 0);
}

#[test]
fn failed_delivery_is_queued_and_retried_to_success() {
    let _env = env_lock();
    let _s = sandbox("retry");
    let (url, rx) = stateful_listener(1);

    notifications::fire_notifications(&slack_config(&url), &payload("task-complete")).unwrap();
    let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        first.contains("500") || first.contains("POST"),
        "req: {first}"
    );

    // Failure recorded and queued with backoff in the future.
    assert_eq!(queue_len(), 1);
    let entry = daemon_state::load_daemon_state(None)
        .unwrap()
        .notification_queue
        .unwrap()[0]
        .clone();
    assert_eq!(entry.attempts, 1);
    let status = status_for("slack:");
    assert!(!status.ok);

    // Not due yet: drain does nothing.
    notifications::drain_notification_queue(&slack_config(&url));
    assert_eq!(queue_len(), 1);

    // Once due, the retry succeeds and clears the queue.
    force_queue_due();
    notifications::drain_notification_queue(&slack_config(&url));
    let retried = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("retry must fire");
    assert!(retried.contains("POST"), "retry request: {retried}");
    assert_eq!(queue_len(), 0);
    let status = status_for("slack:");
    assert!(status.ok, "ledger must record the successful retry");
}

#[test]
fn drain_gives_up_after_max_attempts_and_records_it() {
    let _env = env_lock();
    let _s = sandbox("giveup");
    let (url, _rx) = stateful_listener(usize::MAX); // always 500

    notifications::fire_notifications(&slack_config(&url), &payload("task-failed")).unwrap();
    assert_eq!(queue_len(), 1);

    force_queue_due();
    notifications::drain_notification_queue(&slack_config(&url)); // attempt 2
    assert_eq!(queue_len(), 1, "attempt 2 failure reschedules");
    force_queue_due();
    notifications::drain_notification_queue(&slack_config(&url)); // attempt 3: give up
    assert_eq!(queue_len(), 0, "attempt 3 failure drops the entry");

    let status = status_for("slack:");
    assert!(!status.ok);
    assert!(
        status.error.as_deref().unwrap_or("").contains("3 attempts"),
        "error: {:?}",
        status.error
    );
}

#[test]
fn telegram_endpoint_is_derived_from_bot_token() {
    assert_eq!(
        notifications::telegram_endpoint("123:ABC"),
        "https://api.telegram.org/bot123:ABC/sendMessage"
    );
}

#[test]
fn worker_tick_drains_due_notifications() {
    let _env = env_lock();
    let s = sandbox("tickdrain");
    let (url, rx) = stateful_listener(1);

    // Empty registry: an idle tick that still runs the drain. The channel
    // must live in the on-disk config — the tick loads its own copy.
    fs::write(
        s.home.join("projects.json"),
        serde_json::to_string(&ProjectsRegistry {
            version: 1,
            projects: vec![],
        })
        .unwrap(),
    )
    .unwrap();
    let config = slack_config(&url);
    fs::write(
        s.home.join("config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();

    notifications::fire_notifications(&slack_config(&url), &payload("task-complete")).unwrap();
    let _ = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(queue_len(), 1);
    force_queue_due();

    run_worker_tick("2026-01-01T00:00:00.000Z").unwrap();
    let _ = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("tick must retry due deliveries");
    assert_eq!(queue_len(), 0);
}

#[test]
fn queue_is_bounded_and_drops_oldest() {
    let _env = env_lock();
    let _s = sandbox("cap");
    let (url, _rx) = stateful_listener(usize::MAX);

    // 25 fires -> 25 failures -> queue capped at 20, oldest dropped.
    for _ in 0..25 {
        notifications::fire_notifications(&slack_config(&url), &payload("task-failed")).unwrap();
    }
    assert_eq!(queue_len(), 20);
}
