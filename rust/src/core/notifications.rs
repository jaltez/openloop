use std::time::Duration;

use super::types::{GlobalConfig, NotificationChannelConfig};


/// Fire notifications through all configured channels.
/// Shell-command hooks (legacy) are handled separately in the worker.
pub fn fire_notifications(config: &GlobalConfig, payload: &super::hooks::LifecycleHookPayload) -> Result<(), String> {
    let channels = config.notification_channels.clone().unwrap_or_default();
    for channel in channels {
        let matches_event = channel
            .events()
            .is_empty()
            || channel.events().iter().any(|e| e == &payload.event || e == "*");
        if !matches_event {
            continue;
        }
        match channel {
            NotificationChannelConfig::Webhook { url, .. } => {
                let _ = send_webhook(&url, payload);
            }
            NotificationChannelConfig::Desktop { .. } => {
                send_desktop_notification(payload);
            }
        }
    }
    Ok(())
}

fn send_webhook(url: &str, payload: &super::hooks::LifecycleHookPayload) -> Result<(), String> {
    let body = serde_json::to_string(payload).unwrap_or_default();
    // Best-effort: errors and timeouts resolve silently (TS parity).
    let _ = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .post(url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "openloop-daemon/1.0")
        .send_string(&body);
    Ok(())
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

impl NotificationChannelConfig {
    pub fn events(&self) -> &Vec<String> {
        match self {
            NotificationChannelConfig::Webhook { events, .. } => events,
            NotificationChannelConfig::Desktop { events } => events,
        }
    }

    pub fn channel_type(&self) -> &'static str {
        match self {
            NotificationChannelConfig::Webhook { .. } => "webhook",
            NotificationChannelConfig::Desktop { .. } => "desktop",
        }
    }
}
