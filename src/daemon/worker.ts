import fs from "node:fs/promises";
import { spawn } from "node:child_process";
import { createDefaultDaemonState, loadDaemonState, localDateStamp, pauseDaemon, saveDaemonState } from "../core/daemon-state.js";
import { ensureDir } from "../core/fs.js";
import { loadGlobalConfig } from "../core/global-config.js";
import { appendEvent } from "../core/event-log.js";
import { daemonLogPath, daemonPidPath, runtimeDir } from "../core/paths.js";
import { listProjects } from "../core/project-registry.js";
import { loadProjectQueueStates, selectNextProject } from "../core/project-selection.js";
import { loadTaskLedger, saveTaskLedger } from "../core/task-ledger.js";
import { determineWorkerRole, runProjectIteration, selectNextTask } from "../core/scheduler.js";
import type { DaemonState, GlobalConfig, SchedulerResult } from "../core/types.js";

export async function startWorkerLoop(options?: { foreground?: boolean }): Promise<void> {
  await ensureDir(runtimeDir());
  await rotateLogIfNeeded(daemonLogPath());
  const startedAt = new Date().toISOString();
  await fs.writeFile(daemonPidPath(), `${process.pid}\n`, "utf8");
  await appendEvent({ ts: startedAt, event: "daemon_started", pid: process.pid }).catch(() => {});
  await recoverStuckTasks();

  let shuttingDown = false;

  const shutdown = async () => {
    if (shuttingDown) return;
    shuttingDown = true;
    await fs.rm(daemonPidPath(), { force: true });
    await fs.appendFile(daemonLogPath(), `[${new Date().toISOString()}] shutdown\n`, "utf8");
    process.exit(0);
  };

  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);

  const MAX_CONSECUTIVE_ERRORS = 5;
  let consecutiveErrors = 0;

  while (!shuttingDown) {
    try {
      const state = await runWorkerTick({ startedAt });
      consecutiveErrors = 0;
      await fs.appendFile(daemonLogPath(), `[${new Date().toISOString()}] tick\n`, "utf8");
      if (options?.foreground) {
        const ts = new Date().toISOString().slice(11, 19);
        const proj = state.activeProject ?? "idle";
        const run = state.currentRun;
        const summary = run
          ? `${proj} | ${run.taskId ?? "--"} | mode:${run.mode}`
          : `${proj} | no run`;
        process.stderr.write(`[${ts}] ${summary}\n`);
      }
    } catch (error) {
      consecutiveErrors++;
      const detail = error instanceof Error ? (error.stack ?? error.message) : String(error);
      await fs.appendFile(
        daemonLogPath(),
        `[${new Date().toISOString()}] tick error (${consecutiveErrors}/${MAX_CONSECUTIVE_ERRORS}): ${detail}\n`,
        "utf8",
      ).catch(() => {});
      if (options?.foreground) {
        process.stderr.write(`[${new Date().toISOString().slice(11, 19)}] tick error: ${detail.split("\n")[0]}\n`);
      }
      if (consecutiveErrors >= MAX_CONSECUTIVE_ERRORS) {
        await fs.appendFile(
          daemonLogPath(),
          `[${new Date().toISOString()}] max consecutive errors reached; entering degraded (paused) state\n`,
          "utf8",
        ).catch(() => {});
        await pauseDaemon().catch(() => {});
        consecutiveErrors = 0;
      }
    }
    // W9: Use configurable tick interval from global config.
    const tickConfig = await loadGlobalConfig().catch(() => null);
    const tickMs = (tickConfig?.runtime.tickIntervalSeconds ?? 5) * 1000;
    await sleep(tickMs);
  }
}

