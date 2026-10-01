use chrono::TimeZone;
use std::path::{Path, PathBuf};

use super::fsutil::{ensure_dir, file_exists};
use super::git::{add_worktree, get_git_diff_fingerprint, get_git_diff_stat, get_git_working_tree_state, remove_worktree};
use super::model_resolution::resolve_model;
use super::now_iso;
use super::project_policy::evaluate_task_scope_policy;
use super::providers::resolve_provider;
use super::types::*;

// ---------------------------------------------------------------------------
// Scheduler submodules: self-healing, prompts, promotion, no-progress
// ---------------------------------------------------------------------------

pub fn is_self_healing_task(kind: TaskKind) -> bool {
    matches!(
        kind,
        TaskKind::LintFix | TaskKind::TypeFix | TaskKind::LocalizedTestFix | TaskKind::CiHeal
    )
}

pub fn is_supported_self_healing_task(kind: TaskKind) -> bool {
    SUPPORTED_SELF_HEALING_TASK_KINDS.contains(&kind.as_str())
}

pub struct SelfHealingBlock {
    pub reason: String,
    pub note: String,
    pub failure_signature: String,
}

pub fn get_self_healing_block(task: &ProjectTask, project_policy: &ProjectPolicy) -> Option<SelfHealingBlock> {
    if !is_self_healing_task(task.kind) {
        return None;
    }

    if !project_policy.self_healing.enabled {
        return Some(SelfHealingBlock {
            reason: "self-healing is disabled by project policy".to_string(),
            note: "Openloop blocked self-healing task because self-healing is disabled by project policy.".to_string(),
            failure_signature: "self-healing-disabled".to_string(),
        });
    }

    if !is_supported_self_healing_task(task.kind) {
        return Some(SelfHealingBlock {
            reason: format!("self-healing task kind '{}' is outside the V1 scope", task.kind.as_str()),
            note: format!(
                "Openloop blocked self-healing task kind '{}' because V1 only supports ledger-driven lint-fix and type-fix tasks.",
                task.kind.as_str()
            ),
            failure_signature: "self-healing-kind-not-supported".to_string(),
        });
    }

    if !project_policy
        .self_healing
        .allowed_task_kinds
        .iter()
        .any(|kind| kind == task.kind.as_str())
    {
        return Some(SelfHealingBlock {
            reason: format!(
                "self-healing task kind '{}' is not allowed by project policy",
                task.kind.as_str()
            ),
            note: format!(
                "Openloop blocked self-healing task kind '{}' because it is not allowlisted in project policy.",
                task.kind.as_str()
            ),
            failure_signature: "self-healing-kind-not-allowed".to_string(),
        });
    }

    None
}

pub fn determine_worker_role(task: &ProjectTask, mode: &str) -> WorkerRole {
    if task.kind == TaskKind::Discovery || task.kind == TaskKind::ScopeProposal {
        return WorkerRole::RepoImprover;
    }
    if is_supported_self_healing_task(task.kind) {
        return WorkerRole::CiHealer;
    }
    if mode == "plan" {
        return WorkerRole::SddPlanner;
    }
    WorkerRole::Implementer
}

fn describe_self_healing_task(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::LintFix => "lint issue",
        TaskKind::TypeFix => "type issue",
        _ => "localized deterministic test failure",
    }
}

pub fn synthesize_continuous_improvement_task(
    project_alias: &str,
    ledger: &TaskLedger,
    project_config: &ProjectConfig,
    project_policy: &ProjectPolicy,
) -> Option<ProjectTask> {
    let existing_refs: std::collections::HashSet<&str> =
        ledger.tasks.iter().map(|task| task.source.r#ref.as_str()).collect();
    let configured_validation_names = super::validation_utils::get_configured_validation_names(project_config);

    let make = |id: &str, title: &str, kind: TaskKind, r#ref: &str, criteria: Vec<&str>| {
        create_continuous_improvement_task(
            project_alias,
            id,
            title,
            kind,
            r#ref,
            criteria.into_iter().map(|s| s.to_string()).collect(),
        )
    };

    if configured_validation_names.is_empty() && !existing_refs.contains("continuous-improvement:validation-setup") {
        return Some(make(
            "improve-validation-setup",
            "Define validation commands for unattended runs",
            TaskKind::Discovery,
            "continuous-improvement:validation-setup",
            vec![
                "Project-local validation commands are defined for at least one unattended validation step.",
                "The task ledger captures the follow-up implementation scope clearly enough for execution.",
            ],
        ));
    }

    if project_config.validation.test_command.is_none() && !existing_refs.contains("continuous-improvement:test-command") {
        return Some(make(
            "define-test-command",
            "Define unattended test command",
            TaskKind::Discovery,
            "continuous-improvement:test-command",
            vec![
                "Project-local runtime config defines a test command suitable for unattended validation.",
                "The task ledger captures enough follow-up detail to implement and verify the command safely.",
            ],
        ));
    }

    if project_config.validation.typecheck_command.is_none() && !existing_refs.contains("continuous-improvement:typecheck-command") {
        return Some(make(
            "define-typecheck-command",
            "Define unattended typecheck command",
            TaskKind::Discovery,
            "continuous-improvement:typecheck-command",
            vec![
                "Project-local runtime config defines a typecheck command suitable for unattended validation.",
                "The task ledger captures enough follow-up detail to implement and verify the command safely.",
            ],
        ));
    }

    if project_config.validation.lint_command.is_none() && !existing_refs.contains("continuous-improvement:lint-command") {
        return Some(make(
            "define-lint-command",
            "Define unattended lint command",
            TaskKind::Discovery,
            "continuous-improvement:lint-command",
            vec![
                "Project-local runtime config defines a lint command suitable for unattended validation.",
                "The task ledger captures enough follow-up detail to implement and verify the command safely.",
            ],
        ));
    }

    let missing_scope_policy = project_policy.scope.allow_globs.is_empty()
        && project_policy.scope.deny_globs.is_empty()
        && project_policy.scope.high_risk_areas.is_empty();
    if missing_scope_policy && !existing_refs.contains("continuous-improvement:scope-policy") {
        return Some(make(
            "define-scope-policy",
            "Define unattended scope policy",
            TaskKind::ScopeProposal,
            "continuous-improvement:scope-policy",
            vec![
                "The project policy defines unattended allowlists, denied areas, or high-risk areas.",
                "The resulting policy is specific enough for scheduler-side scope enforcement.",
            ],
        ));
    }

    None
}

