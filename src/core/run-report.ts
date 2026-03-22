import { listProjects } from "./project-registry.js";
import { loadDaemonState } from "./daemon-state.js";
import { loadGlobalConfig } from "./global-config.js";
import { loadTaskLedger } from "./task-ledger.js";
import { getGitDiffStat, type DiffStatEntry } from "./git.js";
import type { ProjectTask } from "./types.js";

export interface ProjectReportSection {
  alias: string;
  completed: TaskReportEntry[];
  failed: TaskReportEntry[];
  blocked: TaskReportEntry[];
  inProgress: TaskReportEntry[];
  idle: boolean;
}

export interface TaskReportEntry {
  taskId: string;
  title: string;
  risk: string;
  status: string;
  branch: string | null;
  changedFiles: DiffStatEntry[];
  validation: string[];
  promotion: string;
}

export interface RunReport {
  generatedAt: string;
  periodLabel: string;
  budgetSpentUsd: number;
  budgetRemainingUsd: number;
  totalCompleted: number;
  totalFailed: number;
  totalBlocked: number;
  totalIdle: number;
  projects: ProjectReportSection[];
}

async function buildTaskEntry(task: ProjectTask, projectPath: string): Promise<TaskReportEntry> {
  const validationSummary: string[] = [];
  if (task.lastRun?.validation) {
    for (const v of task.lastRun.validation) {
      validationSummary.push(`${v.name} ${v.exitCode === 0 ? "✅" : "⚠️"}`);
    }
  }

  let changedFiles: DiffStatEntry[] = [];
  if (task.branch && task.lastRun?.baseBranch) {
    const stats = await getGitDiffStat(projectPath, task.lastRun.baseBranch, task.branch);
    if (stats) changedFiles = stats;
  }

  return {
    taskId: task.id,
    title: task.title,
    risk: task.risk,
    status: task.status,
    branch: task.branch,
    changedFiles,
    validation: validationSummary,
    promotion: task.lastRun?.promotionAction ?? task.promotion,
  };
}

export async function generateRunReport(options?: {
  sinceMs?: number;
  appHomeOverride?: string;
}): Promise<RunReport> {
  const sinceMs = options?.sinceMs ?? 24 * 60 * 60 * 1000; // Default 24h
  const config = await loadGlobalConfig(options?.appHomeOverride);
  const daemonState = await loadDaemonState(options?.appHomeOverride);
  const projects = await listProjects(options?.appHomeOverride);

  const projectSections: ProjectReportSection[] = [];

  for (const project of projects) {
    let ledger;
    try {
      ledger = await loadTaskLedger(project.path);
    } catch {
      projectSections.push({
        alias: project.alias,
        completed: [],
        failed: [],
        blocked: [],
        inProgress: [],
        idle: true,
      });
      continue;
    }

    const cutoff = new Date(Date.now() - sinceMs).toISOString();
    const recentlyUpdated = ledger.tasks.filter((t) => t.updatedAt >= cutoff);

    const completed = await Promise.all(
      recentlyUpdated.filter((t) => t.status === "done" || t.status === "promoted").map((t) => buildTaskEntry(t, project.path)),
    );
    const failed = await Promise.all(
      recentlyUpdated.filter((t) => t.status === "failed").map((t) => buildTaskEntry(t, project.path)),
    );
    const blocked = await Promise.all(
      recentlyUpdated.filter((t) => t.status === "blocked").map((t) => buildTaskEntry(t, project.path)),
    );
    const inProgress = await Promise.all(
      recentlyUpdated.filter((t) => t.status === "in_progress" || t.status === "ready").map((t) => buildTaskEntry(t, project.path)),
    );

    projectSections.push({
      alias: project.alias,
      completed,
      failed,
      blocked,
      inProgress,
      idle: completed.length === 0 && failed.length === 0 && blocked.length === 0 && inProgress.length === 0,
    });
  }

  const totalCompleted = projectSections.reduce((s, p) => s + p.completed.length, 0);
  const totalFailed = projectSections.reduce((s, p) => s + p.failed.length, 0);
  const totalBlocked = projectSections.reduce((s, p) => s + p.blocked.length, 0);
  const totalIdle = projectSections.filter((p) => p.idle).length;

  return {
    generatedAt: new Date().toISOString(),
    periodLabel: sinceMs === 24 * 60 * 60 * 1000 ? "last 24 hours" : `last ${Math.round(sinceMs / 3600000)}h`,
    budgetSpentUsd: daemonState.budgetSpentUsd,
    budgetRemainingUsd: Math.max(0, config.budgets.dailyCostUsd - daemonState.budgetSpentUsd),
    totalCompleted,
    totalFailed,
    totalBlocked,
    totalIdle,
    projects: projectSections,
  };
}

export function formatRunReport(report: RunReport): string {
  const lines: string[] = [];
  const date = new Date(report.generatedAt).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
  });

  lines.push(`=== OpenLoop Summary — ${date} ===`);
  lines.push("");
  lines.push(
    `✅ ${report.totalCompleted} completed | ⚠️ ${report.totalFailed} failed | 🚫 ${report.totalBlocked} blocked | 💤 ${report.totalIdle} idle`,
  );
  lines.push(
    `💰 $${report.budgetSpentUsd.toFixed(2)} spent ($${report.budgetRemainingUsd.toFixed(2)} remaining)`,
  );

  for (const section of report.projects) {
    lines.push("");
    lines.push(`${section.alias}:`);

    if (section.idle) {
      lines.push("  💤 No activity in this period");
      continue;
    }

    for (const task of section.completed) {
      lines.push(`  ✅ ${task.taskId} — ${task.title} (${task.risk}, ${task.promotion})`);
      if (task.branch) lines.push(`     Branch: ${task.branch}`);
      for (const f of task.changedFiles) {
        lines.push(`     Changed: ${f.file} (+${f.added}, -${f.removed})`);
      }
      if (task.validation.length > 0) lines.push(`     Validations: ${task.validation.join(" | ")}`);
    }

    for (const task of section.failed) {
      lines.push(`  ❌ ${task.taskId} — ${task.title} (${task.risk})`);
      if (task.branch) lines.push(`     Branch: ${task.branch}`);
      for (const f of task.changedFiles) {
        lines.push(`     Changed: ${f.file} (+${f.added}, -${f.removed})`);
      }
      if (task.validation.length > 0) lines.push(`     Validations: ${task.validation.join(" | ")}`);
    }

    for (const task of section.blocked) {
      lines.push(`  🚫 ${task.taskId} — ${task.title} (${task.risk})`);
    }

    for (const task of section.inProgress) {
      lines.push(`  🔄 ${task.taskId} — ${task.title} (${task.risk}, ${task.status})`);
    }
  }

  return lines.join("\n");
}
