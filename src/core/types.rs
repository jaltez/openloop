#![allow(dead_code)]

use serde::{Deserialize, Serialize};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// Global config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalConfig {
    pub version: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_project_alias: Option<String>,
    pub budgets: GlobalBudgets,
    pub runtime: GlobalRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notifications: Option<GlobalNotifications>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_channels: Option<Vec<NotificationChannelConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Vec<LifecycleHookConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard: Option<DashboardConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalNotifications {
    pub on_task_complete: Option<String>,
    pub on_task_failed: Option<String>,
    pub on_budget_blocked: Option<String>,
    pub on_all_tasks_done: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalBudgets {
    pub daily_cost_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_per_run_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalRuntime {
    pub run_timeout_seconds: u64,
    pub max_attempts_per_task: u32,
    pub no_progress_repeat_limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick_interval_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_pending_reviews_per_project: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_runs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_refresh_interval_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_selection_strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardConfig {
    pub port: u16,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NotificationChannelConfig {
    #[serde(rename = "webhook")]
    Webhook { url: String, events: Vec<String> },
    #[serde(rename = "desktop")]
    Desktop { events: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleHookConfig {
    #[serde(rename = "type")]
    pub hook_type: String, // "command" | "webhook"
    pub events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

// ---------------------------------------------------------------------------
// Issue sync
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueSourceConfig {
    pub provider: String, // "github" | "gitlab"
    pub repo: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_sync: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_interval_minutes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_status_comments: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedIssue {
    pub number: i64,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub state: String,
    pub url: String,
    pub task_id: Option<String>,
    pub synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueSyncLedger {
    pub version: i32,
    pub source: IssueSourceConfig,
    pub issues: Vec<SyncedIssue>,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Agent runs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AgentUsage>,
}

// ---------------------------------------------------------------------------
// Project config / registry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    pub version: i32,
    pub project: ProjectMeta,
    pub pi: PiConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentConfig>,
    pub runtime: ProjectRuntime,
    pub validation: ProjectValidation,
    pub risk: ProjectRisk,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_source: Option<IssueSourceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Vec<LifecycleHookConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Vec<ProjectSchedule>>,
    #[serde(
        rename = "scheduleState",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub schedule_state: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ProjectReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<ProjectVerification>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub repo_root: Option<String>,
    #[serde(default)]
    pub initialized_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiConfig {
    #[serde(default)]
    pub model: Option<String>,
    pub prompt_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    #[serde(rename = "type", default)]
    pub agent_type: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRuntime {
    pub use_worktree: bool,
    pub branch_prefix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_setup_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectValidation {
    #[serde(default)]
    pub lint_command: Option<String>,
    #[serde(default)]
    pub test_command: Option<String>,
    #[serde(default)]
    pub typecheck_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRisk {
    pub default_unknown_area_classification: String,
    pub require_policy_for_auto_merge: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectReview {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectVerification {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedProject {
    pub alias: String,
    pub path: String,
    #[serde(default)]
    pub default_branch: Option<String>,
    pub initialized: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsRegistry {
    pub version: i32,
    pub projects: Vec<LinkedProject>,
}

// ---------------------------------------------------------------------------
// Daemon state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonProjectState {
    pub alias: String,
    pub queue_size: usize,
    pub paused: bool,
    #[serde(default)]
    pub last_iteration_at: Option<String>,
    #[serde(default)]
    pub last_result: Option<String>,
    pub blocked_tasks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonCurrentRunState {
    pub project_alias: String,
    #[serde(default)]
    pub task_id: Option<String>,
    pub mode: String,
    pub role: String,
    pub started_at: String,
    pub deadline_at: String,
    pub attempt_number: u32,
    #[serde(default)]
    pub pause_requested_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonState {
    pub version: i32,
    pub started_at: String,
    pub pid: i32,
    #[serde(default)]
    pub active_project: Option<String>,
    pub paused: bool,
    #[serde(default)]
    pub paused_at: Option<String>,
    pub total_budget_spent_usd: f64,
    pub budget_date: String,
    pub budget_spent_usd: f64,
    pub budget_blocked: bool,
    #[serde(default)]
    pub current_run: Option<DaemonCurrentRunState>,
    /// All active runs when runtime.maxConcurrentRuns > 1. Kept in addition
    /// to `currentRun` (the most recent run) so single-run-era readers keep
    /// working against the same state file.
    #[serde(
        default,
        rename = "currentRuns",
        skip_serializing_if = "Option::is_none"
    )]
    pub current_runs: Option<Vec<DaemonCurrentRunState>>,
    #[serde(default)]
    pub projects: Vec<DaemonProjectState>,
    #[serde(
        default,
        rename = "lastDigestDate",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_digest_date: Option<String>,
    /// Last time the daemon polled pending PR promotions via `gh`, used to
    /// throttle runtime.prRefreshIntervalSeconds.
    #[serde(
        default,
        rename = "lastPrRefreshAt",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_pr_refresh_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStoppedBy {
    None,
    Timeout,
    Budget,
    Pause,
    NoProgress,
}

impl RunStoppedBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunStoppedBy::None => "none",
            RunStoppedBy::Timeout => "timeout",
            RunStoppedBy::Budget => "budget",
            RunStoppedBy::Pause => "pause",
            RunStoppedBy::NoProgress => "no-progress",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerRole {
    SddPlanner,
    Implementer,
    RepoImprover,
    CiHealer,
}

impl WorkerRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerRole::SddPlanner => "sdd-planner",
            WorkerRole::Implementer => "implementer",
            WorkerRole::RepoImprover => "repo-improver",
            WorkerRole::CiHealer => "ci-healer",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSchedule {
    pub id: String,
    pub cron: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

pub const SUPPORTED_SELF_HEALING_TASK_KINDS: &[&str] =
    &["lint-fix", "type-fix", "localized-test-fix"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskScope {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    #[serde(rename = "feature")]
    Feature,
    #[serde(rename = "bugfix")]
    Bugfix,
    #[serde(rename = "test")]
    Test,
    #[serde(rename = "refactor")]
    Refactor,
    #[serde(rename = "docs")]
    Docs,
    #[serde(rename = "lint-fix")]
    LintFix,
    #[serde(rename = "type-fix")]
    TypeFix,
    #[serde(rename = "localized-test-fix")]
    LocalizedTestFix,
    #[serde(rename = "ci-heal")]
    CiHeal,
    #[serde(rename = "discovery")]
    Discovery,
    #[serde(rename = "scope-proposal")]
    ScopeProposal,
}

impl TaskKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskKind::Feature => "feature",
            TaskKind::Bugfix => "bugfix",
            TaskKind::Test => "test",
            TaskKind::Refactor => "refactor",
            TaskKind::Docs => "docs",
            TaskKind::LintFix => "lint-fix",
            TaskKind::TypeFix => "type-fix",
            TaskKind::LocalizedTestFix => "localized-test-fix",
            TaskKind::CiHeal => "ci-heal",
            TaskKind::Discovery => "discovery",
            TaskKind::ScopeProposal => "scope-proposal",
        }
    }

    pub fn parse(s: &str) -> Option<TaskKind> {
        Some(match s {
            "feature" => TaskKind::Feature,
            "bugfix" => TaskKind::Bugfix,
            "test" => TaskKind::Test,
            "refactor" => TaskKind::Refactor,
            "docs" => TaskKind::Docs,
            "lint-fix" => TaskKind::LintFix,
            "type-fix" => TaskKind::TypeFix,
            "localized-test-fix" => TaskKind::LocalizedTestFix,
            "ci-heal" => TaskKind::CiHeal,
            "discovery" => TaskKind::Discovery,
            "scope-proposal" => TaskKind::ScopeProposal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    #[serde(rename = "proposed")]
    Proposed,
    #[serde(rename = "planned")]
    Planned,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "awaiting-approval")]
    AwaitingApproval,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "blocked")]
    Blocked,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "promoted")]
    Promoted,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Proposed => "proposed",
            TaskStatus::Planned => "planned",
            TaskStatus::Ready => "ready",
            TaskStatus::AwaitingApproval => "awaiting-approval",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::Promoted => "promoted",
        }
    }

    pub fn parse(s: &str) -> Option<TaskStatus> {
        Some(match s {
            "proposed" => TaskStatus::Proposed,
            "planned" => TaskStatus::Planned,
            "ready" => TaskStatus::Ready,
            "awaiting-approval" => TaskStatus::AwaitingApproval,
            "in_progress" => TaskStatus::InProgress,
            "blocked" => TaskStatus::Blocked,
            "done" => TaskStatus::Done,
            "failed" => TaskStatus::Failed,
            "cancelled" => TaskStatus::Cancelled,
            "promoted" => TaskStatus::Promoted,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskRisk {
    #[serde(rename = "low-risk")]
    LowRisk,
    #[serde(rename = "medium-risk")]
    MediumRisk,
    #[serde(rename = "high-risk")]
    HighRisk,
}

impl TaskRisk {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskRisk::LowRisk => "low-risk",
            TaskRisk::MediumRisk => "medium-risk",
            TaskRisk::HighRisk => "high-risk",
        }
    }

    pub fn parse(s: &str) -> Option<TaskRisk> {
        Some(match s {
            "low-risk" => TaskRisk::LowRisk,
            "medium-risk" => TaskRisk::MediumRisk,
            "high-risk" => TaskRisk::HighRisk,
            _ => return None,
        })
    }

    pub fn rank(&self) -> u8 {
        match self {
            TaskRisk::LowRisk => 0,
            TaskRisk::MediumRisk => 1,
            TaskRisk::HighRisk => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskPromotion {
    #[serde(rename = "auto-merge")]
    AutoMerge,
    #[serde(rename = "pull-request")]
    PullRequest,
    #[serde(rename = "manual-only")]
    ManualOnly,
}

impl TaskPromotion {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskPromotion::AutoMerge => "auto-merge",
            TaskPromotion::PullRequest => "pull-request",
            TaskPromotion::ManualOnly => "manual-only",
        }
    }

    pub fn parse(s: &str) -> Option<TaskPromotion> {
        Some(match s {
            "auto-merge" => TaskPromotion::AutoMerge,
            "pull-request" => TaskPromotion::PullRequest,
            "manual-only" => TaskPromotion::ManualOnly,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub r#ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTask {
    pub id: String,
    pub title: String,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub risk: TaskRisk,
    #[serde(default)]
    pub scope: Option<TaskScope>,
    pub source: TaskSource,
    #[serde(default)]
    pub spec_id: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    pub acceptance_criteria: Vec<String>,
    pub attempts: u32,
    #[serde(default)]
    pub last_failure_signature: Option<String>,
    pub promotion: TaskPromotion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promoted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<TaskRunSummary>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLedger {
    pub version: i32,
    pub updated_at: String,
    pub tasks: Vec<ProjectTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerSelection {
    #[serde(default)]
    pub task: Option<ProjectTask>,
    pub mode: String, // "implement" | "plan" | "idle"
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Review
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewFindingSeverity {
    #[serde(rename = "block")]
    Block,
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "info")]
    Info,
}

impl ReviewFindingSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReviewFindingSeverity::Block => "block",
            ReviewFindingSeverity::Warn => "warn",
            ReviewFindingSeverity::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFinding {
    pub rule: String,
    pub severity: ReviewFindingSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    pub findings: Vec<ReviewFinding>,
    pub has_blocking: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub malformed: Option<bool>,
}

// ---------------------------------------------------------------------------
// Scheduler results / validation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationName {
    #[serde(rename = "lint")]
    Lint,
    #[serde(rename = "test")]
    Test,
    #[serde(rename = "typecheck")]
    Typecheck,
}

impl ValidationName {
    pub fn as_str(&self) -> &'static str {
        match self {
            ValidationName::Lint => "lint",
            ValidationName::Test => "test",
            ValidationName::Typecheck => "typecheck",
        }
    }

    pub fn parse(s: &str) -> Option<ValidationName> {
        Some(match s {
            "lint" => ValidationName::Lint,
            "test" => ValidationName::Test,
            "typecheck" => ValidationName::Typecheck,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationSummary {
    pub name: ValidationName,
    pub command: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromotionDecision {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "auto-merge-eligible")]
    AutoMergeEligible,
    #[serde(rename = "manual-review")]
    ManualReview,
    #[serde(rename = "blocked")]
    Blocked,
}

impl PromotionDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            PromotionDecision::None => "none",
            PromotionDecision::AutoMergeEligible => "auto-merge-eligible",
            PromotionDecision::ManualReview => "manual-review",
            PromotionDecision::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromotionAction {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "queue-auto-merge")]
    QueueAutoMerge,
    #[serde(rename = "queue-review")]
    QueueReview,
    #[serde(rename = "block")]
    Block,
}

impl PromotionAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            PromotionAction::None => "none",
            PromotionAction::QueueAutoMerge => "queue-auto-merge",
            PromotionAction::QueueReview => "queue-review",
            PromotionAction::Block => "block",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunOutcome {
    #[serde(rename = "planned")]
    Planned,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "validation-failed")]
    ValidationFailed,
    #[serde(rename = "pi-failed")]
    PiFailed,
    #[serde(rename = "error")]
    Error,
}

impl RunOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunOutcome::Planned => "planned",
            RunOutcome::Completed => "completed",
            RunOutcome::ValidationFailed => "validation-failed",
            RunOutcome::PiFailed => "pi-failed",
            RunOutcome::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerResult {
    pub project_alias: String,
    #[serde(default)]
    pub task_id: Option<String>,
    pub mode: String,
    #[serde(default)]
    pub role: Option<WorkerRole>,
    pub reason: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub validation: Vec<ValidationSummary>,
    pub promotion_decision: PromotionDecision,
    pub promotion_action: PromotionAction,
    #[serde(default)]
    pub promotion_artifact_path: Option<String>,
    #[serde(default)]
    pub promotion_result_artifact_path: Option<String>,
    #[serde(default)]
    pub task_status: Option<TaskStatus>,
    #[serde(default)]
    pub promoted_at: Option<String>,
    pub stopped_by: RunStoppedBy,
    #[serde(default)]
    pub attempt_number: Option<u32>,
    pub dirty_tree_detected: bool,
    #[serde(default)]
    pub budget_snapshot_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_findings: Option<Vec<ReviewFinding>>,
    #[serde(default)]
    pub run_summary_path: Option<String>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub cost_source: Option<CostSource>,
    #[serde(default)]
    pub approval_packet_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CostSource {
    #[serde(rename = "measured")]
    Measured,
    #[serde(rename = "estimated")]
    Estimated,
}

impl CostSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CostSource::Measured => "measured",
            CostSource::Estimated => "estimated",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRunSummary {
    pub completed_at: String,
    pub mode: String,
    #[serde(default)]
    pub role: Option<WorkerRole>,
    #[serde(default)]
    pub pi_exit_code: Option<i32>,
    pub outcome: RunOutcome,
    #[serde(default)]
    pub base_branch: Option<String>,
    pub validation: Vec<ValidationSummary>,
    pub promotion_decision: PromotionDecision,
    pub effective_promotion_mode: TaskPromotion,
    pub promotion_action: PromotionAction,
    #[serde(default)]
    pub promotion_artifact_path: Option<String>,
    pub promotion_artifact_state: PromotionArtifactStatus,
    #[serde(default)]
    pub promotion_result_artifact_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Promotion artifacts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromotionArtifactStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "applied")]
    Applied,
    #[serde(rename = "rejected")]
    Rejected,
}

impl PromotionArtifactStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PromotionArtifactStatus::Pending => "pending",
            PromotionArtifactStatus::Applied => "applied",
            PromotionArtifactStatus::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionArtifact {
    pub version: i32,
    pub created_at: String,
    pub project_alias: String,
    pub task_id: String,
    #[serde(default)]
    pub base_branch: Option<String>,
    pub decision: PromotionDecision,
    pub action: PromotionAction,
    pub effective_promotion_mode: TaskPromotion,
    pub validation: Vec<ValidationSummary>,
    #[serde(default)]
    pub pi_exit_code: Option<i32>,
    pub outcome: RunOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_packet_path: Option<String>,
    pub status: PromotionArtifactStatus,
    #[serde(default)]
    pub processed_at: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionCheckRollupEntry {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionResultArtifact {
    pub version: i32,
    pub created_at: String,
    pub project_alias: String,
    pub task_id: String,
    pub source_promotion_artifact_path: String,
    pub source_promotion_action: PromotionAction,
    pub source_promotion_decision: PromotionDecision,
    pub result: String, // "applied" | "rejected" | "refreshed"
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<Vec<PromotionCheckRollupEntry>>,
}

// ---------------------------------------------------------------------------
// Project policy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskClassPolicy {
    pub auto_merge_allowed: bool,
    pub requires_human_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPolicy {
    pub version: i32,
    pub scope: PolicyScope,
    pub risk_classes: std::collections::BTreeMap<String, RiskClassPolicy>,
    pub self_healing: SelfHealingPolicy,
    pub promotion: PolicyPromotionModes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyScope {
    #[serde(default)]
    pub allow_globs: Vec<String>,
    #[serde(default)]
    pub deny_globs: Vec<String>,
    #[serde(default)]
    pub high_risk_areas: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfHealingPolicy {
    pub enabled: bool,
    pub allowed_task_kinds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyPromotionModes {
    pub low_risk_mode: TaskPromotion,
    pub medium_risk_mode: TaskPromotion,
    pub high_risk_mode: TaskPromotion,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenLoopEvent {
    pub ts: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_snapshot_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_promotions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_checks: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct RunTimeoutError(pub String);

impl std::fmt::Display for RunTimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RunTimeoutError {}