export async function runWorkerTick(options?: {
  startedAt?: string;
  runProjectIterationFn?: typeof runProjectIteration;
}): Promise<DaemonState> {
  const startedAt = options?.startedAt ?? new Date().toISOString();
  const config = await loadGlobalConfig();
  const existingState = await loadDaemonState();
  const normalizedState = normalizeDaemonBudgetState(existingState, new Date(startedAt));
  const projects = await listProjects();
  const queueStates = await loadProjectQueueStates();

  if (normalizedState.paused) {
    const pausedState = buildDaemonState({
      existingState: normalizedState,
      startedAt,
      projects,
      queueStates,
      activeProjectAlias: null,
      lastIterationAt: new Date().toISOString(),
      iterationResult: "paused",
      currentRun: normalizedState.currentRun,
    });
    await saveDaemonState(pausedState);
    return pausedState;
  }

  if (normalizedState.budgetSpentUsd >= config.budgets.dailyCostUsd) {
    normalizedState.budgetBlocked = true;
    const blockedState = buildDaemonState({
      existingState: normalizedState,
      startedAt,
      projects,
      queueStates,
      activeProjectAlias: null,
      lastIterationAt: new Date().toISOString(),
      iterationResult: "budget-blocked",
      currentRun: null,
    });
    await saveDaemonState(blockedState);
    if (config.notifications?.onBudgetBlocked) {
      fireShellCommand(config.notifications.onBudgetBlocked, {
        OPENLOOP_EVENT: "budget-blocked",
        OPENLOOP_PROJECT: "",
        OPENLOOP_TASK_ID: "",
      }).catch(() => {});
    }
    return blockedState;
  }

  const activeProject = await selectNextProject();
  let iterationResult: string | null = null;
  let currentRun: DaemonState["currentRun"] = null;

  if (activeProject) {
    const ledger = await loadTaskLedger(activeProject.path);
    const selection = selectNextTask(ledger);
    if (selection.task && selection.mode !== "idle") {
      const startedAtRun = new Date();
      currentRun = {
        projectAlias: activeProject.alias,
        taskId: selection.task.id,
        mode: selection.mode,
        role: determineWorkerRole(selection.task, selection.mode),
        startedAt: startedAtRun.toISOString(),
        deadlineAt: new Date(startedAtRun.getTime() + config.runtime.runTimeoutSeconds * 1000).toISOString(),
        attemptNumber: selection.task.attempts + 1,
        pauseRequestedAt: normalizedState.pausedAt,
      };
    }
  }

  const preRunState = buildDaemonState({
    existingState: normalizedState,
    startedAt,
    projects,
    queueStates,
    activeProjectAlias: activeProject?.alias ?? null,
    lastIterationAt: null,
    iterationResult: null,
    currentRun,
  });
  await saveDaemonState(preRunState);

  if (activeProject) {
    try {
      const result = await (options?.runProjectIterationFn ?? runProjectIteration)(activeProject, {
        timeoutMs: config.runtime.runTimeoutSeconds * 1000,
        maxAttemptsPerTask: config.runtime.maxAttemptsPerTask,
        noProgressRepeatLimit: config.runtime.noProgressRepeatLimit,
      });
      iterationResult = `${result.mode}:${result.taskId ?? "none"}:${result.exitCode ?? "idle"}:${result.stoppedBy}`;
      if (result.exitCode !== null) {
        const cost = config.budgets.estimatedCostPerRunUsd ?? 0;
        normalizedState.budgetSpentUsd = parseFloat((normalizedState.budgetSpentUsd + cost).toFixed(4));
        normalizedState.totalBudgetSpentUsd = parseFloat((normalizedState.totalBudgetSpentUsd + cost).toFixed(4));
      }
      // D3: Append structured event to events.jsonl audit trail.
      await appendEvent({
        ts: new Date().toISOString(),
        event: result.exitCode !== null ? "pi_completed" : (result.mode === "idle" ? "idle" : "task_skipped"),
        project: activeProject.alias,
        taskId: result.taskId ?? undefined,
        exitCode: result.exitCode,
        mode: result.mode,
        role: result.role ?? undefined,
        stoppedBy: result.stoppedBy,
        taskStatus: result.taskStatus ?? undefined,
      }).catch(() => {});
      fireNotification(config, result, activeProject.alias).catch(() => {});
    } catch (error) {
      iterationResult = error instanceof Error ? error.message : String(error);
    }
  }

  const postRunState = buildDaemonState({
    existingState: normalizedState,
    startedAt,
    projects,
    queueStates,
    activeProjectAlias: activeProject?.alias ?? null,
    lastIterationAt: new Date().toISOString(),
    iterationResult,
    currentRun: null,
  });
  await saveDaemonState(postRunState);
  return postRunState;
}

