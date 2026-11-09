pub mod approval_packets;
pub mod cron;
pub mod daemon_state;
pub mod dashboard;
pub mod digest;
pub mod doctor;
pub mod event_log;
pub mod file_lock;
pub mod fsutil;
pub mod git;
pub mod global_config;
pub mod hooks;
pub mod issue_sync;
pub mod mcp_server;
pub mod model_resolution;
pub mod notifications;
pub mod paths;
pub mod pi;
pub mod pr_refresh;
pub mod project_config;
pub mod project_policy;
pub mod project_registry;
pub mod project_selection;
pub mod promotion_artifacts;
pub mod promotion_queue;
pub mod providers;
pub mod retention;
pub mod review;
pub mod run_report;
pub mod run_summaries;
pub mod scheduler;
pub mod service_control;
pub mod stack_detection;
pub mod table;
pub mod task_inspection;
pub mod task_ledger;
pub mod templates;
pub mod types;
pub mod validation;
pub mod validation_utils;

pub mod active_runs;

pub mod worker;

use chrono::{Local, Utc};

/// ISO-8601 UTC timestamp with millisecond precision, matching JS
/// `new Date().toISOString()` ("2026-10-01T12:34:56.789Z").
pub fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn epoch_iso() -> String {
    "1970-01-01T00:00:00.000Z".to_string()
}

/// Local calendar date stamp (YYYY-MM-DD), matching daemon-state.ts.
pub fn local_date_stamp() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// JS `Date.now()`.
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Parse an ISO timestamp into epoch millis; returns 0 on failure.
pub fn parse_iso_millis(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}
