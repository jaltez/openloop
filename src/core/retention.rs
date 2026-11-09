use std::path::Path;

use super::types::ProjectConfig;

/// Enforce a project's control-plane retention: keep only the newest N run
/// artifacts (summaries + transcripts share a run timestamp) and the newest
/// N promotion-result artifacts per task. The event log and the promotions/
/// queue artifacts are never touched. Returns (runs pruned, results pruned).
pub fn enforce_retention(
    project_path: &Path,
    config: &ProjectConfig,
) -> Result<(usize, usize), String> {
    let policy = config.retention.clone().unwrap_or_default();
    let runs_dir = project_path.join(".openloop").join("runs");
    let results_dir = project_path.join(".openloop").join("promotion-results");

    let runs_pruned = prune_newest_per_task(&runs_dir, policy.max_run_artifacts_per_task as usize)?;
    let results_pruned =
        prune_newest_per_task(&results_dir, policy.max_promotion_results_per_task as usize)?;
    Ok((runs_pruned, results_pruned))
}

/// Files in these directories are named `{timestamp}-{task-id}{suffix}` with
/// the timestamp's `Z` ending the time part (e.g.
/// `2026-01-02T00-00-00-000Z-fix-login.md`). One "run" per task is the set of
/// files sharing a timestamp; keep the newest `keep` runs per task.
fn prune_newest_per_task(dir: &Path, keep: usize) -> Result<usize, String> {
    if !dir.is_dir() {
        return Ok(0);
    }
    // (task, timestamp, file name) for openloop-written artifacts only.
    let mut grouped: std::collections::HashMap<
        String,
        std::collections::BTreeMap<String, Vec<String>>,
    > = std::collections::HashMap::new();
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_artifact =
            name.ends_with(".json") || name.ends_with(".md") || name.ends_with(".log");
        if !entry.metadata().map(|m| m.is_file()).unwrap_or(false) || !is_artifact {
            continue;
        }
        let Some((timestamp, task)) = split_run_filename(&name) else {
            continue; // foreign file: never touch
        };
        grouped
            .entry(task)
            .or_default()
            .entry(timestamp)
            .or_default()
            .push(name);
    }

    // BTreeMap keys sort ascending by timestamp; newest runs are at the end.
    let mut pruned = 0;
    for (_task, runs) in grouped {
        let excess: Vec<&String> = runs.keys().rev().skip(keep).collect();
        for timestamp in excess {
            if let Some(files) = runs.get(timestamp) {
                for file in files {
                    if std::fs::remove_file(dir.join(file)).is_ok() {
                        pruned += 1;
                    }
                }
            }
        }
    }
    Ok(pruned)
}

/// Split `2026-01-02T00-00-00-000Z-task-id.md` into
/// ("2026-01-02T00-00-00-000Z", "task-id"). Returns None when the name does
/// not carry the timestamp marker (foreign files are left alone).
fn split_run_filename(name: &str) -> Option<(String, String)> {
    let marker = name.find('Z')?;
    let rest = &name[marker + 1..];
    let task = rest.strip_prefix('-')?;
    if task.is_empty() || marker < 10 {
        return None;
    }
    Some((name[..=marker].to_string(), task.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_timestamp_and_task() {
        assert_eq!(
            split_run_filename("2026-01-02T00-00-00-000Z-fix-login.md"),
            Some(("2026-01-02T00-00-00-000Z".into(), "fix-login.md".into()))
        );
        assert_eq!(
            split_run_filename("2026-01-02T00-00-00-000Z-fix-login.transcript.log"),
            Some((
                "2026-01-02T00-00-00-000Z".into(),
                "fix-login.transcript.log".into()
            ))
        );
        // Foreign names never match: no Z timestamp marker.
        assert_eq!(split_run_filename("README.md"), None);
        assert_eq!(split_run_filename("2026-01-02T00-00-00-000-fix.md"), None);
    }
}
