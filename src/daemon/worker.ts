import fs from "node:fs/promises";
import { createDefaultDaemonState, loadDaemonState, localDateStamp, saveDaemonState } from "../core/daemon-state.js";
import { ensureDir } from "../core/fs.js";
import { loadGlobalConfig } from "../core/global-config.js";
import { daemonLogPath, daemonPidPath, runtimeDir } from "../core/paths.js";
import { listProjects } from "../core/project-registry.js";
import { loadProjectQueueStates, selectNextProject } from "../core/project-selection.js";
import { loadTaskLedger } from "../core/task-ledger.js";
import { determineWorkerRole, runProjectIteration, selectNextTask } from "../core/scheduler.js";
import type { DaemonState } from "../core/types.js";

export async function startWorkerLoop(): Promise<void> {
  await ensureDir(runtimeDir());
  const startedAt = new Date().toISOString();
  await fs.writeFile(daemonPidPath(), `${process.pid}\n`, "utf8");

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

  while (!shuttingDown) {
    await runWorkerTick({ startedAt });
    await fs.appendFile(daemonLogPath(), `[${new Date().toISOString()}] tick\n`, "utf8");
    await sleep(5000);
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