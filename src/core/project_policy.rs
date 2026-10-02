use super::types::*;

/// Default project policy, mirroring DEFAULT_PROJECT_POLICY.
pub fn default_project_policy() -> ProjectPolicy {
    ProjectPolicy {
        version: 1,
        scope: PolicyScope {
            allow_globs: Vec::new(),
            deny_globs: Vec::new(),
            high_risk_areas: Vec::new(),
        },
        risk_classes: [
            (
                "low-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: true,
                    requires_human_review: false,
                },
            ),
            (
                "medium-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: false,
                    requires_human_review: true,
                },
            ),
            (
                "high-risk".to_string(),
                RiskClassPolicy {
                    auto_merge_allowed: false,
                    requires_human_review: true,
                },
            ),
        ]
        .into_iter()
        .collect(),
        self_healing: SelfHealingPolicy {
            enabled: true,
            allowed_task_kinds: super::types::SUPPORTED_SELF_HEALING_TASK_KINDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        },
        promotion: PolicyPromotionModes {
            low_risk_mode: TaskPromotion::AutoMerge,
            medium_risk_mode: TaskPromotion::PullRequest,
            high_risk_mode: TaskPromotion::PullRequest,
        },
    }
}
pub fn load_project_policy(project_path: &std::path::Path) -> Result<ProjectPolicy, String> {
    let policy_path = project_path.join(".openloop").join("policy.yaml");
    let raw = match std::fs::read_to_string(&policy_path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(default_project_policy()),
        Err(e) => return Err(e.to_string()),
    };
    // TS parity: a malformed policy.yaml throws — silently falling back to
    // defaults would disable scope enforcement without notice.
    let parsed: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(merge_policy(Some(&parsed)))
}

#[derive(Debug, Clone)]
pub struct ScopePolicyDecision {
    pub blocked: bool,
    pub adjusted_risk: TaskRisk,
    pub reason: Option<String>,
    pub note: Option<String>,
    pub failure_signature: Option<String>,
}

pub fn evaluate_task_scope_policy(
    task: &ProjectTask,
    policy: &ProjectPolicy,
    default_unknown_area_classification: &str,
) -> ScopePolicyDecision {
    let scope_paths = normalize_scope_paths(
        task.scope
            .as_ref()
            .map(|s| s.paths.iter().map(|p| p.as_str()).collect())
            .unwrap_or_default(),
    );
    let has_scope_rules = !policy.scope.allow_globs.is_empty()
        || !policy.scope.deny_globs.is_empty()
        || !policy.scope.high_risk_areas.is_empty();

    let no_decision = || ScopePolicyDecision {
        blocked: false,
        adjusted_risk: task.risk,
        reason: None,
        note: None,
        failure_signature: None,
    };

    if scope_paths.is_empty() {
        if !has_scope_rules {
            return no_decision();
        }
        let default_risk =
            TaskRisk::parse(default_unknown_area_classification).unwrap_or(TaskRisk::MediumRisk);
        let adjusted = max_risk(task.risk, default_risk);
        if adjusted == task.risk {
            return no_decision();
        }
        return ScopePolicyDecision {
            blocked: false,
            adjusted_risk: adjusted,
            reason: None,
            note: Some(format!(
                "Openloop classified task scope conservatively as {} because the task did not declare scope paths while project scope rules are enabled.",
                adjusted.as_str()
            )),
            failure_signature: None,
        };
    }

    let denied_paths: Vec<&str> = scope_paths
        .iter()
        .map(|s| s.as_str())
        .filter(|candidate| matches_any_glob(candidate, &policy.scope.deny_globs))
        .collect();
    if !denied_paths.is_empty() {
        return ScopePolicyDecision {
            blocked: true,
            adjusted_risk: task.risk,
            reason: Some(format!(
                "task scope hits denied paths: {}",
                denied_paths.join(", ")
            )),
            note: Some(format!(
                "Openloop blocked task scope because it targets denied paths: {}.",
                denied_paths.join(", ")
            )),
            failure_signature: Some("policy-scope-denied".to_string()),
        };
    }

    if !policy.scope.allow_globs.is_empty() {
        let outside_allowlist: Vec<&str> = scope_paths
            .iter()
            .map(|s| s.as_str())
            .filter(|candidate| !matches_any_glob(candidate, &policy.scope.allow_globs))
            .collect();
        if !outside_allowlist.is_empty() {
            return ScopePolicyDecision {
                blocked: true,
                adjusted_risk: task.risk,
                reason: Some(format!(
                    "task scope is outside allowed unattended paths: {}",
                    outside_allowlist.join(", ")
                )),
                note: Some(format!(
                    "Openloop blocked task scope because it falls outside the unattended allowlist: {}.",
                    outside_allowlist.join(", ")
                )),
                failure_signature: Some("policy-scope-not-allowlisted".to_string()),
            };
        }
    }

    let high_risk_matches: Vec<&str> = scope_paths
        .iter()
        .map(|s| s.as_str())
        .filter(|candidate| matches_any_glob(candidate, &policy.scope.high_risk_areas))
        .collect();
    if !high_risk_matches.is_empty() {
        return ScopePolicyDecision {
            blocked: false,
            adjusted_risk: max_risk(task.risk, TaskRisk::HighRisk),
            reason: None,
            note: Some(format!(
                "Openloop escalated task risk to high-risk because scope touches high-risk areas: {}.",
                high_risk_matches.join(", ")
            )),
            failure_signature: None,
        };
    }

    no_decision()
}

