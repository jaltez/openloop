import path from "node:path";
import { readJsonFile, writeJsonFile } from "./fs.js";
import { withFileLock } from "./file-lock.js";
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
  await withTaskLedger(projectPath, (ledger) => {
    for (const ref of task.dependsOn ?? []) {
      if (ref.includes(":")) {
        continue; // cross-project refs are accepted and reserved
      }
      if (!ledger.tasks.some((existing) => existing.id === ref)) {
        throw new Error(`Unknown task id in dependsOn: ${ref}`);
      }
    }
    if (ledger.tasks.some((existing) => existing.id === task.id)) {
      task.id = `${task.id}-${Date.now()}`;
    }
    ledger.tasks.push(task);
  });
}

/**
 * Replace or append a single task under the ledger lock. The scheduler holds a
 * long-lived in-memory task object across an agent run; persisting it through
 * an upsert (instead of saving the whole stale in-memory ledger) preserves
 * concurrent edits to other tasks made by the CLI/TUI in the meantime.
 */
export async function upsertTask(projectPath: string, task: ProjectTask): Promise<void> {
  await withTaskLedger(projectPath, (ledger) => {
    const index = ledger.tasks.findIndex((existing) => existing.id === task.id);
    if (index === -1) {
      ledger.tasks.push(task);
    } else {
      ledger.tasks[index] = task;
    }
  });
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
    "awaiting-approval": 0,
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
    queueSize: ledger.tasks.filter((task) => ["proposed", "planned", "ready", "awaiting-approval", "in_progress"].includes(task.status))
      .length,
    blockedTasks: ledger.tasks.filter((task) => task.status === "blocked").length,
  };
}

export async function updateTask(
  projectPath: string,
  taskId: string,
  patch: Partial<Pick<ProjectTask, "title" | "status" | "risk" | "kind" | "scope">>,
): Promise<ProjectTask> {
  return withTaskLedger(projectPath, (ledger) => {
    const task = ledger.tasks.find((candidate) => candidate.id === taskId);
    if (!task) {
      throw new Error(`Unknown task id: ${taskId}`);
    }
    if (patch.title !== undefined) task.title = patch.title;
    if (patch.status !== undefined) task.status = patch.status;
    if (patch.risk !== undefined) task.risk = patch.risk;
    if (patch.kind !== undefined) task.kind = patch.kind;
    if (patch.scope !== undefined) task.scope = patch.scope;
    task.updatedAt = new Date().toISOString();
    return task;
  });
}

export async function removeTask(projectPath: string, taskId: string): Promise<void> {
  await withTaskLedger(projectPath, (ledger) => {
    const index = ledger.tasks.findIndex((candidate) => candidate.id === taskId);
    if (index === -1) {
      throw new Error(`Unknown task id: ${taskId}`);
    }
    ledger.tasks.splice(index, 1);
  });
}

// ---------------------------------------------------------------------------
// Ledger lock — daemon, CLI, and TUI processes mutate tasks.json concurrently;
// every load-modify-save cycle must hold this cross-process lock.
// ---------------------------------------------------------------------------

function tasksLockPath(projectPath: string): string {
  return path.join(projectPath, ".openloop", ".tasks.lock");
}

/**
 * Run a load-modify-save cycle on the task ledger under the cross-process
 * `.tasks.lock`. The mutator receives the freshly loaded ledger and mutates it
 * in place (or returns a value); the ledger is persisted on completion.
 */
export async function withTaskLedger<T>(projectPath: string, mutator: (ledger: TaskLedger) => Promise<T> | T): Promise<T> {
  return withFileLock(tasksLockPath(projectPath), { label: "Task ledger" }, async () => {
    const ledger = await loadTaskLedger(projectPath);
    const result = await mutator(ledger);
    await saveTaskLedger(projectPath, ledger);
    return result;
  });
}
