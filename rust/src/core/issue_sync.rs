use std::path::Path;
use std::time::Duration;

use super::fsutil::{read_json_file, write_json_file};
use super::now_iso;
use super::types::*;

const SYNC_LEDGER_FILE: &str = "issue-sync.json";

fn issue_sync_path(project_path: &Path) -> std::path::PathBuf {
    project_path.join(".openloop").join(SYNC_LEDGER_FILE)
}

fn empty_sync_ledger() -> IssueSyncLedger {
    IssueSyncLedger {
        version: 1,
        source: IssueSourceConfig {
            provider: "github".to_string(),
            repo: String::new(),
            label: "openloop".to_string(),
            token: None,
            auto_sync: None,
            sync_interval_minutes: None,
            post_status_comments: None,
            last_synced_at: None,
        },
        issues: Vec::new(),
        updated_at: super::epoch_iso(),
    }
}

pub fn load_issue_sync_ledger(project_path: &Path) -> Result<IssueSyncLedger, String> {
    read_json_file(&issue_sync_path(project_path), empty_sync_ledger())
}

pub fn save_issue_sync_ledger(project_path: &Path, ledger: &mut IssueSyncLedger) -> Result<(), String> {
    ledger.updated_at = now_iso();
    write_json_file(&issue_sync_path(project_path), ledger)
}

#[derive(Debug, Clone)]
struct RemoteIssue {
    number: i64,
    title: String,
    body: String,
    labels: Vec<String>,
    state: String,
    url: String,
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()
}

pub fn fetch_github_issues(repo: &str, label: &str, token: Option<&str>) -> Result<Vec<RemoteIssue>, String> {
    let encoded_label = urlencode(label);
    let url = format!(
        "https://api.github.com/repos/{repo}/issues?labels={encoded_label}&state=open&per_page=100"
    );
    let mut request = http_agent()
        .get(&url)
        .set("User-Agent", "openloop-cli")
        .set("Accept", "application/vnd.github+json");
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    let response = request.call().map_err(|e| match e {
        ureq::Error::Status(code, resp) => {
            let mut body = String::new();
            use std::io::Read;
            let _ = resp.into_reader().take(200).read_to_string(&mut body);
            format!("GitHub API returned {code}: {body}")
        }
        other => format!("GitHub API request failed: {other}"),
    })?;

    let data: Vec<serde_json::Value> = response
        .into_json()
        .map_err(|e| format!("GitHub API returned unparseable body: {e}"))?;

    Ok(data
        .iter()
        .filter(|issue| issue.get("pull_request").is_none())
        .map(|issue| RemoteIssue {
            number: issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0),
            title: issue
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            body: issue
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            labels: issue
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| l.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            state: if issue.get("state").and_then(|v| v.as_str()) == Some("open") {
                "open".to_string()
            } else {
                "closed".to_string()
            },
            url: issue
                .get("html_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
        .collect())
}

pub fn fetch_gitlab_issues(repo: &str, label: &str, token: Option<&str>) -> Result<Vec<RemoteIssue>, String> {
    let encoded_project = urlencode(repo);
    let encoded_label = urlencode(label);
    let url = format!(
        "https://gitlab.com/api/v4/projects/{encoded_project}/issues?labels={encoded_label}&state=opened&per_page=100"
    );
    let mut request = http_agent().get(&url).set("User-Agent", "openloop-cli");
    if let Some(token) = token {
        request = request.set("PRIVATE-TOKEN", token);
    }
    let response = request.call().map_err(|e| match e {
        ureq::Error::Status(code, resp) => {
            let mut body = String::new();
            use std::io::Read;
            let _ = resp.into_reader().take(200).read_to_string(&mut body);
            format!("GitLab API returned {code}: {body}")
        }
        other => format!("GitLab API request failed: {other}"),
    })?;

    let data: Vec<serde_json::Value> = response
        .into_json()
        .map_err(|e| format!("GitLab API returned unparseable body: {e}"))?;

    Ok(data
        .iter()
        .map(|issue| RemoteIssue {
            number: issue.get("iid").and_then(|v| v.as_i64()).unwrap_or(0),
            title: issue
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            body: issue
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            labels: issue
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| l.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            state: if issue.get("state").and_then(|v| v.as_str()) == Some("opened") {
                "open".to_string()
            } else {
                "closed".to_string()
            },
            url: issue
                .get("web_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
        .collect())
}

pub fn post_github_comment(repo: &str, issue_number: i64, body: &str, token: &str) -> Result<(), String> {
    let url = format!("https://api.github.com/repos/{repo}/issues/{issue_number}/comments");
    let response = http_agent()
        .post(&url)
        .set("User-Agent", "openloop-cli")
        .set("Accept", "application/vnd.github+json")
        .set("Authorization", &format!("Bearer {token}"))
        .send_json(serde_json::json!({ "body": body }))
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let mut body = String::new();
                use std::io::Read;
                let _ = resp.into_reader().take(200).read_to_string(&mut body);
                format!("GitHub comment failed ({code}): {body}")
            }
            other => format!("GitHub comment failed: {other}"),
        })?;
    // TS parity: only HTTP 201 counts as success.
    if response.status() != 201 {
        return Err(format!("GitHub comment failed ({}): expected 201", response.status()));
    }
    Ok(())
}

pub fn post_gitlab_comment(repo: &str, issue_number: i64, body: &str, token: &str) -> Result<(), String> {
    let encoded_project = urlencode(repo);
    let url = format!("https://gitlab.com/api/v4/projects/{encoded_project}/issues/{issue_number}/notes");
    let response = http_agent()
        .post(&url)
        .set("User-Agent", "openloop-cli")
        .set("PRIVATE-TOKEN", token)
        .send_json(serde_json::json!({ "body": body }))
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let mut body = String::new();
                use std::io::Read;
                let _ = resp.into_reader().take(200).read_to_string(&mut body);
                format!("GitLab comment failed ({code}): {body}")
            }
            other => format!("GitLab comment failed: {other}"),
        })?;
    // TS parity: only HTTP 201 counts as success.
    if response.status() != 201 {
        return Err(format!("GitLab comment failed ({}): expected 201", response.status()));
    }
    Ok(())
}

