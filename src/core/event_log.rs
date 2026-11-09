use chrono::TimeZone;
use std::io::Write;
use std::path::PathBuf;

use super::fsutil::{ensure_dir, rotate_log_file};
use super::now_iso;
use super::now_millis;
use super::paths::app_home;
use super::types::OpenLoopEvent;

pub fn events_log_path(override_home: Option<&str>) -> PathBuf {
    app_home(override_home).join("run").join("events.jsonl")
}

// Rotate events.jsonl at 10 MiB, keeping .1–.3 (same policy as daemon.log).
const EVENT_LOG_ROTATE_MAX_BYTES: u64 = 10 * 1024 * 1024;
const EVENT_LOG_ROTATE_BACKUPS: usize = 3;

pub fn append_event(event: &OpenLoopEvent, override_home: Option<&str>) -> Result<(), String> {
    let log_path = events_log_path(override_home);
    let line = format!("{}\n", serde_json::to_string(event).unwrap_or_default());
    if let Some(parent) = log_path.parent() {
        let _ = ensure_dir(parent);
    }
    rotate_log_file(
        &log_path,
        EVENT_LOG_ROTATE_MAX_BYTES,
        EVENT_LOG_ROTATE_BACKUPS,
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = file.write_all(line.as_bytes());
    }
    Ok(())
}

pub fn append_event_lossy(event: &OpenLoopEvent) {
    let _ = append_event(event, None);
}

#[derive(Debug, Clone, Default)]
pub struct ReadEventsOptions<'a> {
    pub override_home: Option<&'a str>,
    pub since_ms: Option<i64>,
    pub project: Option<&'a str>,
    pub limit: Option<usize>,
}

pub fn read_recent_events(options: &ReadEventsOptions<'_>) -> Vec<OpenLoopEvent> {
    let log_path = events_log_path(options.override_home);
    // TS parity: read the whole file (bounded at 10 MiB by rotation), filter
    // by since/project, and only then keep the last `limit` MATCHING events.
    // A byte-window tail under-reports whenever filters discard windowed
    // lines or events exceed the assumed line size.
    let raw = match std::fs::read_to_string(&log_path) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    // TS truthiness: sinceMs=0 means "no cutoff", not "cutoff at now".
    let cutoff = options.since_ms.filter(|ms| *ms > 0).map(|ms| {
        chrono::Utc
            .timestamp_millis_opt(now_millis() - ms)
            .single()
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
            .unwrap_or_default()
    });

    let mut events = Vec::new();
    for line in raw.split('\n').filter(|l| !l.is_empty()) {
        if let Ok(event) = serde_json::from_str::<OpenLoopEvent>(line) {
            if let Some(cutoff) = &cutoff {
                if event.ts.as_str() < cutoff.as_str() {
                    continue;
                }
            }
            if let Some(project) = options.project {
                if event.project.as_deref() != Some(project) {
                    continue;
                }
            }
            events.push(event);
        }
    }

    if let Some(limit) = options.limit {
        let start = events.len().saturating_sub(limit);
        return events[start..].to_vec();
    }
    events
}

pub fn now_event(event: &str) -> OpenLoopEvent {
    OpenLoopEvent {
        ts: now_iso(),
        event: event.to_string(),
        project: None,
        task_id: None,
        exit_code: None,
        mode: None,
        role: None,
        stopped_by: None,
        pid: None,
        task_status: None,
        detail: None,
        hook_event: None,
        hook_type: None,
        hook_scope: None,
        error: None,
        digest: None,
        budget_snapshot_usd: None,
        pending_promotions: None,
        failed_checks: None,
        extra: Default::default(),
    }
}
