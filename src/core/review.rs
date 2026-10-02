use std::path::Path;

use super::fsutil::{ensure_dir, file_exists};
use super::git::get_diff_patch;
use super::project_policy::{matches_any_glob, normalize_scope_path};
use super::types::*;

// ---------------------------------------------------------------------------
// Deterministic checks — the spine of orthogonality. Pure code, no agent.
// ---------------------------------------------------------------------------

/// Check whether the agent's actual changes respect the task's declared scope
/// and the project's deny/high-risk globs. These catch what validation structurally
/// cannot: the agent touching files it was not supposed to touch.
pub fn check_scope_drift(
    changed_files: &[String],
    task: &ProjectTask,
    policy: &ProjectPolicy,
) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    let scope_paths: Vec<String> = task
        .scope
        .as_ref()
        .map(|s| s.paths.iter().map(|p| normalize_scope_path(p)).collect())
        .unwrap_or_default();

    for file in changed_files {
        let normalized = normalize_scope_path(file);

        if matches_any_glob(&normalized, &policy.scope.deny_globs) {
            findings.push(ReviewFinding {
                rule: "deny-glob-violation".to_string(),
                severity: ReviewFindingSeverity::Block,
                message: format!("Changed file '{file}' matches a denied glob in project policy."),
                file: Some(file.clone()),
            });
        }

        if matches_any_glob(&normalized, &policy.scope.high_risk_areas) {
            findings.push(ReviewFinding {
                rule: "high-risk-area-touched".to_string(),
                severity: ReviewFindingSeverity::Warn,
                message: format!("Changed file '{file}' is in a declared high-risk area."),
                file: Some(file.clone()),
            });
        }

        if !scope_paths.is_empty() && !matches_any_glob(&normalized, &scope_paths) {
            findings.push(ReviewFinding {
                rule: "scope-drift".to_string(),
                severity: ReviewFindingSeverity::Warn,
                message: format!(
                    "Changed file '{file}' is outside the task's declared scope paths."
                ),
                file: Some(file.clone()),
            });
        }
    }

    findings
}

// High-confidence secret patterns — conservative to avoid false-positive blocks.
enum SecretPattern {
    AwsKey,
    GithubToken,
    PrivateKey,
    GenericSecret,
}

fn match_secret(pattern: &SecretPattern, text: &str) -> usize {
    match pattern {
        SecretPattern::AwsKey => count_aws_keys(text),
        SecretPattern::GithubToken => count_github_tokens(text),
        SecretPattern::PrivateKey => count_private_key_blocks(text),
        SecretPattern::GenericSecret => count_generic_secret(text),
    }
}

/// AKIA[0-9A-Z]{16}
fn count_aws_keys(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut count = 0;
    let mut i = 0;
    while i + 4 <= chars.len() {
        if chars[i] == 'A'
            && chars[i + 1] == 'K'
            && chars[i + 2] == 'I'
            && chars[i + 3] == 'A'
            && i + 20 <= chars.len()
            && chars[i + 4..i + 20]
                .iter()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
        {
            count += 1;
            i += 20;
            continue;
        }
        i += 1;
    }
    count
}

