use std::time::{Duration, Instant};

use super::types::RunTimeoutError;

/// Run a fallible operation under a wall-clock deadline. `timeout_ms` of None
/// disables the deadline; <= 0 fails immediately (matching withTimeout).
pub fn with_timeout<T>(
    timeout_ms: Option<u64>,
    message: &str,
    op: impl FnOnce(Option<Instant>) -> Result<T, String>,
) -> Result<T, TimeoutOutcome> {
    let timeout_ms = match timeout_ms {
        None => return op(None).map_err(TimeoutOutcome::Other),
        Some(ms) => ms,
    };
    if timeout_ms == 0 {
        return Err(TimeoutOutcome::Timeout(RunTimeoutError(message.to_string())));
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    op(Some(deadline)).map_err(TimeoutOutcome::Other)
}

pub enum TimeoutOutcome {
    Timeout(RunTimeoutError),
    Other(String),
}
