use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Cross-process file lock. The lock file's content is an ownership token: a
/// holder only ever unlinks a lock whose token still matches its own, so a
/// stale-break or a delayed release can never delete a newer holder's lock.
pub fn with_file_lock<T>(
    lock_path: &Path,
    label: &str,
    critical: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    const DEFAULT_STALE_MS: i64 = 10 * 60 * 1000;
    const DEFAULT_RETRIES: u32 = 60;
    const DEFAULT_RETRY_DELAY_MS: u64 = 50;

    if let Some(parent) = lock_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let rand: u32 = (std::process::id() as u32)
        .wrapping_mul(2654435761)
        .rotate_left(13)
        ^ (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0));
    let token = format!("{}.{:08x}", std::process::id(), rand);
    acquire(lock_path, &token, label, DEFAULT_STALE_MS, DEFAULT_RETRIES, DEFAULT_RETRY_DELAY_MS)?;

    let result = critical();
    // Release: only unlink when the token still matches ours.
    if let Ok(current) = fs::read_to_string(lock_path) {
        if current.trim() == token {
            let _ = fs::remove_file(lock_path);
        }
    }
    result
}

fn acquire(
    lock_path: &Path,
    token: &str,
    label: &str,
    stale_ms: i64,
    retries: u32,
    retry_delay_ms: u64,
) -> Result<(), String> {
    for attempt in 0..=retries {
        // O_EXCL create.
        match fs::OpenOptions::new().write(true).create_new(true).open(lock_path) {
            Ok(mut f) => {
                use std::io::Write;
                let _ = writeln!(f, "{token}");
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Break locks whose holder is long gone (crashed mid-critical).
                // Re-stat right before removing so a freshly re-acquired lock
                // (same path, different token) is never deleted by this contender.
                let stat = fs::metadata(lock_path).ok();
                if let Some(stat) = stat {
                    let mtime = stat
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    if now - mtime > stale_ms {
                        let holder_token = fs::read_to_string(lock_path).ok();
                        let still_stale = fs::metadata(lock_path).ok().and_then(|s2| {
                            s2.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        });
                        let still_mtime = still_stale.map(|d| d.as_millis() as i64).unwrap_or(-1);
                        if still_mtime == mtime && holder_token.is_some() {
                            let _ = fs::remove_file(lock_path);
                        }
                        continue;
                    }
                }
                if attempt == retries {
                    break;
                }
                std::thread::sleep(Duration::from_millis(retry_delay_ms));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!("{label} is locked by another openloop process"))
}