/// gh[ps]_[A-Za-z0-9]{36,}
fn count_github_tokens(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut count = 0;
    let mut i = 0;
    while i + 3 <= chars.len() {
        if chars[i] == 'g'
            && chars[i + 1] == 'h'
            && (chars[i + 2] == 'p' || chars[i + 2] == 's')
            && chars[i + 3] == '_'
        {
            let mut j = i + 4;
            while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j - (i + 4) >= 36 {
                count += 1;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    count
}

/// -----BEGIN (RSA |EC |OPENSSH |DSA |)PRIVATE KEY-----
fn count_private_key_blocks(text: &str) -> usize {
    const VARIANTS: [&str; 5] = [
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
        "-----BEGIN OPENSSH PRIVATE KEY-----",
        "-----BEGIN DSA PRIVATE KEY-----",
        "-----BEGIN PRIVATE KEY-----",
    ];
    VARIANTS.iter().map(|v| text.matches(v).count()).sum()
}
fn is_secret_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-')
}

fn count_generic_secret(text: &str) -> usize {
    // (api[_-]?key|secret|token|password|passwd)\s*[:=]\s*["'][A-Za-z0-9+/=_-]{32,}["'] (case-insensitive)
    let lower = text.to_lowercase();
    const KEYWORDS: [&str; 6] = [
        "api_key", "apikey", "api-key", "secret", "token", "password",
    ];
    const KEYWORDS2: [&str; 1] = ["passwd"];
    let all: Vec<&str> = KEYWORDS
        .iter()
        .copied()
        .chain(KEYWORDS2.iter().copied())
        .collect();
    let chars: Vec<char> = lower.chars().collect();
    let mut count = 0;
    let mut i = 0;
    while i < chars.len() {
        let mut matched_len = 0;
        for kw in &all {
            let kw_chars: Vec<char> = kw.chars().collect();
            if i + kw_chars.len() <= chars.len() && chars[i..i + kw_chars.len()] == kw_chars[..] {
                matched_len = kw_chars.len();
                break;
            }
        }
        if matched_len == 0 {
            i += 1;
            continue;
        }
        let mut j = i + matched_len;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j < chars.len() && (chars[j] == ':' || chars[j] == '=') {
            j += 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '"' || chars[j] == '\'') {
                let quote = chars[j];
                j += 1;
                let value_start = j;
                while j < chars.len() && chars[j] != quote {
                    if !is_secret_char(chars[j]) {
                        break;
                    }
                    j += 1;
                }
                // TS regex parity: the closing quote may differ from the opening one.
                if j < chars.len() && (chars[j] == '"' || chars[j] == '\'') && j - value_start >= 32
                {
                    count += 1;
                }
            }
        }
        i += matched_len;
    }
    count
}

/// Deterministic checks + agent-review file interpretation, for callers that
/// already invoked the reviewer subprocess themselves.
pub fn review_from_agent_files(
    control_plane_path: &Path,
    execution_path: &Path,
    task: &ProjectTask,
    project_policy: &ProjectPolicy,
) -> ReviewResult {
    let mut findings: Vec<ReviewFinding> = Vec::new();
    let diff_patch = get_diff_patch(execution_path);
    let changed_files = diff_patch
        .as_deref()
        .map(extract_changed_files)
        .unwrap_or_default();
    findings.extend(check_scope_drift(&changed_files, task, project_policy));
    if let Some(diff) = &diff_patch {
        findings.extend(check_secrets(diff));
    }
    if let Some((agent_findings, malformed)) = read_agent_review(control_plane_path, &task.id) {
        findings.extend(agent_findings);
        if malformed {
            return ReviewResult {
                has_blocking: findings
                    .iter()
                    .any(|f| f.severity == ReviewFindingSeverity::Block),
                findings,
                malformed: Some(true),
            };
        }
    }
    ReviewResult {
        has_blocking: findings
            .iter()
            .any(|f| f.severity == ReviewFindingSeverity::Block),
        findings,
        malformed: Some(false),
    }
}

/// Scan added diff lines for common hardcoded-secret patterns. Only inspects
/// additions (lines starting with +), not context or deletions.
pub fn check_secrets(diff_patch: &str) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    let added_content: String = diff_patch
        .split('\n')
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .collect::<Vec<_>>()
        .join("\n");

    let patterns: [(&str, SecretPattern); 4] = [
        ("aws-access-key-id", SecretPattern::AwsKey),
        ("github-token", SecretPattern::GithubToken),
        ("private-key-block", SecretPattern::PrivateKey),
        ("generic-secret-assignment", SecretPattern::GenericSecret),
    ];

    for (name, regex) in &patterns {
        let matches = match_secret(regex, &added_content);
        if matches > 0 {
            findings.push(ReviewFinding {
                rule: format!("secret-detection:{name}"),
                severity: ReviewFindingSeverity::Block,
                message: format!(
                    "Possible hardcoded secret detected in diff ({name}): {matches} occurrence(s)."
                ),
                file: None,
            });
        }
    }

    findings
}

/// Extract changed file paths from a git diff patch by parsing `diff --git` headers.
pub fn extract_changed_files(patch: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut files = Vec::new();
    for line in patch.split('\n') {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            if let Some((_, b_path)) = rest.rsplit_once(" b/") {
                let b_path = b_path.to_string();
                if !b_path.is_empty() && seen.insert(b_path.clone()) {
                    files.push(b_path);
                }
            }
        }
    }
    files
}

// ---------------------------------------------------------------------------
// LLM review — the bonus layer. Agent inspects the diff and writes structured
// findings to a JSON file. Fail-open: if the agent doesn't write a valid file,
// only deterministic findings apply.
// ---------------------------------------------------------------------------

pub fn build_review_prompt(task: &ProjectTask, diff_patch: &str, reviews_dir_abs: &Path) -> String {
    const MAX_DIFF_CHARS: usize = 20_000;
    let truncated_diff = if diff_patch.len() > MAX_DIFF_CHARS {
        // Cut on a char boundary.
        let mut end = MAX_DIFF_CHARS;
        while end < diff_patch.len() && !diff_patch.is_char_boundary(end) {
            end += 1;
        }
        format!(
            "{}\n\n... (diff truncated, {} chars omitted)",
            &diff_patch[..end],
            diff_patch.len() - end
        )
    } else {
        diff_patch.to_string()
    };

    [
        "You are reviewing code changes made by another AI agent for safety and correctness.",
        "Do NOT modify any code files. Only review and report findings.",
        "",
        &format!("Task: {} ({})", task.title, task.id),
        &format!("Kind: {}", task.kind.as_str()),
        &format!("Risk: {}", task.risk.as_str()),
        "",
        "## Changed Code (git diff HEAD)",
        "```diff",
        &truncated_diff,
        "```",
        "",
        "## Review Checklist",
        "- Security vulnerabilities: injection, path traversal, hardcoded secrets, unsafe deserialization",
        "- Logic errors or incorrect implementations that pass tests but are wrong",
        "- Breaking changes to public APIs or interfaces",
        "- Missing error handling for new code paths",
        "",
        "## Output",
        &format!(
            "Write your review as JSON to `{}` (absolute path) with exactly this schema:",
            reviews_dir_abs.join(format!("{}.json", task.id)).display()
        ),
        "```json",
        "{",
        "  \"verdict\": \"approve\" | \"request-changes\",",
        "  \"findings\": [",
        "    { \"severity\": \"block\" | \"warn\" | \"info\", \"message\": \"concise description\" }",
        "  ]",
        "}",
        "```",
        "",
        "If the changes look safe and correct, write verdict \"approve\" with an empty findings array.",
        "If you find issues, set verdict \"request-changes\" and list each finding with appropriate severity.",
        "\"block\" = must not auto-merge, \"warn\" = should review, \"info\" = FYI.",
    ]
    .join("\n")
}