fn urlencode(value: &str) -> String {
    // Minimal percent-encoder for the characters we emit (labels, project paths).
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn issue_to_task_id(issue: &RemoteIssue) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in issue.title.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').chars().take(50).collect::<String>();
    if slug.is_empty() {
        format!("issue-{}", issue.number)
    } else {
        format!("issue-{}-{slug}", issue.number)
    }
}

fn guess_issue_kind(issue: &RemoteIssue) -> TaskKind {
    let lower = format!("{} {}", issue.title, issue.body).to_lowercase();
    if lower.contains("bug") || lower.contains("fix") {
        return TaskKind::Bugfix;
    }
    if lower.contains("test") {
        return TaskKind::Test;
    }
    if lower.contains("refactor") {
        return TaskKind::Refactor;
    }
    if lower.contains("docs") || lower.contains("documentation") {
        return TaskKind::Docs;
    }
    if lower.contains("lint") {
        return TaskKind::LintFix;
    }
    if lower.contains("type") {
        return TaskKind::TypeFix;
    }
    TaskKind::Feature
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SyncResult {
    pub imported: usize,
    pub skipped: usize,
    pub total: usize,
    pub errors: Vec<String>,
}

pub fn sync_issues(project_path: &Path, source: &IssueSourceConfig) -> Result<SyncResult, String> {
    let remote_issues = if source.provider == "github" {
        fetch_github_issues(&source.repo, &source.label, source.token.as_deref())?
    } else {
        fetch_gitlab_issues(&source.repo, &source.label, source.token.as_deref())?
    };

    let mut sync_ledger = load_issue_sync_ledger(project_path)?;
    sync_ledger.source = source.clone();

    let mut result = SyncResult {
        imported: 0,
        skipped: 0,
        total: remote_issues.len(),
        errors: Vec::new(),
    };

    for issue in &remote_issues {
        if sync_ledger.issues.iter().any(|synced| synced.number == issue.number) {
            result.skipped += 1;
            continue;
        }

        let task_id = issue_to_task_id(issue);
        let now = now_iso();
        let provider_prefix = if source.provider == "github" { "github" } else { "gitlab" };

        let mut task = ProjectTask {
            id: task_id.clone(),
            title: issue.title.clone(),
            kind: guess_issue_kind(issue),
            status: TaskStatus::Proposed,
            risk: TaskRisk::MediumRisk,
            scope: None,
            source: TaskSource {
                source_type: "issue".to_string(),
                r#ref: format!("{provider_prefix}#{}", issue.number),
            },
            spec_id: None,
            branch: None,
            owner: Some("openloop".to_string()),
            acceptance_criteria: if !issue.body.is_empty() {
                vec![format!("From issue: {}", issue.url), issue.body.chars().take(500).collect()]
            } else {
                vec![format!("From issue: {}", issue.url)]
            },
            attempts: 0,
            last_failure_signature: None,
            promotion: TaskPromotion::PullRequest,
            promoted_at: None,
            depends_on: None,
            estimated_cost_usd: None,
            notes: Some(vec![format!("Synced from {provider_prefix} issue #{}", issue.number)]),
            last_run: None,
            created_at: now.clone(),
            updated_at: now,
        };

        match super::task_ledger::add_task(project_path, &mut task) {
            Ok(()) => {
                sync_ledger.issues.push(SyncedIssue {
                    number: issue.number,
                    title: issue.title.clone(),
                    body: issue.body.chars().take(1000).collect(),
                    labels: issue.labels.clone(),
                    state: issue.state.clone(),
                    url: issue.url.clone(),
                    task_id: Some(task_id.clone()),
                    synced_at: now_iso(),
                });
                result.imported += 1;
            }
            Err(err) => {
                result.errors.push(format!("Issue #{}: {err}", issue.number));
            }
        }
    }

    save_issue_sync_ledger(project_path, &mut sync_ledger)?;

    // Update last synced timestamp on project config
    let mut project_config = super::project_config::load_project_config(project_path)?;
    if project_config.issue_source.is_some() {
        project_config.issue_source.as_mut().unwrap().last_synced_at = Some(now_iso());
        super::project_config::save_project_config(project_path, &project_config)?;
    }

    Ok(result)
}

pub fn post_task_status_to_issue(
    project_path: &Path,
    task_id: &str,
    status: &str,
    details: Option<&str>,
) -> Result<(), String> {
    let sync_ledger = load_issue_sync_ledger(project_path)?;
    let Some(synced_issue) = sync_ledger.issues.iter().find(|i| i.task_id.as_deref() == Some(task_id)) else {
        return Ok(());
    };

    let source = &sync_ledger.source;
    if source.token.is_none() || source.post_status_comments.unwrap_or(false) == false {
        return Ok(());
    }
    let Some(token) = source.token.as_deref() else {
        return Ok(());
    };

    let emoji = match status {
        "done" => "✅",
        "promoted" => "🚀",
        "failed" => "❌",
        "blocked" => "⚠️",
        "in_progress" => "🔄",
        "ready" => "📋",
        "planned" => "📝",
        _ => "ℹ️",
    };
    let details_part = details.map(|d| format!("\n{d}")).unwrap_or_default();
    let body = format!(
        "{emoji} **OpenLoop Update** — Task status: `{status}`{details_part}\n_Updated by [OpenLoop](https://github.com/openloop) daemon_"
    );

    if source.provider == "github" {
        post_github_comment(&source.repo, synced_issue.number, &body, token)
    } else {
        post_gitlab_comment(&source.repo, synced_issue.number, &body, token)
    }
}

/// Post a PR link comment to the originating issue when a task creates a PR.
pub fn post_pr_link_to_issue(
    project_path: &Path,
    task_id: &str,
    pr_url: &str,
    branch: Option<&str>,
) -> Result<(), String> {
    let sync_ledger = load_issue_sync_ledger(project_path)?;
    let Some(synced_issue) = sync_ledger.issues.iter().find(|i| i.task_id.as_deref() == Some(task_id)) else {
        return Ok(());
    };

    let source = &sync_ledger.source;
    if source.token.is_none() || source.post_status_comments.unwrap_or(false) == false {
        return Ok(());
    }
    let Some(token) = source.token.as_deref() else {
        return Ok(());
    };

    let branch_part = branch.map(|b| format!("\nBranch: `{b}`")).unwrap_or_default();
    let body = format!(
        "🔗 **OpenLoop PR** — A pull request was created for this issue{branch_part}\nPR: {pr_url}\n_Created by [OpenLoop](https://github.com/openloop) daemon_"
    );

    if source.provider == "github" {
        post_github_comment(&source.repo, synced_issue.number, &body, token)
    } else {
        post_gitlab_comment(&source.repo, synced_issue.number, &body, token)
    }
}

pub fn list_synced_issues(project_path: &Path) -> Result<Vec<SyncedIssue>, String> {
    Ok(load_issue_sync_ledger(project_path)?.issues)
}