fn merge_policy(policy: Option<&serde_yaml::Value>) -> ProjectPolicy {
    let defaults = default_project_policy();
    let empty = serde_yaml::Value::Null;
    let p = policy.unwrap_or(&empty);

    let get_vec = |section: &str, key: &str| -> Vec<String> {
        p.get(section)
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };

    let risk_class = |name: &str| -> RiskClassPolicy {
        let configured = p.get("riskClasses").and_then(|s| s.get(name));
        RiskClassPolicy {
            auto_merge_allowed: configured
                .and_then(|c| c.get("autoMergeAllowed"))
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.risk_classes[name].auto_merge_allowed),
            requires_human_review: configured
                .and_then(|c| c.get("requiresHumanReview"))
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.risk_classes[name].requires_human_review),
        }
    };

    let configured_allowed_kinds: Vec<String> = p
        .get("selfHealing")
        .and_then(|s| s.get("allowedTaskKinds"))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(|| defaults.self_healing.allowed_task_kinds.clone());

    let promo_mode = |key: &str| -> TaskPromotion {
        p.get("promotion")
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_str())
            .and_then(TaskPromotion::parse)
            .unwrap_or(match key {
                "lowRiskMode" => defaults.promotion.low_risk_mode,
                "mediumRiskMode" => defaults.promotion.medium_risk_mode,
                _ => defaults.promotion.high_risk_mode,
            })
    };

    ProjectPolicy {
        version: 1,
        scope: PolicyScope {
            allow_globs: get_vec("scope", "allowGlobs"),
            deny_globs: get_vec("scope", "denyGlobs"),
            high_risk_areas: get_vec("scope", "highRiskAreas"),
        },
        risk_classes: [
            ("low-risk".to_string(), risk_class("low-risk")),
            ("medium-risk".to_string(), risk_class("medium-risk")),
            ("high-risk".to_string(), risk_class("high-risk")),
        ]
        .into_iter()
        .collect(),
        self_healing: SelfHealingPolicy {
            enabled: p
                .get("selfHealing")
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(defaults.self_healing.enabled),
            allowed_task_kinds: configured_allowed_kinds
                .into_iter()
                .filter(|kind| SUPPORTED_SELF_HEALING_TASK_KINDS.contains(&kind.as_str()))
                .collect(),
        },
        promotion: PolicyPromotionModes {
            low_risk_mode: promo_mode("lowRiskMode"),
            medium_risk_mode: promo_mode("mediumRiskMode"),
            high_risk_mode: promo_mode("highRiskMode"),
        },
    }
}

pub fn normalize_scope_paths(paths: Vec<&str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for p in paths {
        let normalized = normalize_scope_path(p);
        if !normalized.is_empty() && seen.insert(normalized.clone()) {
            result.push(normalized);
        }
    }
    result
}

pub fn normalize_scope_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

pub fn matches_any_glob(candidate: &str, globs: &[String]) -> bool {
    globs.iter().any(|pattern| matches_glob(candidate, pattern))
}

fn matches_glob(candidate: &str, pattern: &str) -> bool {
    let normalized_pattern = normalize_scope_path(pattern);
    if normalized_pattern.is_empty() {
        return false;
    }
    let expression = glob_to_pattern(&normalized_pattern);
    regex_lite_match(&expression, candidate)
}

/// Convert a glob (`*`, `**`, `?`) into a mini-regex we can match without a
/// regex engine: emit tokens and run a simple backtracking matcher.
fn glob_to_pattern(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let current = chars[i];
        let next = chars.get(i + 1).copied();
        match current {
            '*' if next == Some('*') => {
                tokens.push(GlobToken::AnyRun);
                i += 2;
            }
            '*' => {
                tokens.push(GlobToken::AnyExceptSlash);
                i += 1;
            }
            '?' => {
                tokens.push(GlobToken::OneExceptSlash);
                i += 1;
            }
            c => {
                tokens.push(GlobToken::Literal(c));
                i += 1;
            }
        }
    }
    tokens
}

#[derive(Debug, Clone, PartialEq)]
enum GlobToken {
    AnyRun,         // .*  — ** in the glob
    AnyExceptSlash, // [^/]*  — * in the glob
    OneExceptSlash, // [^/]  — ? in the glob
    Literal(char),
}

/// Anchored full-match with backtracking, equivalent to `^expr$` regex
/// matching for the token language produced by glob_to_pattern.
fn regex_lite_match(tokens: &[GlobToken], candidate: &str) -> bool {
    let chars: Vec<char> = candidate.chars().collect();
    fn rec(tokens: &[GlobToken], chars: &[char]) -> bool {
        if tokens.is_empty() {
            return chars.is_empty();
        }
        match &tokens[0] {
            GlobToken::Literal(c) => {
                !chars.is_empty() && chars[0] == *c && rec(&tokens[1..], &chars[1..])
            }
            GlobToken::OneExceptSlash => {
                !chars.is_empty() && chars[0] != '/' && rec(&tokens[1..], &chars[1..])
            }
            GlobToken::AnyExceptSlash => {
                // try consuming 0..n non-slash chars
                let mut k = 0;
                loop {
                    if rec(&tokens[1..], &chars[k..]) {
                        return true;
                    }
                    if k >= chars.len() || chars[k] == '/' {
                        return false;
                    }
                    k += 1;
                }
            }
            GlobToken::AnyRun => {
                for k in 0..=chars.len() {
                    if rec(&tokens[1..], &chars[k..]) {
                        return true;
                    }
                }
                false
            }
        }
    }
    rec(tokens, &chars)
}

fn max_risk(left: TaskRisk, right: TaskRisk) -> TaskRisk {
    if left.rank() >= right.rank() {
        left
    } else {
        right
    }
}