fn create_continuous_improvement_task(
    project_alias: &str,
    id: &str,
    title: &str,
    kind: TaskKind,
    r#ref: &str,
    acceptance_criteria: Vec<String>,
) -> ProjectTask {
    let now = now_iso();
    ProjectTask {
        id: id.to_string(),
        title: title.to_string(),
        kind,
        status: TaskStatus::Proposed,
        risk: TaskRisk::MediumRisk,
        scope: None,
        source: TaskSource {
            source_type: "discovery".to_string(),
            r#ref: r#ref.to_string(),
        },
        spec_id: None,
        branch: None,
        owner: Some(project_alias.to_string()),
        acceptance_criteria,
        attempts: 0,
        last_failure_signature: None,
        promotion: TaskPromotion::ManualOnly,
        promoted_at: None,
        depends_on: None,
        estimated_cost_usd: None,
        notes: Some(vec!["Created automatically during an idle continuous improvement pass.".to_string()]),
        last_run: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

pub fn build_prompt(
    task: &ProjectTask,
    mode: &str,
    role: WorkerRole,
    spec_content: Option<&str>,
    provider_is_pi: bool,
) -> String {
    let header = if mode == "implement" {
        if is_supported_self_healing_task(task.kind) {
            format!(
                "Repair the following {} with the smallest viable change.",
                describe_self_healing_task(task.kind)
            )
        } else {
            "Implement the following task.".to_string()
        }
    } else {
        "Plan the following task and prepare it for implementation.".to_string()
    };

    let mut lines: Vec<String> = Vec::new();
    // Pi loads openloop conventions from its system-prompt files; every other
    // provider only sees AGENTS.md — point it there explicitly.
    if !provider_is_pi {
        lines.push("Read AGENTS.md and .openloop/policy.yaml before making changes.".to_string());
    }
    lines.push(header);
    lines.push(format!("Task ID: {}", task.id));
    lines.push(format!("Title: {}", task.title));
    lines.push(format!("Kind: {}", task.kind.as_str()));
    lines.push(format!("Assigned Role: {}", role.as_str()));
    lines.push(format!("Risk: {}", task.risk.as_str()));
    lines.push(format!("Source: {} / {}", task.source.source_type, task.source.r#ref));

    if mode == "implement" && is_supported_self_healing_task(task.kind) {
        lines.push("Self-Healing Scope: Ledger-driven repair limited to lint-fix, type-fix, and localized-test-fix tasks.".to_string());
    }

    if mode == "plan" {
        lines.push(format!("Spec Output: Write your implementation plan to .openloop/specs/{}.md before finishing.", task.id));
    }

    if let Some(scope) = &task.scope {
        if !scope.paths.is_empty() {
            lines.push("Scope Paths:".to_string());
            for scope_path in &scope.paths {
                lines.push(format!("- {scope_path}"));
            }
        }
    }

    lines.push("Acceptance Criteria:".to_string());
    for criterion in &task.acceptance_criteria {
        lines.push(format!("- {criterion}"));
    }

    if mode == "implement" {
        if let Some(spec) = spec_content {
            lines.push(String::new());
            lines.push("## Implementation Spec".to_string());
            lines.push(spec.to_string());
        }
    }

    lines.join("\n")
}


pub fn detect_and_set_spec_id(project_path: &Path, task: &mut ProjectTask) {
    let spec_path = PathBuf::from(".openloop").join("specs").join(format!("{}.md", task.id));
    let full_path = project_path.join(&spec_path);
    if full_path.exists() {
        task.spec_id = Some(spec_path.to_string_lossy().to_string());
    }
}

/// Verifier prompt: the implementer's work is judged by a separate role that
/// must not touch code. Output is a JSON array (one object per acceptance
/// criterion) written to the ABSOLUTE control-plane path — the execution tree
/// (worktree) has no `.openloop/` directory to write to.
pub fn build_verifier_prompt(task: &ProjectTask, spec_content: Option<&str>, reviews_dir_abs: &Path) -> String {
    let verifications_path = reviews_dir_abs
        .parent()
        .unwrap_or(reviews_dir_abs)
        .join("verifications")
        .join(format!("{}.json", task.id));
    let mut lines: Vec<String> = vec![
        "You are the verification role for an AI implementation run.".to_string(),
        "Do NOT modify any code files. Only inspect the current state of the repository and report verdicts.".to_string(),
        String::new(),
        format!("Task: {} ({})", task.title, task.id),
        String::new(),
        "## Acceptance Criteria".to_string(),
    ];
    for (index, criterion) in task.acceptance_criteria.iter().enumerate() {
        lines.push(format!("{}. {}", index + 1, criterion));
    }
    if let Some(spec) = spec_content {
        lines.push(String::new());
        lines.push("## Spec".to_string());
        lines.push(spec.to_string());
    }
    lines.extend([
        String::new(),
        "## Output".to_string(),
        format!(
            "Create the directory containing `{}` if needed and write a JSON ARRAY to that exact absolute path:",
            verifications_path.display()
        ),
        "```json".to_string(),
        "[ { \"index\": 1, \"criterion\": \"the criterion text\", \"verdict\": \"pass\" | \"fail\" | \"needs-human\", \"evidence\": \"what you observed\" } ]".to_string(),
        "```".to_string(),
        "One object per criterion, index starting at 1, in criterion order.".to_string(),
        "verdict \"pass\" = criterion satisfied; \"fail\" = not satisfied; \"needs-human\" = cannot be determined automatically.".to_string(),
    ]);
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Promotion decisions
// ---------------------------------------------------------------------------

pub fn decide_promotion(
    task: &ProjectTask,
    validation: &[ValidationSummary],
    effective_promotion_mode: TaskPromotion,
    project_config: &ProjectConfig,
) -> PromotionDecision {
    if validation.iter().any(|item| item.exit_code != 0) {
        return PromotionDecision::Blocked;
    }

    if task.status != TaskStatus::Done {
        return PromotionDecision::None;
    }

    if effective_promotion_mode == TaskPromotion::AutoMerge {
        // S3: If no validations are configured, auto-merge is unsafe regardless of risk tier.
        if super::validation_utils::get_configured_validation_names(project_config).is_empty() {
            eprintln!(
                "[openloop] Warning: no validation commands configured for project. Forcing manual-only promotion. Run 'openloop config project-set-validation' to configure lint/test/typecheck commands."
            );
            return PromotionDecision::ManualReview;
        }
        if !super::validation_utils::has_all_required_auto_merge_validations(validation, project_config) {
            return PromotionDecision::ManualReview;
        }
        return PromotionDecision::AutoMergeEligible;
    }

    PromotionDecision::ManualReview
}
pub fn decide_promotion_action(decision: PromotionDecision) -> PromotionAction {
    match decision {
        PromotionDecision::AutoMergeEligible => PromotionAction::QueueAutoMerge,
        PromotionDecision::ManualReview => PromotionAction::QueueReview,
        PromotionDecision::Blocked => PromotionAction::Block,
        PromotionDecision::None => PromotionAction::None,
    }
}

pub fn read_spec_content(project_path: &Path, task: &ProjectTask) -> Option<String> {
    let spec_id = task.spec_id.as_ref()?;
    std::fs::read_to_string(project_path.join(spec_id)).ok()
}

pub fn resolve_effective_promotion_mode(
    task: &ProjectTask,
    project_policy: &ProjectPolicy,
    require_policy_for_auto_merge: bool,
) -> TaskPromotion {
    let policy_mode = get_policy_promotion_mode(task.risk, project_policy);
    if task.promotion == TaskPromotion::ManualOnly || policy_mode == TaskPromotion::ManualOnly {
        return TaskPromotion::ManualOnly;
    }

    if task.promotion == TaskPromotion::PullRequest || policy_mode == TaskPromotion::PullRequest {
        return TaskPromotion::PullRequest;
    }

    if task.risk != TaskRisk::LowRisk {
        return TaskPromotion::PullRequest;
    }

    if require_policy_for_auto_merge
        && !project_policy
            .risk_classes
            .get(task.risk.as_str())
            .map(|rc| rc.auto_merge_allowed)
            .unwrap_or(false)
    {
        return TaskPromotion::PullRequest;
    }

    TaskPromotion::AutoMerge
}

fn get_policy_promotion_mode(risk: TaskRisk, project_policy: &ProjectPolicy) -> TaskPromotion {
    match risk {
        TaskRisk::LowRisk => project_policy.promotion.low_risk_mode,
        TaskRisk::MediumRisk => project_policy.promotion.medium_risk_mode,
        TaskRisk::HighRisk => project_policy.promotion.high_risk_mode,
    }
}

// ---------------------------------------------------------------------------
// No-progress detection
// ---------------------------------------------------------------------------

pub struct NoProgressInput<'a> {
    pub previous_failure_signature: Option<&'a str>,
    pub previous_promotion_decision: Option<PromotionDecision>,
    pub current_promotion_decision: PromotionDecision,
    pub before_fingerprint: Option<String>,
    pub after_fingerprint: Option<String>,
    pub no_progress_repeat_limit: u32,
}

pub fn should_stop_for_no_progress(task: &mut ProjectTask, input: &NoProgressInput<'_>) -> bool {
    let mut observations: Vec<String> = Vec::new();
    let current_failure_signature = task.last_failure_signature.clone();

    if let (Some(current), Some(previous)) = (&current_failure_signature, input.previous_failure_signature) {
        if current == previous {
            observations.push(format!("failure-{}", sanitize_observation_key(current)));
        }
    }

    if input.previous_promotion_decision == Some(PromotionDecision::Blocked)
        && input.current_promotion_decision == PromotionDecision::Blocked
    {
        observations.push("promotion-blocked".to_string());
    }

    if input.before_fingerprint == input.after_fingerprint {
        observations.push("diff-unchanged".to_string());
    }

    let mut blocked = false;
    let existing_notes = task.notes.clone().unwrap_or_default();
    let mut new_notes = existing_notes.clone();
    for observation in &observations {
        let count = next_no_progress_count(&existing_notes, observation);
        new_notes.push(format!("openloop:no-progress:{observation}:{count}"));
        if count >= input.no_progress_repeat_limit {
            blocked = true;
        }
    }
    task.notes = Some(new_notes);

    blocked
}

fn next_no_progress_count(notes: &[String], key: &str) -> u32 {
    let prefix = format!("openloop:no-progress:{key}:");
    for note in notes.iter().rev() {
        if !note.starts_with(&prefix) {
            continue;
        }
        let count: u32 = note[prefix.len()..].parse().unwrap_or(0);
        return count + 1;
    }
    1
}

fn sanitize_observation_key(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_lowercase();
    if trimmed.is_empty() {
        "observation".to_string()
    } else {
        trimmed
    }
}

// ---------------------------------------------------------------------------
// Task selection
// ---------------------------------------------------------------------------

pub fn select_next_task(ledger: &TaskLedger) -> SchedulerSelection {
    let ready_tasks: Vec<&ProjectTask> = ledger.tasks.iter().filter(|task| task.status == TaskStatus::Ready).collect();

    // dependsOn gating: a ready task is only eligible when every local ref
    // resolves to a promoted/done/cancelled task. Cross-project refs are
    // reserved for future cross-repo scheduling — never satisfied here.
    let dependency_satisfied = |task: &ProjectTask| -> bool {
        task.depends_on
            .as_ref()
            .map(|deps| {
                deps.iter().all(|r| {
                    if r.contains(':') {
                        return false;
                    }
                    ledger
                        .tasks
                        .iter()
                        .any(|candidate| &candidate.id == r && matches!(candidate.status, TaskStatus::Promoted | TaskStatus::Done | TaskStatus::Cancelled))
                })
            })
            .unwrap_or(true)
    };

    let satisfiable_ready: Vec<&ProjectTask> = ready_tasks.iter().copied().filter(|t| dependency_satisfied(t)).collect();

    // Aging: a medium/high-risk task stuck ready for over 24h becomes
    // queue-equivalent to low-risk, so low-risk-first cannot starve it.
    // Within the equivalent set the oldest-updated task runs first.
    let stale_cutoff = super::now_millis() - 24 * 60 * 60 * 1000;
    let cutoff_iso = chrono::Utc
        .timestamp_millis_opt(stale_cutoff)
        .single()
        .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_default();
    let equivalent: Vec<&&ProjectTask> = satisfiable_ready
        .iter()
        .filter(|task| task.risk == TaskRisk::LowRisk || task.updated_at.as_str() < cutoff_iso.as_str())
        .collect();
    if !equivalent.is_empty() {
        let mut sorted: Vec<&&ProjectTask> = equivalent.clone();
        sorted.sort_by(|left, right| left.updated_at.cmp(&right.updated_at).then_with(|| left.id.cmp(&right.id)));
        let oldest = sorted[0];
        let aged = oldest.risk != TaskRisk::LowRisk;
        return SchedulerSelection {
            task: Some((*oldest).clone()),
            mode: "implement".to_string(),
            reason: if aged {
                format!(
                    "selected ready task {} (aged past 24h, queue-equivalent to low-risk)",
                    oldest.id
                )
            } else {
                "selected ready low-risk task".to_string()
            },
        };
    }

    if let Some(ready_task) = satisfiable_ready.first() {
        return SchedulerSelection {
            task: Some((*ready_task).clone()),
            mode: "implement".to_string(),
            reason: "selected ready task".to_string(),
        };
    }

    if let Some(planning_task) = ledger
        .tasks
        .iter()
        .find(|task| task.status == TaskStatus::Proposed || task.status == TaskStatus::Planned)
    {
        return SchedulerSelection {
            task: Some(planning_task.clone()),
            mode: "plan".to_string(),
            reason: "selected task requiring planning".to_string(),
        };
    }

    if !ready_tasks.is_empty() {
        let mut local_blockers: Vec<String> = Vec::new();
        let mut cross_refs: Vec<String> = Vec::new();
        for r in ready_tasks.iter().flat_map(|task| task.depends_on.iter().flatten()) {
            if r.contains(':') {
                if !cross_refs.contains(r) {
                    cross_refs.push(r.clone());
                }
                continue;
            }
            let dependency_ok = ledger.tasks.iter().any(|candidate| {
                &candidate.id == r && matches!(candidate.status, TaskStatus::Promoted | TaskStatus::Done | TaskStatus::Cancelled)
            });
            if !dependency_ok && !local_blockers.contains(r) {
                local_blockers.push(r.clone());
            }
        }
        let mut blocker_list = local_blockers;
        for r in cross_refs {
            blocker_list.push(format!("{r} (cross-project, not schedulable here)"));
        }
        if !blocker_list.is_empty() {
            return SchedulerSelection {
                task: None,
                mode: "idle".to_string(),
                reason: format!("ready tasks blocked by unsatisfied dependsOn: {}", blocker_list.join(", ")),
            };
        }
    }

    SchedulerSelection {
        task: None,
        mode: "idle".to_string(),
        reason: "no eligible task found".to_string(),
    }
}

/// Resolve the cost of a completed agent run: a parsed positive cost wins
/// ("measured"); anything else falls back to the configured estimate.
pub fn resolve_run_cost(result: &AgentRunResult, estimated: f64) -> (f64, CostSource) {
    if let Some(measured) = result.usage.as_ref().and_then(|u| u.cost_usd) {
        if measured.is_finite() && measured > 0.0 {
            return (measured, CostSource::Measured);
        }
    }
    (estimated, CostSource::Estimated)
}

// ---------------------------------------------------------------------------
// runProjectIteration — the full scheduler iteration
// ---------------------------------------------------------------------------

pub struct IterationOptions<'a> {
    pub model_override: Option<&'a str>,
    pub timeout_ms: Option<u64>,
    pub max_attempts_per_task: Option<u32>,
    pub no_progress_repeat_limit: Option<u32>,
}

/// Agent runner abstraction: a closure that receives (project, prompt, model, remaining_ms)
/// and returns the AgentRunResult. Production wires the provider dispatch.
pub type PiRunner<'a> = dyn FnMut(&LinkedProject, &str, Option<&str>, Option<u64>) -> Result<AgentRunResult, String> + 'a;

/// Reviewer/verifier runner: prompt -> exit code.
pub type PromptRunner<'a> = dyn FnMut(&str) -> Result<i32, String> + 'a;

pub struct RunHooks<'a> {
    pub pi_runner: Option<&'a mut PiRunner<'a>>,
    pub reviewer_runner: Option<&'a mut PromptRunner<'a>>,
    pub verifier_runner: Option<&'a mut PromptRunner<'a>>,
    /// Validation runner: (project_path, command, remaining_ms) -> exit code.
    pub validation_runner: Option<&'a mut dyn FnMut(&Path, &str, Option<u64>) -> Result<i32, String>>,
}

fn idle_result(project_alias: &str, reason: &str, model: Option<&str>) -> SchedulerResult {
    SchedulerResult {
        project_alias: project_alias.to_string(),
        task_id: None,
        mode: "idle".to_string(),
        role: None,
        reason: reason.to_string(),
        model: model.map(|m| m.to_string()),
        exit_code: None,
        prompt: None,
        validation: Vec::new(),
        promotion_decision: PromotionDecision::None,
        promotion_action: PromotionAction::None,
        promotion_artifact_path: None,
        promotion_result_artifact_path: None,
        task_status: None,
        promoted_at: None,
        stopped_by: RunStoppedBy::None,
        attempt_number: None,
        dirty_tree_detected: false,
        budget_snapshot_usd: None,
        review_findings: None,
        run_summary_path: None,
        cost_usd: None,
        cost_source: None,
        approval_packet_path: None,
    }
}

fn run_worktree_setup_command(cwd: &Path, command: &str) -> Result<i32, String> {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(1))
}

struct VerifierOutcome {
    failed: bool,
    requires_review: bool,
    failing_criteria: Vec<String>,
}


const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024;

/// Persist the agent stdout/stderr tail next to the run summary (control plane).
fn write_run_transcript(control_plane_path: &Path, task_id: &str, result: &AgentRunResult) {
    if result.stdout.is_empty() && result.stderr.is_empty() {
        return;
    }
    let runs_dir = control_plane_path.join(".openloop").join("runs");
    if ensure_dir(&runs_dir).is_err() {
        return;
    }
    let text = if !result.stderr.is_empty() {
        format!("{}\n--- stderr ---\n{}", result.stdout, result.stderr)
    } else {
        result.stdout.clone()
    };
    let bytes = text.as_bytes();
    let clipped: &[u8] = if bytes.len() > MAX_TRANSCRIPT_BYTES {
        &bytes[bytes.len() - MAX_TRANSCRIPT_BYTES..]
    } else {
        bytes
    };
    let timestamp = now_iso().replace(':', "-");
    let path = runs_dir.join(format!("{timestamp}-{task_id}.transcript.log"));
    let _ = std::fs::write(path, clipped);
}

fn maybe_write_promotion_artifact(
    project_path: &Path,
    project_alias: &str,
    task_id: &str,
    base_branch: Option<&str>,
    decision: PromotionDecision,
    action: PromotionAction,
    effective_promotion_mode: TaskPromotion,
    validation: &[ValidationSummary],
    pi_exit_code: Option<i32>,
    outcome: RunOutcome,
    approval_packet_path: Option<&str>,
) -> Result<Option<String>, String> {
    if action == PromotionAction::None {
        return Ok(None);
    }

    let artifact = PromotionArtifact {
        version: 1,
        created_at: now_iso(),
        project_alias: project_alias.to_string(),
        task_id: task_id.to_string(),
        base_branch: base_branch.map(|s| s.to_string()),
        decision,
        action,
        effective_promotion_mode,
        validation: validation.to_vec(),
        pi_exit_code,
        outcome,
        approval_packet_path: approval_packet_path.map(|s| s.to_string()),
        status: PromotionArtifactStatus::Pending,
        processed_at: None,
        note: None,
    };
    super::promotion_artifacts::write_promotion_artifact(project_path, &artifact).map(Some)
}

/// Repoint an approval packet at the run summary that actually exists.
fn fix_approval_packet_summary_path(packet_path: &str, summary_path: &str) {
    // Packet provenance repair is best-effort.
    let Ok(raw) = std::fs::read_to_string(packet_path) else {
        return;
    };
    let Ok(mut packet) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let current = packet.get("runSummaryPath").and_then(|v| v.as_str()).unwrap_or("");
    if current == summary_path {
        return;
    }
    packet["runSummaryPath"] = serde_json::json!(summary_path);
    let _ = super::fsutil::write_atomic(
        Path::new(packet_path),
        format!("{}\n", serde_json::to_string_pretty(&packet).unwrap_or_default()).as_bytes(),
    );
}

fn get_initial_git_state(project_path: &Path) -> Option<String> {
    get_git_working_tree_state(project_path).ok().and_then(|s| s.current_branch)
}

/// The full scheduler iteration for one project. Mirrors runProjectIteration.
pub fn run_project_iteration(
    project: &LinkedProject,
    options: &IterationOptions<'_>,
    hooks: &mut RunHooks<'_>,
) -> Result<SchedulerResult, String> {
    let project_path = Path::new(&project.path);
    let ledger = super::task_ledger::load_task_ledger(project_path)?;
    let model = resolve_model(project_path, options.model_override, None)?;
    let global_config = super::global_config::load_global_config(None)?;
    let project_config = super::project_config::load_project_config(project_path)?;
    let project_policy = super::project_policy::load_project_policy(project_path)?;

    let mut selection = select_next_task(&ledger);
    let timeout_ms = options
        .timeout_ms
        .or(Some(global_config.runtime.run_timeout_seconds * 1000));
    let max_attempts_per_task = options
        .max_attempts_per_task
        .unwrap_or(global_config.runtime.max_attempts_per_task);
    let no_progress_repeat_limit = options
        .no_progress_repeat_limit
        .unwrap_or(global_config.runtime.no_progress_repeat_limit);
    let estimated_cost_per_run = global_config.budgets.estimated_cost_per_run_usd.unwrap_or(0.0);

    if selection.task.is_none() {
        if let Some(generated_task) = synthesize_continuous_improvement_task(
            &project.alias,
            &ledger,
            &project_config,
            &project_policy,
        ) {
            super::task_ledger::upsert_task(project_path, &generated_task)?;
            selection = SchedulerSelection {
                reason: format!("generated continuous improvement backlog task: {}", generated_task.title),
                task: Some(generated_task),
                mode: "plan".to_string(),
            };
        }
    }

    let Some(mut task) = selection.task.clone() else {
        let result = idle_result(&project.alias, &selection.reason, model.as_deref());
        let summary = super::run_summaries::write_run_summary(project_path, &result, None)?;
        let mut result = result;
        result.run_summary_path = Some(summary);
        return Ok(result);
    };

    let mode = selection.mode.clone();
    if mode == "idle" {
        return Err("Scheduler selected a task but remained idle.".to_string());
    }
    let role = determine_worker_role(&task, &mode);

    let scope_decision = evaluate_task_scope_policy(
        &task,
        &project_policy,
        &project_config.risk.default_unknown_area_classification,
    );
    if let Some(note) = &scope_decision.note {
        let mut notes = task.notes.clone().unwrap_or_default();
        notes.push(note.clone());
        task.notes = Some(notes);
    }
    if scope_decision.adjusted_risk != task.risk {
        task.risk = scope_decision.adjusted_risk;
    }
    if scope_decision.blocked {
        task.status = TaskStatus::Blocked;
        task.last_failure_signature = scope_decision.failure_signature.clone();
        task.updated_at = now_iso();
        super::task_ledger::upsert_task(project_path, &task)?;

        let mut result = idle_result(&project.alias, &format!("{}; {}", selection.reason, scope_decision.reason.unwrap_or_else(|| "task blocked by project scope policy".to_string())), model.as_deref());
        result.task_id = Some(task.id.clone());
        result.mode = mode.clone();
        result.role = Some(role);
        result.task_status = Some(task.status);
        result.promoted_at = task.promoted_at.clone();
        result.run_summary_path = Some(super::run_summaries::write_run_summary(project_path, &result, None)?);
        return Ok(result);
    }

    if let Some(block) = get_self_healing_block(&task, &project_policy) {
        task.status = TaskStatus::Blocked;
        task.last_failure_signature = Some(block.failure_signature.clone());
        let mut notes = task.notes.clone().unwrap_or_default();
        notes.push(block.note.clone());
        task.notes = Some(notes);
        task.updated_at = now_iso();
        super::task_ledger::upsert_task(project_path, &task)?;

        let mut result = idle_result(&project.alias, &format!("{}; {}", selection.reason, block.reason), model.as_deref());
        result.task_id = Some(task.id.clone());
        result.mode = mode.clone();
        result.role = Some(role);
        result.task_status = Some(task.status);
        result.promoted_at = task.promoted_at.clone();
        result.run_summary_path = Some(super::run_summaries::write_run_summary(project_path, &result, None)?);
        return Ok(result);
    }

    if task.attempts >= max_attempts_per_task {
        task.status = TaskStatus::Blocked;
        task.last_failure_signature = Some("max-attempts-reached".to_string());
        let mut notes = task.notes.clone().unwrap_or_default();
        notes.push(format!(
            "Openloop blocked task after reaching max attempts ({}).",
            max_attempts_per_task
        ));
        task.notes = Some(notes);
        task.updated_at = now_iso();
        super::task_ledger::upsert_task(project_path, &task)?;

        let mut result = idle_result(&project.alias, &format!("{}; max attempts reached", selection.reason), model.as_deref());
        result.task_id = Some(task.id.clone());
        result.mode = mode.clone();
        result.role = Some(role);
        result.run_summary_path = Some(super::run_summaries::write_run_summary(project_path, &result, None)?);
        return Ok(result);
    }

    let previous_status = task.status;
    let previous_failure_signature = task.last_failure_signature.clone();
    let previous_promotion_decision = task.last_run.as_ref().map(|lr| lr.promotion_decision);
    task.status = TaskStatus::InProgress;
    task.owner = Some("openloop".to_string());
    task.updated_at = now_iso();
    super::task_ledger::upsert_task(project_path, &task)?;

    // D6: Use the model-agnostic agent runner.
    let default_provider = global_config.default_provider.clone();
    // Dispatch-time provider gate: fail with a clear message before the agent
    // subprocess would spawn, instead of an ENOENT mid-run.
    if hooks.pi_runner.is_none() {
        super::pi::assert_provider_available(Some(&project_config), default_provider.as_deref())?;
    }

    let mut validation: Vec<ValidationSummary> = Vec::new();
    let mut approval_packet_path: Option<String> = None;
    let mut agent_started = false;

    // W1: Include spec content in implement prompts when a spec file exists.
    let spec_content = if mode == "implement" {
        std::fs::read_to_string(project_path.join(task.spec_id.clone().unwrap_or_default())).ok().filter(|s| !s.is_empty()).filter(|_| task.spec_id.is_some())
    } else {
        None
    };
    let resolved_provider_name = resolve_provider(
        project_config.agent.as_ref().and_then(|a| a.agent_type.as_deref()),
        project_config.agent.as_ref().and_then(|a| a.command.as_deref()),
        global_config.default_provider.as_deref(),
    )
    .name;
    let built_prompt = build_prompt(&task, &mode, role, spec_content.as_deref(), resolved_provider_name == super::providers::ProviderName::Pi);
    let git_state = get_initial_git_state(project_path);
    let attempt_number = task.attempts + 1;
    let run_started_at = std::time::Instant::now();
    let before_fingerprint = if project_config.runtime.use_worktree {
        // Worktree starts clean; progress is detected from the after-state.
        None
    } else {
        get_git_diff_fingerprint(project_path)
    };

    // D5: Worktree path/branch for isolated runs (setup happens inside the try below).
    let use_worktree = project_config.runtime.use_worktree;
    let worktree_branch_name = use_worktree
        .then(|| format!("{}{}", project_config.runtime.branch_prefix, task.id));
    let worktree_path = use_worktree.then(|| {
        project_path
            .join(".openloop")
            .join("worktrees")
            .join(&task.id)
    });

    // Control-plane path (main tree: .openloop ledger, specs, reviews) vs execution
    // path (worktree when runtime.useWorktree, else the main tree the agent edits).
    let control_plane_path = project_path.to_path_buf();
    let execution_path = worktree_path
        .clone()
        .unwrap_or_else(|| project_path.to_path_buf());

    let get_remaining_timeout_ms = || -> Option<u64> {
        timeout_ms.map(|total| total.saturating_sub(run_started_at.elapsed().as_millis() as u64))
    };

    let cleanup_worktree = || {
        if let Some(worktree_path) = &worktree_path {
            remove_worktree(project_path, worktree_path, true);
        }
    };

    // The worktree is per-run scratch: agent edits live on the run branch; the
    // checkout is always force-removed so leftover untracked files can't wedge it.
    let worktree_result: Result<SchedulerResult, String> = (|| {
        // D5: Set up an isolated git worktree for this run if useWorktree is configured.
        // Fail closed: when isolation is explicitly requested but cannot be established,
        // abort the run rather than silently operating on the main working tree.
        if let (true, Some(worktree_path), Some(worktree_branch_name)) = (use_worktree, worktree_path.as_deref(), worktree_branch_name.as_deref()) {
            if let Err(error) = add_worktree(project_path, Path::new(worktree_path), worktree_branch_name) {
                cleanup_worktree();
                return Err(format!(
                    "Worktree isolation failed for {}: {}. Aborting because useWorktree is enabled; refusing to fall back to the main working tree.",
                    worktree_path.display(), error
                ));
            }
            // Liveness marker: crash recovery skips force-reclaiming a checkout
            // whose owning process is still alive (e.g. a concurrent CLI run).
            let _ = std::fs::write(
                Path::new(worktree_path).join(".openloop-run.pid"),
                format!("{}\n", std::process::id()),
            );
            // Worktrees lack gitignored build state (node_modules etc.) — allow an
            // explicit setup step. Fail closed: a failed setup aborts the run.
            if let Some(worktree_setup_command) = project_config.runtime.worktree_setup_command.as_deref() {
                let setup_exit_code = match run_worktree_setup_command(Path::new(worktree_path), worktree_setup_command) {
                    Ok(code) => code,
                    Err(e) => {
                        cleanup_worktree();
                        return Err(format!(
                            "Worktree setup command failed: {} ({}). Aborting because useWorktree is enabled; refusing to fall back to the main working tree.",
                            worktree_setup_command, e
                        ));
                    }
                };
                if setup_exit_code != 0 {
                    cleanup_worktree();
                    return Err(format!(
                        "Worktree setup command failed: {} (exit {}). Aborting because useWorktree is enabled; refusing to fall back to the main working tree.",
                        worktree_setup_command, setup_exit_code
                    ));
                }
            }
        }

        let run_project = LinkedProject {
            path: worktree_path
                .as_deref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| project.path.clone()),
            alias: project.alias.clone(),
            default_branch: project.default_branch.clone(),
            initialized: project.initialized,
            created_at: project.created_at.clone(),
            updated_at: project.updated_at.clone(),
        };
        agent_started = true;

        // Run the agent (subprocess with its own timeout enforcement).
        let run_result: Result<AgentRunResult, String> = match hooks.pi_runner.as_mut() {
            Some(runner) => runner(&run_project, &built_prompt, model.as_deref(), get_remaining_timeout_ms()),
            None => super::pi::run_agent(
                &super::pi::PiRunOptions {
                    prompt: &built_prompt,
                    model: model.as_deref(),
                    project: &run_project,
                    timeout_ms: get_remaining_timeout_ms(),
                },
                Some(&project_config),
                default_provider.as_deref(),
            ),
        };
        let run_result = match run_result {
            Ok(r) => r,
            Err(e) if super::pi::is_timeout_error(&e) => {
                return Err(format!("__iteration_timeout__:{}", super::pi::timeout_message(&e)));
            }
            Err(e) => return Err(e),
        };
        let exit_code = run_result.exit_code;
        let mut outcome = RunOutcome::Completed;
        let mut stopped_by = RunStoppedBy::None;
        let mut review_result: Option<ReviewResult> = None;
        let mut verifier_requires_review = false;

        if exit_code == 0 {
            if mode == "implement" {
                validation = match hooks.validation_runner.as_mut() {
                    Some(runner) => {
                        let mut results = Vec::new();
                        let steps: [(ValidationName, &Option<String>); 3] = [
                            (ValidationName::Lint, &project_config.validation.lint_command),
                            (ValidationName::Test, &project_config.validation.test_command),
                            (ValidationName::Typecheck, &project_config.validation.typecheck_command),
                        ];
                        for (name, command) in steps {
                            let Some(command) = command else { continue };
                            let timeout = get_remaining_timeout_ms();
                            if let Some(ms) = timeout {
                                if ms == 0 {
                                    return Err("__iteration_timeout__:Run exceeded timeout before validation completed.".to_string());
                                }
                            }
                            let exit = runner(&execution_path, command, timeout)?;
                            results.push(ValidationSummary {
                                name,
                                command: command.clone(),
                                exit_code: exit,
                            });
                            if exit != 0 {
                                break;
                            }
                        }
                        results
                    }
                    None => super::validation::run_configured_validations(
                        &execution_path,
                        &project_config,
                        get_remaining_timeout_ms,
                    )
                    .map_err(|e| match e {
                        super::validation::ValidationError::Timeout(t) => format!("__iteration_timeout__:{}", t.0),
                        super::validation::ValidationError::Other(o) => o,
                    })?,
                };
                let validation_failed = validation.iter().any(|item| item.exit_code != 0);
                if validation_failed {
                    task.status = TaskStatus::Failed;
                    task.attempts += 1;
                    task.last_failure_signature = Some(format!(
                        "validation-{}",
                        validation
                            .iter()
                            .find(|item| item.exit_code != 0)
                            .map(|item| item.name.as_str())
                            .unwrap_or("unknown")
                    ));
                    push_note(&mut task, "Openloop implement run passed Pi but failed validation.");
                    outcome = RunOutcome::ValidationFailed;
                } else {
                    // V1: Independent verifier — the implementer's work is judged by a
                    // separate role before the task can be called done.
                    let verification_enabled = project_config
                        .verification
                        .as_ref()
                        .map(|v| v.enabled)
                        .unwrap_or(true);
                    if verification_enabled {
                        let reviews_dir_abs = control_plane_path.join(".openloop").join("reviews");
                        let verifier_prompt = build_verifier_prompt(&task, spec_content.as_deref(), &reviews_dir_abs);
                        let verdicts_path = control_plane_path
                            .join(".openloop")
                            .join("verifications")
                            .join(format!("{}.json", task.id));
                        // Verdicts from a previous attempt are never valid for this one.
                        let _ = std::fs::remove_file(&verdicts_path);
                        let verifier_outcome = match hooks.verifier_runner.as_mut() {
                            Some(vr) => {
                                let _ = vr(&verifier_prompt);
                                read_verifier_outcome(&verdicts_path)
                            }
                            None => {
                                let verifier_exit = match hooks.pi_runner.as_mut() {
                                    Some(runner) => {
                                        runner(&run_project, &verifier_prompt, model.as_deref(), get_remaining_timeout_ms())
                                            .map(|r| r.exit_code)
                                            .unwrap_or(1)
                                    }
                                    None => super::pi::run_agent(
                                        &super::pi::PiRunOptions {
                                            prompt: &verifier_prompt,
                                            model: model.as_deref(),
                                            project: &run_project,
                                            timeout_ms: get_remaining_timeout_ms(),
                                        },
                                        Some(&project_config),
                                        default_provider.as_deref(),
                                    )
                                    .map(|r| r.exit_code)
                                    .unwrap_or(1),
                                };
                                let _ = verifier_exit;
                                read_verifier_outcome(&verdicts_path)
                            }
                        };
                        if verifier_outcome.failed {
                            task.status = TaskStatus::Failed;
                            task.attempts += 1;
                            task.last_failure_signature = Some("verifier-rejected".to_string());
                            for criterion in &verifier_outcome.failing_criteria {
                                push_note(&mut task, &format!("Verifier rejected criterion {criterion}."));
                            }
                            outcome = RunOutcome::ValidationFailed;
                        } else {
                            task.status = TaskStatus::Done;
                            push_note(&mut task, &format!("Openloop {mode} run succeeded."));
                            if verifier_outcome.requires_review {
                                push_note(&mut task, "Verifier verdict requires human review.");
                                verifier_requires_review = true;
                            }
                        }
                    } else {
                        task.status = TaskStatus::Done;
                        push_note(&mut task, &format!("Openloop {mode} run succeeded."));
                    }
                    if task.status == TaskStatus::Done
                        && project_config
                            .review
                            .as_ref()
                            .map(|r| r.enabled)
                            .unwrap_or(false)
                    {
                        let reviews_dir_abs = control_plane_path.join(".openloop").join("reviews");
                        let review_prompt = super::review::build_review_prompt(&task, &get_diff_text(&execution_path), &reviews_dir_abs);
                        let _ = ensure_dir(&reviews_dir_abs);
                        let reviewer_exit = match hooks.reviewer_runner.as_mut() {
                            Some(rr) => rr(&review_prompt).unwrap_or(1),
                            None => match hooks.pi_runner.as_mut() {
                                Some(runner) => runner(&run_project, &review_prompt, model.as_deref(), get_remaining_timeout_ms())
                                    .map(|r| r.exit_code)
                                    .unwrap_or(1),
                                None => super::pi::run_agent(
                                    &super::pi::PiRunOptions {
                                        prompt: &review_prompt,
                                        model: model.as_deref(),
                                        project: &run_project,
                                        timeout_ms: get_remaining_timeout_ms(),
                                    },
                                    Some(&project_config),
                                    default_provider.as_deref(),
                                )
                                .map(|r| r.exit_code)
                                .unwrap_or(1),
                            },
                        };
                        let _ = reviewer_exit;
                        review_result = Some(super::review::review_from_agent_files(
                            &control_plane_path,
                            &execution_path,
                            &task,
                            &project_policy,
                        ));
                        let rr = review_result.as_ref().unwrap();
                        if rr.malformed == Some(true) {
                            push_note(&mut task, "Reviewer output malformed; downgraded to manual review.");
                        }
                        if !rr.findings.is_empty() {
                            for f in &rr.findings {
                                push_note(&mut task, &format!("Review [{}] {}: {}", f.severity.as_str(), f.rule, f.message));
                            }
                            if rr.has_blocking {
                                push_note(&mut task, "Openloop review found blocking issues; downgrading auto-merge to manual review.");
                            }
                        }
                    }
                }
            } else {
                // W1: After a successful plan run, detect spec file written by Pi.
                let _ = ensure_dir(&project_path.join(".openloop").join("specs"));
                detect_and_set_spec_id(project_path, &mut task);
                // B2: Gate medium/high-risk tasks for human approval after planning.
                if task.risk == TaskRisk::MediumRisk || task.risk == TaskRisk::HighRisk {
                    task.status = TaskStatus::AwaitingApproval;
                    let risk_label = task.risk.as_str().to_string();
                    push_note(
                        &mut task,
                        &format!("Openloop {mode} run succeeded; awaiting human approval ({risk_label})."),
                    );
                } else {
                    task.status = TaskStatus::Ready;
                    push_note(&mut task, &format!("Openloop {mode} run succeeded."));
                }
                outcome = RunOutcome::Planned;
            }
        } else {
            task.status = previous_status;
            task.attempts += 1;
            task.last_failure_signature = Some(format!("pi-exit-{exit_code}"));
            push_note(&mut task, &format!("Openloop {mode} run failed with exit {exit_code}."));
            outcome = RunOutcome::PiFailed;
        }

        let effective_promotion_mode = resolve_effective_promotion_mode(
            &task,
            &project_policy,
            project_config.risk.require_policy_for_auto_merge,
        );
        let mut promotion_decision = decide_promotion(&task, &validation, effective_promotion_mode, &project_config);
        if (review_result.as_ref().map(|r| r.has_blocking || r.malformed == Some(true)).unwrap_or(false) || verifier_requires_review)
            && promotion_decision == PromotionDecision::AutoMergeEligible
        {
            promotion_decision = PromotionDecision::ManualReview;
        }
        let after_fingerprint = get_git_diff_fingerprint(&execution_path);
        {
            let np_input = NoProgressInput {
                previous_failure_signature: previous_failure_signature.as_deref(),
                previous_promotion_decision,
                current_promotion_decision: promotion_decision,
                before_fingerprint: before_fingerprint.clone(),
                after_fingerprint: after_fingerprint.clone(),
                no_progress_repeat_limit,
            };
            if should_stop_for_no_progress(&mut task, &np_input) {
                task.status = TaskStatus::Blocked;
                push_note(&mut task, "Openloop blocked task due to no-progress detection.");
                stopped_by = RunStoppedBy::NoProgress;
            }
        }

        let promotion_action = decide_promotion_action(promotion_decision);
        let run_cost = resolve_run_cost(&run_result, estimated_cost_per_run);
        // Human-approval packet for anything landing in a promotion queue: the
        // deterministic summary path is computed up front so the packet can
        // reference it before either artifact is written.
        let summary_timestamp = now_iso().replace(':', "-");
        let summary_path = control_plane_path
            .join(".openloop")
            .join("runs")
            .join(format!("{summary_timestamp}-{}.md", task.id));
        if promotion_action == PromotionAction::QueueReview || promotion_action == PromotionAction::QueueAutoMerge {
            let diff_stat = get_git_diff_stat(&execution_path, git_state.as_deref(), None);
            approval_packet_path = super::approval_packets::write_approval_packet(
                &control_plane_path,
                &task,
                &super::approval_packets::ApprovalPacketData {
                    diff_stat,
                    validation: validation.clone(),
                    review_findings: review_result.as_ref().map(|r| r.findings.clone()).unwrap_or_default(),
                    cost_usd: run_cost.0,
                    cost_source: run_cost.1,
                    run_summary_path: Some(summary_path.to_string_lossy().to_string()),
                    spec_path: task
                        .spec_id
                        .as_ref()
                        .map(|spec_id| control_plane_path.join(spec_id).to_string_lossy().to_string()),
                },
            )
            .ok();
        }

        let promotion_artifact_path = maybe_write_promotion_artifact(
            project_path,
            &project.alias,
            &task.id,
            git_state.as_deref(),
            promotion_decision,
            promotion_action,
            effective_promotion_mode,
            &validation,
            Some(exit_code),
            outcome,
            approval_packet_path.as_deref(),
        )?;
        task.last_run = Some(TaskRunSummary {
            completed_at: now_iso(),
            mode: mode.clone(),
            role: Some(role),
            pi_exit_code: Some(exit_code),
            outcome,
            base_branch: git_state.clone(),
            validation: validation.clone(),
            promotion_decision,
            effective_promotion_mode,
            promotion_action,
            promotion_artifact_path: promotion_artifact_path.clone(),
            promotion_artifact_state: PromotionArtifactStatus::Pending,
            promotion_result_artifact_path: None,
        });

        task.updated_at = now_iso();
        super::task_ledger::upsert_task(project_path, &task)?;

        let mut result = idle_result(&project.alias, &selection.reason, model.as_deref());
        result.task_id = Some(task.id.clone());
        result.mode = mode.clone();
        result.role = Some(role);
        result.exit_code = Some(exit_code);
        result.prompt = Some(built_prompt.clone());
        result.validation = validation.clone();
        result.promotion_decision = promotion_decision;
        result.promotion_action = promotion_action;
        result.promotion_artifact_path = promotion_artifact_path;
        result.task_status = Some(task.status);
        result.promoted_at = task.promoted_at.clone();
        result.stopped_by = stopped_by;
        result.attempt_number = Some(attempt_number);
        result.dirty_tree_detected = before_fingerprint != after_fingerprint;
        result.budget_snapshot_usd = task.estimated_cost_usd;
        result.review_findings = review_result.as_ref().map(|r| r.findings.clone()).filter(|f| !f.is_empty());
        result.cost_usd = Some(run_cost.0);
        result.cost_source = Some(run_cost.1);
        result.approval_packet_path = approval_packet_path.clone();
        let written_summary = super::run_summaries::write_run_summary(project_path, &result, Some(&summary_path))?;
        result.run_summary_path = Some(written_summary);
        write_run_transcript(&control_plane_path, &task.id, &run_result);
        cleanup_worktree();
        Ok(result)
    })();

    match worktree_result {
        Ok(result) => Ok(result),
        Err(error) => {
            task.status = previous_status;
            task.attempts += 1;
            let is_timeout = error.starts_with("__iteration_timeout__");
            task.last_failure_signature = Some(if is_timeout {
                "timeout".to_string()
            } else {
                error.clone()
            });
            push_note(&mut task, &format!("Openloop {mode} run threw an error."));
            let after_fingerprint = get_git_diff_fingerprint(&execution_path);
            let stopped_by = if is_timeout {
                RunStoppedBy::Timeout
            } else {
                let np_input = NoProgressInput {
                    previous_failure_signature: previous_failure_signature.as_deref(),
                    previous_promotion_decision,
                    current_promotion_decision: PromotionDecision::Blocked,
                    before_fingerprint: before_fingerprint.clone(),
                    after_fingerprint: after_fingerprint.clone(),
                    no_progress_repeat_limit,
                };
                if should_stop_for_no_progress(&mut task, &np_input) {
                    push_note(&mut task, "Openloop blocked task due to no-progress detection.");
                    RunStoppedBy::NoProgress
                } else {
                    RunStoppedBy::None
                }
            };
            if stopped_by == RunStoppedBy::NoProgress {
                task.status = TaskStatus::Blocked;
            }
            let effective_promotion_mode = resolve_effective_promotion_mode(
                &task,
                &project_policy,
                project_config.risk.require_policy_for_auto_merge,
            );
            let promotion_action = decide_promotion_action(PromotionDecision::Blocked);
            let promotion_artifact_path = maybe_write_promotion_artifact(
                project_path,
                &project.alias,
                &task.id,
                git_state.as_deref(),
                PromotionDecision::Blocked,
                promotion_action,
                effective_promotion_mode,
                &validation,
                None,
                RunOutcome::Error,
                None,
            )?;
            task.last_run = Some(TaskRunSummary {
                completed_at: now_iso(),
                mode: mode.clone(),
                role: Some(role),
                pi_exit_code: None,
                outcome: RunOutcome::Error,
                base_branch: git_state.clone(),
                validation: validation.clone(),
                promotion_decision: PromotionDecision::Blocked,
                effective_promotion_mode,
                promotion_action,
                promotion_artifact_path: promotion_artifact_path.clone(),
                promotion_artifact_state: PromotionArtifactStatus::Pending,
                promotion_result_artifact_path: None,
            });
            task.updated_at = now_iso();
            super::task_ledger::upsert_task(project_path, &task)?;

            let mut result = idle_result(&project.alias, &selection.reason, model.as_deref());
            result.task_id = Some(task.id.clone());
            result.mode = mode.clone();
            result.role = Some(role);
            result.prompt = Some(built_prompt.clone());
            result.validation = validation.clone();
            result.promotion_decision = PromotionDecision::Blocked;
            result.promotion_action = promotion_action;
            result.promotion_artifact_path = promotion_artifact_path;
            result.task_status = Some(task.status);
            result.promoted_at = task.promoted_at.clone();
            result.stopped_by = stopped_by;
            result.attempt_number = Some(attempt_number);
            result.dirty_tree_detected = before_fingerprint != after_fingerprint;
            result.cost_usd = agent_started.then_some(estimated_cost_per_run);
            result.cost_source = agent_started.then_some(CostSource::Estimated);
            let catch_summary_path = super::run_summaries::write_run_summary(project_path, &result, None)?;
            result.run_summary_path = Some(catch_summary_path.clone());
            // A packet written before the failure referenced the (never-written)
            // success-path summary — repoint it at the summary that does exist.
            if let Some(packet_path) = &approval_packet_path {
                fix_approval_packet_summary_path(packet_path, &catch_summary_path);
            }
            cleanup_worktree();
            Err(error)
        }
    }
}

