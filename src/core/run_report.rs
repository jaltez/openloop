use chrono::TimeZone;
use std::path::Path;

use super::git::{get_git_diff_stat, DiffStatEntry};
use super::now_iso;
use super::types::*;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReportEntry {
    pub task_id: String,
    pub title: String,
    pub risk: String,
    pub status: String,
    #[serde(default)]
    pub branch: Option<String>,
    pub changed_files: Vec<DiffStatEntry>,
    pub validation: Vec<String>,
    pub promotion: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReportSection {
    pub alias: String,
    pub completed: Vec<TaskReportEntry>,
    pub failed: Vec<TaskReportEntry>,
    pub blocked: Vec<TaskReportEntry>,
    pub in_progress: Vec<TaskReportEntry>,
    pub idle: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub generated_at: String,
    pub period_label: String,
    pub budget_spent_usd: f64,
    pub budget_remaining_usd: f64,
    pub total_completed: usize,
    pub total_failed: usize,
    pub total_blocked: usize,
    pub total_idle: usize,
    pub projects: Vec<ProjectReportSection>,
}

fn build_task_entry(task: &ProjectTask, project_path: &Path) -> TaskReportEntry {
    let mut validation_summary: Vec<String> = Vec::new();
    if let Some(last_run) = &task.last_run {
        for v in &last_run.validation {
            validation_summary.push(format!(
                "{} {}",
                v.name.as_str(),
                if v.exit_code == 0 { "✅" } else { "⚠️" }
            ));
        }
    }

    let mut changed_files: Vec<DiffStatEntry> = Vec::new();
    if let (Some(branch), Some(last_run)) = (&task.branch, &task.last_run) {
        if let Some(base_branch) = &last_run.base_branch {
            if let Some(stats) = get_git_diff_stat(project_path, Some(base_branch), Some(branch)) {
                changed_files = stats;
            }
        }
    }

    TaskReportEntry {
        task_id: task.id.clone(),
        title: task.title.clone(),
        risk: task.risk.as_str().to_string(),
        status: task.status.as_str().to_string(),
        branch: task.branch.clone(),
        changed_files,
        validation: validation_summary,
        promotion: task
            .last_run
            .as_ref()
            .map(|lr| lr.promotion_action.as_str().to_string())
            .unwrap_or_else(|| task.promotion.as_str().to_string()),
    }
}

pub fn generate_run_report(
    since_ms: Option<i64>,
    app_home_override: Option<&str>,
) -> Result<RunReport, String> {
    let since_ms = since_ms.unwrap_or(24 * 60 * 60 * 1000); // Default 24h
    let config = super::global_config::load_global_config(app_home_override)?;
    let daemon_state = super::daemon_state::load_daemon_state(app_home_override)?;
    let projects = super::project_registry::list_projects(app_home_override)?;

    let mut project_sections = Vec::new();

    for project in &projects {
        let project_path = Path::new(&project.path);
        let ledger = match super::task_ledger::load_task_ledger(project_path) {
            Ok(l) => l,
            Err(_) => {
                project_sections.push(ProjectReportSection {
                    alias: project.alias.clone(),
                    completed: Vec::new(),
                    failed: Vec::new(),
                    blocked: Vec::new(),
                    in_progress: Vec::new(),
                    idle: true,
                });
                continue;
            }
        };

        let since = chrono::Utc
            .timestamp_millis_opt(super::now_millis() - since_ms)
            .single()
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
            .unwrap_or_default();
        let recently_updated: Vec<&ProjectTask> = ledger
            .tasks
            .iter()
            .filter(|t| t.updated_at.as_str() >= since.as_str())
            .collect();

        let completed: Vec<TaskReportEntry> = recently_updated
            .iter()
            .filter(|t| t.status == TaskStatus::Done || t.status == TaskStatus::Promoted)
            .map(|t| build_task_entry(t, project_path))
            .collect();
        let failed: Vec<TaskReportEntry> = recently_updated
            .iter()
            .filter(|t| t.status == TaskStatus::Failed)
            .map(|t| build_task_entry(t, project_path))
            .collect();
        let blocked: Vec<TaskReportEntry> = recently_updated
            .iter()
            .filter(|t| t.status == TaskStatus::Blocked)
            .map(|t| build_task_entry(t, project_path))
            .collect();
        let in_progress: Vec<TaskReportEntry> = recently_updated
            .iter()
            .filter(|t| t.status == TaskStatus::InProgress || t.status == TaskStatus::Ready)
            .map(|t| build_task_entry(t, project_path))
            .collect();

        let idle = completed.is_empty()
            && failed.is_empty()
            && blocked.is_empty()
            && in_progress.is_empty();
        project_sections.push(ProjectReportSection {
            alias: project.alias.clone(),
            completed,
            failed,
            blocked,
            in_progress,
            idle,
        });
    }

    let total_completed: usize = project_sections.iter().map(|p| p.completed.len()).sum();
    let total_failed: usize = project_sections.iter().map(|p| p.failed.len()).sum();
    let total_blocked: usize = project_sections.iter().map(|p| p.blocked.len()).sum();
    let total_idle = project_sections.iter().filter(|p| p.idle).count();

    let period_label = if since_ms == 24 * 60 * 60 * 1000 {
        "last 24 hours".to_string()
    } else {
        format!("last {}h", (since_ms as f64 / 3_600_000.0).round() as i64)
    };

    Ok(RunReport {
        generated_at: now_iso(),
        period_label,
        budget_spent_usd: daemon_state.budget_spent_usd,
        budget_remaining_usd: (config.budgets.daily_cost_usd - daemon_state.budget_spent_usd)
            .max(0.0),
        total_completed,
        total_failed,
        total_blocked,
        total_idle,
        projects: project_sections,
    })
}

pub fn format_run_report(report: &RunReport) -> String {
    let mut lines: Vec<String> = Vec::new();
    // TS rendered new Date(report.generatedAt) — the header is a pure
    // function of the report, not the formatting wall clock.
    let date = chrono::DateTime::parse_from_rfc3339(&report.generated_at)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%B %-d, %Y")
                .to_string()
        })
        .unwrap_or_else(|_| chrono::Local::now().format("%B %-d, %Y").to_string());

    lines.push(format!("=== OpenLoop Summary — {date} ==="));
    lines.push(String::new());
    lines.push(format!(
        "✅ {} completed | ⚠️ {} failed | 🚫 {} blocked | 💤 {} idle",
        report.total_completed, report.total_failed, report.total_blocked, report.total_idle
    ));
    lines.push(format!(
        "💰 ${:.2} spent (${:.2} remaining)",
        report.budget_spent_usd, report.budget_remaining_usd
    ));

    for section in &report.projects {
        lines.push(String::new());
        lines.push(format!("{}:", section.alias));

        if section.idle {
            lines.push("  💤 No activity in this period".to_string());
            continue;
        }

        for task in &section.completed {
            lines.push(format!(
                "  ✅ {} — {} ({}, {})",
                task.task_id, task.title, task.risk, task.promotion
            ));
            if let Some(branch) = &task.branch {
                lines.push(format!("     Branch: {branch}"));
            }
            for f in &task.changed_files {
                lines.push(format!(
                    "     Changed: {} (+{}, -{})",
                    f.file, f.added, f.removed
                ));
            }
            if !task.validation.is_empty() {
                lines.push(format!("     Validations: {}", task.validation.join(" | ")));
            }
        }

        for task in &section.failed {
            lines.push(format!(
                "  ❌ {} — {} ({})",
                task.task_id, task.title, task.risk
            ));
            if let Some(branch) = &task.branch {
                lines.push(format!("     Branch: {branch}"));
            }
            for f in &task.changed_files {
                lines.push(format!(
                    "     Changed: {} (+{}, -{})",
                    f.file, f.added, f.removed
                ));
            }
            if !task.validation.is_empty() {
                lines.push(format!("     Validations: {}", task.validation.join(" | ")));
            }
        }

        for task in &section.blocked {
            lines.push(format!(
                "  🚫 {} — {} ({})",
                task.task_id, task.title, task.risk
            ));
        }

        for task in &section.in_progress {
            lines.push(format!(
                "  🔄 {} — {} ({}, {})",
                task.task_id, task.title, task.risk, task.status
            ));
        }
    }

    lines.join("\n")
}
