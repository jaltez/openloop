use chrono::TimeZone;
use std::time::Duration;

use super::now_iso;
use super::types::{
    GlobalConfig, NotificationChannelConfig, NotificationDeliveryStatus, QueuedNotification,
};

/// Fire notifications through all configured channels. Delivery outcomes are
/// recorded in daemon state (`notificationStatus`); failed HTTP deliveries
/// are queued for bounded retry (drained by the daemon tick, see
/// `drain_notification_queue`). Firing never fails the caller.
/// Shell-command hooks (legacy) are handled separately in the worker.
pub fn fire_notifications(
    config: &GlobalConfig,
    payload: &super::hooks::LifecycleHookPayload,
) -> Result<(), String> {
    let channels = config.notification_channels.clone().unwrap_or_default();
    for channel in channels {
        let matches_event = channel.events().is_empty()
            || channel
                .events()
                .iter()
                .any(|e| e == &payload.event || e == "*");
        if !matches_event {
            continue;
        }
        let key = channel_key(&channel);
        let result = deliver(&channel, payload);
        match &result {
            Ok(()) => record_delivery_status(&key, true, None, Some(&payload.event)),
            Err(error) => {
                record_delivery_status(&key, false, Some(error), Some(&payload.event));
                enqueue_retry(&key, payload, error);
            }
        }
    }
    Ok(())
}

/// Deliver one payload through one channel.
fn deliver(
    channel: &NotificationChannelConfig,
    payload: &super::hooks::LifecycleHookPayload,
) -> Result<(), String> {
    match channel {
        NotificationChannelConfig::Webhook { url, .. } => {
            // Raw webhook keeps the full payload JSON (TS parity).
            send_json(
                url,
                &serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
            )
        }
        NotificationChannelConfig::Slack { url, .. } => {
            send_json(url, &serde_json::json!({ "text": plain_message(payload) }))
        }
        NotificationChannelConfig::Discord { url, .. } => send_json(
            url,
            &serde_json::json!({ "content": plain_message(payload) }),
        ),
        NotificationChannelConfig::Telegram {
            bot_token, chat_id, ..
        } => send_json(
            &telegram_endpoint(bot_token),
            &serde_json::json!({
                "chat_id": chat_id,
                "text": plain_message(payload),
            }),
        ),
        NotificationChannelConfig::Desktop { .. } => {
            // Local, fire-and-forget: no delivery semantics to report.
            send_desktop_notification(payload);
            Ok(())
        }
    }
}

/// Single-line human rendering used by the chat presets.
pub fn plain_message(payload: &super::hooks::LifecycleHookPayload) -> String {
    let location = if payload.project.is_empty() {
        String::new()
    } else {
        format!(" [{}]", payload.project)
    };
    let task = if payload.task_id.is_empty() {
        String::new()
    } else {
        format!(" ({})", payload.task_id)
    };
    format!(
        "openloop{location}{task}: {message}",
        message = payload.message
    )
}

/// Telegram sendMessage endpoint for a bot token (unit-testable).
pub fn telegram_endpoint(bot_token: &str) -> String {
    format!("https://api.telegram.org/bot{bot_token}/sendMessage")
}

/// Stable identity for a channel within the global config; used as the
/// delivery-ledger and retry-queue key.
pub fn channel_key(channel: &NotificationChannelConfig) -> String {
    match channel {
        NotificationChannelConfig::Webhook { url, .. } => format!("webhook:{url}"),
        NotificationChannelConfig::Slack { url, .. } => format!("slack:{url}"),
        NotificationChannelConfig::Discord { url, .. } => format!("discord:{url}"),
        NotificationChannelConfig::Telegram { chat_id, .. } => {
            format!("telegram:{chat_id}")
        }
        NotificationChannelConfig::Desktop { .. } => "desktop".to_string(),
    }
}

fn send_json(url: &str, body: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string(body).unwrap_or_default();
    match ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .post(url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "openloop-daemon/1.0")
        .send_string(&text)
    {
        Ok(response) if response.status() < 300 => Ok(()),
        Ok(response) => Err(format!("HTTP {}", response.status())),
        Err(error) => Err(error.to_string()),
    }
}