fn push_note(task: &mut ProjectTask, note: &str) {
    let mut notes = task.notes.take().unwrap_or_default();
    notes.push(note.to_string());
    task.notes = Some(notes);
}

fn read_verifier_outcome(verdicts_path: &Path) -> VerifierOutcome {
    let mut verdicts: Vec<serde_json::Value> = Vec::new();
    let mut missing_or_malformed = true;
    if verdicts_path.exists() {
        if let Ok(raw) = std::fs::read_to_string(verdicts_path) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(arr) = parsed.as_array() {
                    verdicts = arr.clone();
                    missing_or_malformed = false;
                }
            }
        }
    }
    if missing_or_malformed {
        return VerifierOutcome {
            failed: false,
            requires_review: true,
            failing_criteria: Vec::new(),
        };
    }
    let failing_criteria: Vec<String> = verdicts
        .iter()
        .filter(|v| v.get("verdict").and_then(|x| x.as_str()) == Some("fail"))
        .map(|v| {
            if let Some(index) = v.get("index").and_then(|x| x.as_f64()) {
                format!("{index:.0}")
            } else if let Some(criterion) = v.get("criterion").and_then(|x| x.as_str()) {
                criterion.to_string()
            } else {
                "unknown".to_string()
            }
        })
        .collect();
    if !failing_criteria.is_empty() {
        return VerifierOutcome {
            failed: true,
            requires_review: false,
            failing_criteria,
        };
    }
    if verdicts
        .iter()
        .any(|v| v.get("verdict").and_then(|x| x.as_str()) == Some("needs-human"))
    {
        return VerifierOutcome {
            failed: false,
            requires_review: true,
            failing_criteria: Vec::new(),
        };
    }
    VerifierOutcome {
        failed: false,
        requires_review: false,
        failing_criteria: Vec::new(),
    }
}

pub fn _unused_async_bridge() {
    // read_spec_content is declared async in TS; the Rust port is synchronous.
}

fn get_diff_text(execution_path: &Path) -> String {
    super::git::get_diff_patch(execution_path).unwrap_or_default()
}