function buildDaemonState(input: {
  existingState: DaemonState;
  startedAt: string;
  projects: Awaited<ReturnType<typeof listProjects>>;
  queueStates: Awaited<ReturnType<typeof loadProjectQueueStates>>;
  activeProjectAlias: string | null;
  lastIterationAt: string | null;
  iterationResult: string | null;
  currentRun: DaemonState["currentRun"];
}): DaemonState {
  return createDefaultDaemonState({
    startedAt: input.startedAt,
    pid: process.pid,
    activeProject: input.activeProjectAlias,
    paused: input.existingState.paused,
    pausedAt: input.existingState.pausedAt,
    totalBudgetSpentUsd: input.existingState.totalBudgetSpentUsd,
    budgetDate: input.existingState.budgetDate,
    budgetSpentUsd: input.existingState.budgetSpentUsd,
    budgetBlocked: input.existingState.budgetBlocked,
    currentRun: input.currentRun,
    projects: input.projects.map((project) => {
      const queue = input.queueStates.find((state) => state.project.alias === project.alias);
      return {
        alias: project.alias,
        queueSize: queue?.queueSize ?? 0,
        paused: input.existingState.paused,
        lastIterationAt: input.lastIterationAt,
        lastResult: project.alias === input.activeProjectAlias ? input.iterationResult ?? "idle" : "idle",
        blockedTasks: queue?.blockedTasks ?? 0,
      };
    }),
  });
}

function normalizeDaemonBudgetState(state: DaemonState, now: Date): DaemonState {
  const currentDate = localDateStamp(now);
  if (state.budgetDate === currentDate) {
    return state;
  }

  return {
    ...state,
    budgetDate: currentDate,
    budgetSpentUsd: 0,
    budgetBlocked: false,
  };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function fireNotification(
  config: GlobalConfig,
  result: SchedulerResult,
  projectAlias: string,
): Promise<void> {
  const env: Record<string, string> = {
    OPENLOOP_PROJECT: projectAlias,
    OPENLOOP_TASK_ID: result.taskId ?? "",
    OPENLOOP_EVENT: "",
  };

  if (result.taskStatus === "done" && config.notifications?.onTaskComplete) {
    env.OPENLOOP_EVENT = "task-complete";
    await fireShellCommand(config.notifications.onTaskComplete, env);
  } else if (
    (result.taskStatus === "failed" || result.taskStatus === "blocked") &&
    config.notifications?.onTaskFailed
  ) {
    env.OPENLOOP_EVENT = "task-failed";
    await fireShellCommand(config.notifications.onTaskFailed, env);
  } else if (result.mode === "idle" && config.notifications?.onAllTasksDone) {
    env.OPENLOOP_EVENT = "all-tasks-done";
    await fireShellCommand(config.notifications.onAllTasksDone, env);
  }
}

async function fireShellCommand(command: string, env: Record<string, string>): Promise<void> {
  return new Promise((resolve) => {
    const child = spawn("sh", ["-c", command], {
      env: { ...process.env, ...env },
      stdio: "ignore",
      detached: false,
    });
    child.on("close", () => resolve());
    child.on("error", () => resolve());
  });
}

async function recoverStuckTasks(): Promise<void> {
  try {
    const projects = await listProjects();
    for (const project of projects) {
      try {
        const ledger = await loadTaskLedger(project.path);
        const stuckTasks = ledger.tasks.filter((task) => task.status === "in_progress");
        if (stuckTasks.length === 0) continue;

        for (const task of stuckTasks) {
          task.status = "ready";
          task.notes = [...(task.notes ?? []), "Openloop recovered task from in_progress on daemon startup."];
          task.updatedAt = new Date().toISOString();
        }
        await saveTaskLedger(project.path, ledger);
        await fs.appendFile(
          daemonLogPath(),
          `[${new Date().toISOString()}] recovered ${stuckTasks.length} stuck task(s) in ${project.alias}\n`,
          "utf8",
        ).catch(() => {});
      } catch {
        // ignore per-project errors during recovery
      }
    }
  } catch {
    // ignore registry errors during recovery
  }
}

// W3: Rotate daemon.log if it exceeds 10 MB. Keep last 3 rotated files.
const LOG_ROTATE_MAX_BYTES = 10 * 1024 * 1024; // 10 MB
const LOG_ROTATE_MAX_BACKUPS = 3;

async function rotateLogIfNeeded(logPath: string): Promise<void> {
  try {
    const stat = await fs.stat(logPath).catch(() => null);
    if (!stat || stat.size < LOG_ROTATE_MAX_BYTES) {
      return;
    }

    // Shift existing backups: .3 is dropped, .2 -> .3, .1 -> .2, current -> .1
    for (let index = LOG_ROTATE_MAX_BACKUPS - 1; index >= 1; index--) {
      const src = `${logPath}.${index}`;
      const dest = `${logPath}.${index + 1}`;
      await fs.rename(src, dest).catch(() => {});
    }
    await fs.rename(logPath, `${logPath}.1`).catch(() => {});
  } catch {
    // non-fatal — rotation failure should not block startup
  }
}