fn send_desktop_notification(payload: &super::hooks::LifecycleHookPayload) {
    let title = format!("OpenLoop: {}", payload.event);
    let body = &payload.message;
    let (command, args): (&str, Vec<String>) = if cfg!(target_os = "macos") {
        (
            "osascript",
            vec![
                "-e".to_string(),
                format!(
                    "display notification \"{}\" with title \"{}\"",
                    body.replace('"', "\\\""),
                    title.replace('"', "\\\"")
                ),
            ],
        )
    } else {
        // linux (and unsupported platforms just fail silently below)
        ("notify-send", vec![title, body.clone()])
    };
    let _ = std::process::Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

// ---------------------------------------------------------------------------
// Delivery ledger + bounded retry
// ---------------------------------------------------------------------------

const MAX_QUEUE: usize = 20;
const MAX_ATTEMPTS: u32 = 3;
/// Backoff after the 1st and 2nd failed attempts.
const RETRY_BACKOFF_SECONDS: [u64; 2] = [60, 300];

fn record_delivery_status(channel_key: &str, ok: bool, error: Option<&str>, event: Option<&str>) {
    let entry = NotificationDeliveryStatus {
        channel_key: channel_key.to_string(),
        ok,
        at: now_iso(),
        event: event.map(|e| e.to_string()),
        error: error.map(|e| e.to_string()),
    };
    let _ = super::daemon_state::with_daemon_state(
        |state| {
            let statuses = state.notification_status.get_or_insert_with(Vec::new);
            statuses.retain(|s| s.channel_key != channel_key);
            statuses.push(entry.clone());
        },
        None,
    );
}

fn enqueue_retry(channel_key: &str, payload: &super::hooks::LifecycleHookPayload, error: &str) {
    let entry = QueuedNotification {
        channel_key: channel_key.to_string(),
        payload_json: serde_json::to_string(payload).unwrap_or_default(),
        attempts: 1,
        next_attempt_at: iso_after_seconds(RETRY_BACKOFF_SECONDS[0]),
        last_error: Some(error.to_string()),
    };
    let _ = super::daemon_state::with_daemon_state(
        |state| {
            let queue = state.notification_queue.get_or_insert_with(Vec::new);
            queue.push(entry.clone());
            if queue.len() > MAX_QUEUE {
                let dropped = queue.len() - MAX_QUEUE;
                queue.drain(0..dropped);
            }
        },
        None,
    );
}

/// Retry due queued deliveries. Called once per daemon tick; safe to call
/// from tests directly. Returns the number of deliveries retried.
pub fn drain_notification_queue(config: &GlobalConfig) -> usize {
    let now_ms = super::now_millis();
    let due: Vec<QueuedNotification> = match super::daemon_state::load_daemon_state(None) {
        Ok(state) => state
            .notification_queue
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| {
                chrono::DateTime::parse_from_rfc3339(&entry.next_attempt_at)
                    .map(|dt| dt.timestamp_millis() <= now_ms)
                    .unwrap_or(true) // unreadable timestamp: retry rather than strand
            })
            .collect(),
        Err(_) => return 0,
    };
    if due.is_empty() {
        return 0;
    }

    let channels = config.notification_channels.clone().unwrap_or_default();
    let mut processed = 0;

    for entry in due {
        processed += 1;
        let payload: super::hooks::LifecycleHookPayload =
            match serde_json::from_str(&entry.payload_json) {
                Ok(p) => p,
                Err(_) => {
                    // Unparseable queued payload: drop it (corrupt beyond retry).
                    remove_queued(&entry.channel_key);
                    continue;
                }
            };
        let Some(channel) = channels
            .iter()
            .find(|c| channel_key(c) == entry.channel_key)
        else {
            // Channel removed from config: drop the queued delivery.
            remove_queued(&entry.channel_key);
            record_delivery_status(
                &entry.channel_key,
                false,
                Some("channel removed from config"),
                Some(&payload.event),
            );
            continue;
        };
        match deliver(channel, &payload) {
            Ok(()) => {
                record_delivery_status(&entry.channel_key, true, None, Some(&payload.event));
                remove_queued(&entry.channel_key);
            }
            Err(error) => {
                let attempts = entry.attempts + 1;
                if attempts >= MAX_ATTEMPTS {
                    record_delivery_status(
                        &entry.channel_key,
                        false,
                        Some(&format!("giving up after {attempts} attempts: {error}")),
                        Some(&payload.event),
                    );
                    remove_queued(&entry.channel_key);
                } else {
                    let backoff = RETRY_BACKOFF_SECONDS
                        .get((attempts - 1) as usize)
                        .copied()
                        .unwrap_or(RETRY_BACKOFF_SECONDS[1]);
                    let _ = super::daemon_state::with_daemon_state(
                        |state| {
                            if let Some(queue) = state.notification_queue.as_mut() {
                                for queued in queue.iter_mut() {
                                    if queued.channel_key == entry.channel_key {
                                        queued.attempts = attempts;
                                        queued.next_attempt_at = iso_after_seconds(backoff);
                                        queued.last_error = Some(error.clone());
                                    }
                                }
                            }
                        },
                        None,
                    );
                }
            }
        }
    }
    processed
}

fn remove_queued(channel_key: &str) {
    let _ = super::daemon_state::with_daemon_state(
        |state| {
            if let Some(queue) = state.notification_queue.as_mut() {
                queue.retain(|entry| entry.channel_key != channel_key);
                if queue.is_empty() {
                    state.notification_queue = None;
                }
            }
        },
        None,
    );
}

fn iso_after_seconds(seconds: u64) -> String {
    chrono::Utc
        .timestamp_millis_opt(super::now_millis() + (seconds as i64) * 1000)
        .single()
        .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_else(now_iso)
}

impl NotificationChannelConfig {
    pub fn events(&self) -> &Vec<String> {
        match self {
            NotificationChannelConfig::Webhook { events, .. }
            | NotificationChannelConfig::Slack { events, .. }
            | NotificationChannelConfig::Discord { events, .. }
            | NotificationChannelConfig::Telegram { events, .. }
            | NotificationChannelConfig::Desktop { events } => events,
        }
    }

    pub fn channel_type(&self) -> &'static str {
        match self {
            NotificationChannelConfig::Webhook { .. } => "webhook",
            NotificationChannelConfig::Slack { .. } => "slack",
            NotificationChannelConfig::Discord { .. } => "discord",
            NotificationChannelConfig::Telegram { .. } => "telegram",
            NotificationChannelConfig::Desktop { .. } => "desktop",
        }
    }
}
