import path from "node:path";
import { readJsonFile, writeJsonFile } from "./fs.js";
import type { ProjectTask, TaskLedger } from "./types.js";

export interface TaskListFilters {
  status?: ProjectTask["status"];
  risk?: ProjectTask["risk"];
}

export interface TaskListSummary {
  total: number;
  byStatus: Record<ProjectTask["status"], number>;
  byRisk: Record<ProjectTask["risk"], number>;
}

export async function loadTaskLedger(projectPath: string): Promise<TaskLedger> {
  const ledgerPath = path.join(projectPath, ".openloop", "tasks.json");
  return readJsonFile<TaskLedger>(ledgerPath, {
    version: 1,
    updatedAt: new Date(0).toISOString(),
    tasks: [],
  });
}

export async function saveTaskLedger(projectPath: string, ledger: TaskLedger): Promise<void> {
  const ledgerPath = path.join(projectPath, ".openloop", "tasks.json");
  ledger.updatedAt = new Date().toISOString();
  await writeJsonFile(ledgerPath, ledger);
}

export async function addTask(projectPath: string, task: ProjectTask): Promise<void> {
  const ledger = await loadTaskLedger(projectPath);
  ledger.tasks.push(task);
  await saveTaskLedger(projectPath, ledger);
}

export async function getTask(projectPath: string, taskId: string): Promise<ProjectTask> {
  const ledger = await loadTaskLedger(projectPath);
  const task = ledger.tasks.find((candidate) => candidate.id === taskId);
  if (!task) {
    throw new Error(`Unknown task id: ${taskId}`);
  }
  return task;
}

export async function listTasks(projectPath: string, filters: TaskListFilters = {}): Promise<ProjectTask[]> {
  const ledger = await loadTaskLedger(projectPath);
  return ledger.tasks
    .filter((task) => (filters.status ? task.status === filters.status : true))
    .filter((task) => (filters.risk ? task.risk === filters.risk : true))
    .sort((left, right) => {
      const updatedOrder = right.updatedAt.localeCompare(left.updatedAt);
      if (updatedOrder !== 0) {
        return updatedOrder;
      }
      return left.id.localeCompare(right.id);
    });
}

export function summarizeTasks(tasks: ProjectTask[]): TaskListSummary {
  const byStatus: Record<ProjectTask["status"], number> = {
    proposed: 0,
    planned: 0,
    ready: 0,
    in_progress: 0,
    blocked: 0,
    done: 0,
    failed: 0,
    cancelled: 0,
    promoted: 0,
  };
  const byRisk: Record<ProjectTask["risk"], number> = {
    "low-risk": 0,
    "medium-risk": 0,
    "high-risk": 0,
  };

  for (const task of tasks) {
    byStatus[task.status] += 1;
    byRisk[task.risk] += 1;
  }

  return {
    total: tasks.length,
    byStatus,
    byRisk,
  };
}

export function summarizeQueue(ledger: TaskLedger): { queueSize: number; blockedTasks: number } {
  return {
    queueSize: ledger.tasks.filter((task) => ["proposed", "planned", "ready", "in_progress"].includes(task.status)).length,
    blockedTasks: ledger.tasks.filter((task) => task.status === "blocked").length,
  };
}