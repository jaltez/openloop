use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct GitWorkingTreeState {
    pub current_branch: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffStatEntry {
    pub file: String,
    pub added: u64,
    pub removed: u64,
}

pub fn run_git(project_path: &Path, args: &[&str]) -> Result<(String, String), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        return Ok((stdout, stderr));
    }
    let code = output.status.code().unwrap_or(1);
    let msg = stderr.trim().to_string();
    if msg.is_empty() {
        Err(format!("git {} failed with exit {code}", args.join(" ")))
    } else {
        Err(msg)
    }
}

pub fn ensure_clean_git_repo(project_path: &Path) -> Result<(), String> {
    let (stdout, _) = run_git(project_path, &["status", "--porcelain"])?;
    if !stdout.trim().is_empty() {
        return Err("Git working tree is dirty.".to_string());
    }
    Ok(())
}

pub fn checkout_handoff_branch(project_path: &Path, branch_name: &str) -> Result<(), String> {
    ensure_clean_git_repo(project_path)?;
    if branch_exists(project_path, branch_name)? {
        run_git(project_path, &["checkout", branch_name]).map(|_| ())
    } else {
        run_git(project_path, &["checkout", "-b", branch_name]).map(|_| ())
    }
}

pub fn get_git_working_tree_state(project_path: &Path) -> Result<GitWorkingTreeState, String> {
    let (branch, _) = run_git(project_path, &["branch", "--show-current"])?;
    let (status, _) = run_git(project_path, &["status", "--porcelain"])?;
    Ok(GitWorkingTreeState {
        current_branch: {
            let b = branch.trim().to_string();
            if b.is_empty() { None } else { Some(b) }
        },
        dirty: !status.trim().is_empty(),
    })
}

pub fn get_git_diff_fingerprint(project_path: &Path) -> Option<String> {
    run_git(project_path, &["status", "--porcelain=v1", "--untracked-files=all"])
        .ok()
        .map(|(stdout, _)| stdout.trim().to_string())
}

pub fn stage_all_changes(project_path: &Path) -> Result<(), String> {
    run_git(project_path, &["add", "-A"]).map(|_| ())
}

pub fn commit_staged_changes(project_path: &Path, message: &str) -> Result<(), String> {
    run_git(project_path, &["commit", "-m", message]).map(|_| ())
}

pub fn checkout_branch(project_path: &Path, branch_name: &str) -> Result<(), String> {
    run_git(project_path, &["checkout", branch_name]).map(|_| ())
}

pub fn merge_fast_forward(project_path: &Path, branch_name: &str) -> Result<(), String> {
    run_git(project_path, &["merge", "--ff-only", branch_name]).map(|_| ())
}

pub fn delete_branch(project_path: &Path, branch_name: &str) -> Result<(), String> {
    run_git(project_path, &["branch", "-D", branch_name]).map(|_| ())
}

pub fn get_branch_head(project_path: &Path, branch_name: &str) -> Result<String, String> {
    run_git(project_path, &["rev-parse", branch_name]).map(|(s, _)| s.trim().to_string())
}

pub fn get_merge_base(project_path: &Path, left: &str, right: &str) -> Result<String, String> {
    run_git(project_path, &["merge-base", left, right]).map(|(s, _)| s.trim().to_string())
}

pub fn branch_exists(project_path: &Path, branch_name: &str) -> Result<bool, String> {
    match run_git(
        project_path,
        &["rev-parse", "--verify", &format!("refs/heads/{branch_name}")],
    ) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

// D5: Git worktree helpers.

pub fn worktree_exists(project_path: &Path, worktree_path: &Path) -> bool {
    match run_git(project_path, &["worktree", "list", "--porcelain"]) {
        Ok((stdout, _)) => stdout.contains(worktree_path.to_string_lossy().as_ref()),
        Err(_) => false,
    }
}

pub fn add_worktree(project_path: &Path, worktree_path: &Path, branch_name: &str) -> Result<(), String> {
    if branch_exists(project_path, branch_name)? {
        run_git(project_path, &["worktree", "add", &worktree_path.to_string_lossy(), branch_name]).map(|_| ())
    } else {
        run_git(
            project_path,
            &["worktree", "add", "-b", branch_name, &worktree_path.to_string_lossy()],
        )
        .map(|_| ())
    }
}

pub fn remove_worktree(project_path: &Path, worktree_path: &Path, force: bool) {
    let mut args: Vec<String> = vec![
        "worktree".into(),
        "remove".into(),
        worktree_path.to_string_lossy().to_string(),
    ];
    if force {
        args.push("--force".into());
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let _ = run_git(project_path, &refs);
    // Prune stale worktree metadata.
    let _ = run_git(project_path, &["worktree", "prune"]);
}

/// Get per-file diff stats between a branch and its base (or HEAD vs base).
/// Returns None if the repo is not a git directory or the base is unavailable.
pub fn get_git_diff_stat(
    project_path: &Path,
    base: Option<&str>,
    head: Option<&str>,
) -> Option<Vec<DiffStatEntry>> {
    let result = (|| -> Result<Vec<DiffStatEntry>, String> {
        let base_ref = base.unwrap_or("HEAD~1").to_string();
        // head === None (explicit) diffs base against the working tree, capturing
        // uncommitted agent edits; a string head compares two refs.
        let (stdout, _) = match head {
            None => run_git(project_path, &["diff", "--numstat", &base_ref])?,
            Some(h) => run_git(project_path, &["diff", "--numstat", &base_ref, h])?,
        };
        let mut entries = Vec::new();
        for line in stdout.trim().split('\n') {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let added = if parts[0] == "-" { 0 } else { parts[0].parse::<u64>().unwrap_or(0) };
            let removed = if parts[1] == "-" { 0 } else { parts[1].parse::<u64>().unwrap_or(0) };
            entries.push(DiffStatEntry {
                file: parts[2].to_string(),
                added,
                removed,
            });
        }
        Ok(entries)
    })();

    match result {
        Ok(mut entries) => {
            // Working-tree mode: `git diff` only sees tracked files. Agent-created
            // files must be visible to the human reviewer too.
            if head.is_none() {
                if let Ok((untracked, _)) = run_git(project_path, &["ls-files", "--others", "--exclude-standard"]) {
                    for file in untracked.trim().split('\n') {
                        if file.is_empty() {
                            continue;
                        }
                        let line_count = count_untracked_file_lines(project_path, file);
                        entries.push(DiffStatEntry {
                            file: file.to_string(),
                            added: line_count,
                            removed: 0,
                        });
                    }
                }
            }
            Some(entries)
        }
        Err(_) => None,
    }
}

fn count_untracked_file_lines(project_path: &Path, file: &str) -> u64 {
    match std::fs::read_to_string(project_path.join(file)) {
        Ok(content) => {
            let lines = content.split('\n').count();
            let count = if content.ends_with('\n') { lines.saturating_sub(1) } else { lines };
            count as u64
        }
        Err(_) => 0,
    }
}

/// Get the full diff patch of uncommitted changes relative to HEAD.
/// Returns None if the repo is not a git directory.
pub fn get_diff_patch(project_path: &Path) -> Option<String> {
    run_git(project_path, &["diff", "HEAD"]).ok().map(|(s, _)| s)
}