/// Read and parse the LLM reviewer's output file.
/// Returns None when the file is missing (the opt-in reviewer simply didn't
/// produce output — not a failure) and Some((findings, malformed)) when it
/// wrote something unreadable (a failure the caller must treat conservatively).
fn read_agent_review(
    control_plane_path: &Path,
    task_id: &str,
) -> Option<(Vec<ReviewFinding>, bool)> {
    let review_path = control_plane_path
        .join(".openloop")
        .join("reviews")
        .join(format!("{task_id}.json"));
    if !file_exists(&review_path) {
        return None;
    }

    let raw = match std::fs::read_to_string(&review_path) {
        Ok(r) => r,
        Err(_) => return Some((Vec::new(), true)),
    };
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&raw);
    match parsed {
        Err(_) => Some((Vec::new(), true)),
        Ok(data) => {
            let verdict = data.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
            let findings_raw = data.get("findings");
            if verdict != "request-changes" || !findings_raw.map(|f| f.is_array()).unwrap_or(false)
            {
                return Some((Vec::new(), false));
            }
            let mut findings = Vec::new();
            for entry in findings_raw.and_then(|f| f.as_array()).unwrap() {
                let Some(message) = entry.get("message").and_then(|m| m.as_str()) else {
                    continue;
                };
                let raw_severity = entry
                    .get("severity")
                    .and_then(|s| s.as_str())
                    .unwrap_or("info");
                let severity = match raw_severity {
                    "block" => ReviewFindingSeverity::Block,
                    "warn" => ReviewFindingSeverity::Warn,
                    _ => ReviewFindingSeverity::Info,
                };
                findings.push(ReviewFinding {
                    rule: "agent-review".to_string(),
                    severity,
                    message: message.to_string(),
                    file: None,
                });
            }
            Some((findings, false))
        }
    }
}

pub struct RunReviewOptions<'a, F>
where
    F: FnMut(&str) -> Result<i32, String>,
{
    /// Main repo tree — control-plane files (.openloop/reviews, policy) live here.
    pub control_plane_path: &'a Path,
    /// Tree the agent edited — worktree when runtime.useWorktree, else main tree.
    pub execution_path: &'a Path,
    pub task: &'a ProjectTask,
    pub project_policy: &'a ProjectPolicy,
    /// Optional agent runner for LLM review. If absent, only deterministic checks run.
    pub reviewer_runner: Option<&'a mut F>,
}

/// Run the post-implementation review. Deterministic checks (scope drift, secret
/// detection) always execute. The LLM review runs only when a reviewerRunner is
/// provided. Never throws — review failures degrade gracefully to deterministic-only.
pub fn run_review<F>(options: RunReviewOptions<'_, F>) -> ReviewResult
where
    F: FnMut(&str) -> Result<i32, String>,
{
    let mut findings: Vec<ReviewFinding> = Vec::new();

    let diff_patch = get_diff_patch(options.execution_path);
    let changed_files = diff_patch
        .as_deref()
        .map(extract_changed_files)
        .unwrap_or_default();

    findings.extend(check_scope_drift(
        &changed_files,
        options.task,
        options.project_policy,
    ));
    if let Some(diff) = &diff_patch {
        findings.extend(check_secrets(diff));
    }

    if let Some(runner) = options.reviewer_runner {
        if let Some(diff) = &diff_patch {
            let reviews_dir = options.control_plane_path.join(".openloop").join("reviews");
            let _ = ensure_dir(&reviews_dir);
            let prompt = build_review_prompt(options.task, diff, &reviews_dir);
            // Agent review failure is non-fatal — deterministic checks are the spine.
            let _ = runner(&prompt);
            if let Some((agent_findings, malformed)) =
                read_agent_review(options.control_plane_path, &options.task.id)
            {
                findings.extend(agent_findings);
                if malformed {
                    return ReviewResult {
                        has_blocking: findings
                            .iter()
                            .any(|f| f.severity == ReviewFindingSeverity::Block),
                        findings,
                        malformed: Some(true),
                    };
                }
            }
        }
    }

    ReviewResult {
        has_blocking: findings
            .iter()
            .any(|f| f.severity == ReviewFindingSeverity::Block),
        findings,
        malformed: Some(false),
    }
}
