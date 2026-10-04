use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::now_millis;

pub fn ensure_dir(dir_path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir_path)
}

pub fn file_exists(file_path: &Path) -> bool {
    fs::metadata(file_path).is_ok()
}

/// Read JSON with a fallback used when the file does not exist. Mirrors
/// readJsonFile semantics: a corrupt file is a hard error.
pub fn read_json_file<T: DeserializeOwned>(file_path: &Path, fallback: T) -> Result<T, String> {
    if !file_exists(file_path) {
        return Ok(fallback);
    }
    // TS parity: I/O failures (EACCES, EISDIR, ...) surface as read errors;
    // only parse failures carry the "back up or delete" recovery advice.
    let raw = fs::read_to_string(file_path)
        .map_err(|e| format!("Cannot read {}: {e}", file_path.display()))?;
    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "Corrupt JSON in {}. Back up or delete the file and re-run. Original content preserved at the same path. ({e})",
            file_path.display()
        )
    })
}

/// Read JSON tolerantly: missing file OR parse failure both yield None.
pub fn read_json_opt<T: DeserializeOwned>(file_path: &Path) -> Option<T> {
    let raw = fs::read_to_string(file_path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Pretty JSON payload identical to `JSON.stringify(value, null, 2) + "\n"`.
pub fn json_payload<T: Serialize>(value: &T) -> String {
    // JSON.stringify renders integral floats as integers ("1", not "1.0");
    // normalize the value tree before serializing for byte parity with the
    // TS-era state files.
    let mut raw = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    normalize_integral_floats(&mut raw);
    let mut s = serde_json::to_string_pretty(&raw).unwrap_or_else(|_| "{}".to_string());
    s.push('\n');
    s
}

/// Rewrite every finite f64 with a zero fraction as a u64 JSON number.
fn normalize_integral_floats(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Number(number) => {
            if let Some(f) = number.as_f64() {
                if f.is_finite() && f.fract() == 0.0 && f.abs() <= 9.007_199_254_740_992e15 {
                    *number = serde_json::Number::from(f as i64);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(normalize_integral_floats),
        serde_json::Value::Object(map) => map.values_mut().for_each(normalize_integral_floats),
        _ => {}
    }
}

/// Atomic write: serialize to a sibling temp file, then rename over the
/// target — concurrent readers never observe torn or partial JSON.
pub fn write_json_file<T: Serialize>(file_path: &Path, value: &T) -> Result<(), String> {
    write_atomic(file_path, json_payload(value).as_bytes())
}

pub fn write_atomic(file_path: &Path, payload: &[u8]) -> Result<(), String> {
    let parent = file_path.parent().unwrap_or(Path::new("."));
    ensure_dir(parent).map_err(|e| format!("ensure_dir {}: {e}", parent.display()))?;
    let pid = std::process::id();
    let rand: u32 = (now_millis() as u32)
        .wrapping_mul(2654435761)
        .rotate_left(13);
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let temp_path: PathBuf = file_path.with_file_name(format!("{file_name}.{pid}.{rand:08x}.tmp"));
    let result = (|| -> Result<(), String> {
        let mut f = fs::File::create(&temp_path).map_err(|e| e.to_string())?;
        f.write_all(payload).map_err(|e| e.to_string())?;
        drop(f);
        fs::rename(&temp_path, file_path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = &result {
        let _ = fs::remove_file(&temp_path);
        return Err(e.clone());
    }
    // Opportunistic sweep: a crash between the temp write and the rename
    // orphans `*.tmp` files; clean up any older than an hour.
    sweep_stale_temp_files(file_path);
    Ok(())
}

fn sweep_stale_temp_files(file_path: &Path) {
    let dir = match file_path.parent() {
        Some(d) => d,
        None => return,
    };
    let target_prefix = format!(
        "{}.",
        file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let cutoff = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64 - 60 * 60 * 1000)
        .unwrap_or(0);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&target_prefix) || !name.ends_with(".tmp") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if mtime < cutoff {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Rotate a log file once it exceeds max_bytes: current becomes `.1`, existing
/// `.N` shift up, and the oldest (`.<keep>`) is dropped. Non-fatal on failure.
pub fn rotate_log_file(log_path: &Path, max_bytes: u64, keep: usize) {
    let stat = match fs::metadata(log_path) {
        Ok(s) => s,
        Err(_) => return,
    };
    if stat.len() < max_bytes {
        return;
    }
    for index in (1..keep).rev() {
        let from = PathBuf::from(format!("{}.{}", log_path.display(), index));
        let to = PathBuf::from(format!("{}.{}", log_path.display(), index + 1));
        let _ = fs::rename(&from, &to);
    }
    let _ = fs::rename(log_path, PathBuf::from(format!("{}.1", log_path.display())));
}

pub fn copy_tree(source_dir: &Path, target_dir: &Path, overwrite: bool) -> Result<(), String> {
    ensure_dir(target_dir).map_err(|e| e.to_string())?;
    let entries =
        fs::read_dir(source_dir).map_err(|e| format!("read {}: {e}", source_dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let source_path = entry.path();
        let target_path = target_dir.join(&name);
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            copy_tree(&source_path, &target_path, overwrite)?;
            continue;
        }
        if !overwrite && file_exists(&target_path) {
            continue;
        }
        if let Some(parent) = target_path.parent() {
            let _ = ensure_dir(parent);
        }
        fs::copy(&source_path, &target_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Read at most the last `max_bytes` of a file (aligned to a line boundary).
pub fn read_file_tail(path: &Path, max_bytes: u64) -> Result<String, std::io::Error> {
    use std::io::{Read, Seek, SeekFrom};
    use std::os::unix::fs::MetadataExt;
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.size();
    if size <= max_bytes {
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        return Ok(buf);
    }
    file.seek(SeekFrom::Start(size - max_bytes))?;
    let mut raw = Vec::with_capacity(max_bytes as usize);
    file.read_to_end(&mut raw)?;
    // Drop the (likely partial) first line so parsing starts on a boundary.
    let text = String::from_utf8_lossy(&raw);
    Ok(match text.find('\n') {
        Some(idx) => text[idx + 1..].to_string(),
        None => text.to_string(),
    })
}

/// Lexically normalize a path (resolve `.` and `..` without touching the
/// filesystem), mirroring TS path.resolve's normalization step.
pub fn normalize_lexical_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}
