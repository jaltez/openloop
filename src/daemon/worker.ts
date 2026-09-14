import fs from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import { loadDaemonState, localDateStamp, pauseDaemon, withDaemonState } from "../core/daemon-state.js";
import { ensureDir, rotateLogFile } from "../core/fs.js";
import { loadGlobalConfig } from "../core/global-config.js";
import { appendEvent } from "../core/event-log.js";
import { buildDigest } from "../core/digest.js";
import { matchesFiveFieldCron } from "../core/cron.js";
import { listPromotionArtifacts } from "../core/promotion-queue.js";
import { daemonLogPath, daemonPidPath, runtimeDir } from "../core/paths.js";
import { listProjects } from "../core/project-registry.js";
import { loadProjectQueueStates, selectNextProject } from "../core/project-selection.js";
import { loadTaskLedger, withTaskLedger } from "../core/task-ledger.js";
import { RunTimeoutError } from "../core/timeout.js";
import { determineWorkerRole, runProjectIteration, selectNextTask } from "../core/scheduler.js";
import { fireNotifications } from "../core/notifications.js";
import { killActiveRuns } from "../core/active-runs.js";
import { deleteBranch, removeWorktree } from "../core/git.js";
import { loadProjectConfig, saveProjectConfig } from "../core/project-config.js";
import { syncIssues, postTaskStatusToIssue, postPrLinkToIssue } from "../core/issue-sync.js";
import { createDashboardServer, type DashboardServer } from "../core/dashboard.js";
import { runLifecycleHooks, type LifecycleHookPayload } from "../core/hooks.js";
import { downgradePendingAutoMergePromotionToReview } from "../core/promotion-queue.js";
import type { DaemonState, GlobalConfig, LinkedProject, ProjectSchedule, SchedulerResult } from "../core/types.js";

export async function startWorkerLoop(options?: { foreground?: boolean }): Promise<void> {
  await ensureDir(runtimeDir());
  await rotateLogIfNeeded(daemonLogPath());
  const startedAt = new Date().toISOString();
  await fs.writeFile(daemonPidPath(), `${process.pid}\n`, "utf8");
  await appendEvent({ ts: startedAt, event: "daemon_started", pid: process.pid }).catch(() => {});
  await recoverStuckTasks();

  // A3: Start dashboard server if enabled
  let dashboardServer: DashboardServer | null = null;
  try {
    const initConfig = await loadGlobalConfig();
    if (initConfig.dashboard?.enabled) {
      dashboardServer = createDashboardServer(initConfig.dashboard.port);
      await dashboardServer.start();
      await fs
        .appendFile(daemonLogPath(), `[${new Date().toISOString()}] dashboard started on port ${dashboardServer.port}\n`, "utf8")
        .catch(() => {});
    }
  } catch {
    await fs.appendFile(daemonLogPath(), `[${new Date().toISOString()}] dashboard failed to start\n`, "utf8").catch(() => {});
  }

  let shuttingDown = false;

  const shutdown = async () => {
    if (shuttingDown) return;
    shuttingDown = true;
    // Kill running agent process groups so shutdown doesn't orphan them.
    killActiveRuns();
    if (dashboardServer) {
      await dashboardServer.stop().catch(() => {});
    }
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
        const summary = run ? `${proj} | ${run.taskId ?? "--"} | mode:${run.mode}` : `${proj} | no run`;
        process.stderr.write(`[${ts}] ${summary}\n`);
      }
    } catch (error) {
      consecutiveErrors++;
      const detail = error instanceof Error ? (error.stack ?? error.message) : String(error);
      await fs
        .appendFile(
          daemonLogPath(),
          `[${new Date().toISOString()}] tick error (${consecutiveErrors}/${MAX_CONSECUTIVE_ERRORS}): ${detail}\n`,
          "utf8",
        )
        .catch(() => {});
      if (options?.foreground) {
        process.stderr.write(`[${new Date().toISOString().slice(11, 19)}] tick error: ${detail.split("\n")[0]}\n`);
      }
      if (consecutiveErrors >= MAX_CONSECUTIVE_ERRORS) {
        await fs
          .appendFile(
            daemonLogPath(),
            `[${new Date().toISOString()}] max consecutive errors reached; entering degraded (paused) state\n`,
            "utf8",
          )
          .catch(() => {});
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
  /** Clock used for schedule evaluation (defaults to the real tick time). */
  now?: Date;
}): Promise<DaemonState> {
  const startedAt = options?.startedAt ?? new Date().toISOString();
  const scheduleNow = options?.now ?? new Date();
  const config = await loadGlobalConfig();
  const normalizedState = normalizeDaemonBudgetState(await loadDaemonState(), new Date(startedAt));
  const projects = await listProjects();
  const queueStates = await loadProjectQueueStates();

  // Every persist takes the state lock, loads FRESH state, and touches only
  // the fields this tick owns. Anything set concurrently — a pause from the
  // CLI or MCP, an external budget write — survives; the tick never writes a
  // stale snapshot over it.
  const persistTickState = (view: {
    activeProjectAlias: string | null;
    lastIterationAt: string | null;
    iterationResult: string | null;
    update: (state: DaemonState) => void;
  }): Promise<DaemonState> =>
    withDaemonState((state) => {
      state.startedAt = startedAt;
      state.pid = process.pid;
      state.projects = projects.map((project) => {
        const queue = queueStates.find((candidate) => candidate.project.alias === project.alias);
        const previous = state.projects.find((candidate) => candidate.alias === project.alias);
        return {
          alias: project.alias,
          queueSize: queue?.queueSize ?? 0,
          paused: state.paused,
          lastIterationAt:
            project.alias === view.activeProjectAlias && view.lastIterationAt !== null
              ? view.lastIterationAt
              : (previous?.lastIterationAt ?? null),
          lastResult: project.alias === view.activeProjectAlias ? (view.iterationResult ?? "idle") : (previous?.lastResult ?? "idle"),
          blockedTasks: queue?.blockedTasks ?? 0,
        };
      });
      view.update(state);
    });

  if (normalizedState.paused) {
    return persistTickState({
      activeProjectAlias: null,
      lastIterationAt: new Date().toISOString(),
      iterationResult: "paused",
      // paused/pausedAt/currentRun are owned by pause/resume — untouched.
      update: () => {},
    });
  }

  // Daily digest: fire once per local calendar day on the first tick. The
  // date is persisted BEFORE the hooks run (at-most-once: a hook crash never
  // re-fires the digest on every subsequent tick).
  const today = localDateStamp(scheduleNow);
  if (normalizedState.lastDigestDate !== today) {
    await withDaemonState((state) => {
      state.lastDigestDate = today;
    }).catch(() => {});
    try {
      const digest = await buildDigest({ sinceMs: 24 * 60 * 60 * 1000 });
      const payload: LifecycleHookPayload = {
        event: "daily-digest",
        project: "",
        taskId: "",
        message:
          `Daily digest: ${digest.projects.length} project(s), ` +
          `${digest.projects.reduce((sum, project) => sum + project.reviewQueue.length, 0)} pending review(s).`,
        timestamp: new Date().toISOString(),
        mode: "idle",
        digest,
      };
      await runLifecycleHooks({
        globalConfig: config,
        payload,
        daemonLogPath: daemonLogPath(),
      }).catch(() => {});
    } catch {
      // Digest failures never block the tick.
    }
  }

  if (normalizedState.budgetSpentUsd >= config.budgets.dailyCostUsd) {
    const blockedState = await persistTickState({
      activeProjectAlias: null,
      lastIterationAt: new Date().toISOString(),
      iterationResult: "budget-blocked",
      update: (state) => {
        state.budgetBlocked = true;
        state.activeProject = null;
        state.currentRun = null;
      },
    });
    const payload: LifecycleHookPayload = {
      event: "budget-blocked",
      project: "",
      taskId: "",
      message: `Daily budget exhausted at $${normalizedState.budgetSpentUsd.toFixed(4)}.`,
      timestamp: new Date().toISOString(),
      mode: "idle",
      budgetSnapshotUsd: normalizedState.budgetSpentUsd,
    };
    await runLifecycleHooks({
      globalConfig: config,
      payload,
      daemonLogPath: daemonLogPath(),
    }).catch(() => {});
    await fireNotifications(config, payload).catch(() => {});
    if (config.notifications?.onBudgetBlocked) {
      fireShellCommand(config.notifications?.onBudgetBlocked, {
        OPENLOOP_EVENT: "budget-blocked",
        OPENLOOP_PROJECT: "",
        OPENLOOP_TASK_ID: "",
      }).catch(() => {});
    }
    return blockedState;
  }

  // Daemon-native schedules: evaluate every initialized project's schedules
  // each tick. Schedules only add tasks — they never preempt active work.
  for (const project of projects) {
    if (!project.initialized) {
      continue;
    }
    await evaluateSchedules(project, scheduleNow).catch(async (error) => {
      const detail = error instanceof Error ? error.message : String(error);
      await fs
        .appendFile(daemonLogPath(), `[${new Date().toISOString()}] schedule evaluation failed in ${project.alias}: ${detail}\n`, "utf8")
        .catch(() => {});
    });
  }

  const activeProject = await selectNextProject();

  // Review backpressure: a project whose pending promotions pile up must be
  // reviewed before more agent runs land on top of them.
  if (activeProject) {
    const maxPendingReviews = config.runtime.maxPendingReviewsPerProject ?? 3;
    if (maxPendingReviews > 0) {
      const pendingCount = (await listPromotionArtifacts(activeProject.path)).filter((item) => item.artifact.status === "pending").length;
      if (pendingCount >= maxPendingReviews) {
        const backpressureState = await persistTickState({
          activeProjectAlias: activeProject.alias,
          lastIterationAt: new Date().toISOString(),
          iterationResult: "review-backpressure",
          update: (state) => {
            state.activeProject = activeProject.alias;
            state.currentRun = null;
          },
        });
        await fs
          .appendFile(
            daemonLogPath(),
            `[${new Date().toISOString()}] review-backpressure: ${pendingCount} pending promotion(s) >= ${maxPendingReviews} in ${activeProject.alias}\n`,
            "utf8",
          )
          .catch(() => {});
        const payload: LifecycleHookPayload = {
          event: "review-backpressure",
          project: activeProject.alias,
          taskId: "",
          message: `Skipping ${activeProject.alias}: ${pendingCount} pending promotions awaiting review (limit ${maxPendingReviews}).`,
          timestamp: new Date().toISOString(),
          mode: "idle",
          pendingPromotions: pendingCount,
        };
        await runLifecycleHooks({
          globalConfig: config,
          payload,
          daemonLogPath: daemonLogPath(),
        }).catch(() => {});
        return backpressureState;
      }
    }
  }

  let iterationResult: string | null = null;
  let currentRun: DaemonState["currentRun"] = null;
  let chargeUsd = 0;

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

  await persistTickState({
    activeProjectAlias: activeProject?.alias ?? null,
    lastIterationAt: null,
    iterationResult: null,
    update: (state) => {
      state.activeProject = activeProject?.alias ?? null;
      state.currentRun = currentRun;
    },
  });
  if (activeProject) {
    try {
      const result = await (options?.runProjectIterationFn ?? runProjectIteration)(activeProject, {
        timeoutMs: config.runtime.runTimeoutSeconds * 1000,
        maxAttemptsPerTask: config.runtime.maxAttemptsPerTask,
        noProgressRepeatLimit: config.runtime.noProgressRepeatLimit,
      });
      iterationResult = `${result.mode}:${result.taskId ?? "none"}:${result.exitCode ?? "idle"}:${result.stoppedBy}`;
      if (result.exitCode !== null) {
        // Measured provider cost when parsed, else the configured estimate.
        // Timeout-killed runs never parse usage, so they charge the estimate.
        const cost = result.costUsd ?? config.budgets.estimatedCostPerRunUsd ?? 0;
        chargeUsd = cost;

        // B3: Accumulate estimated cost on the task itself.
        if (result.taskId && cost > 0) {
          await withTaskLedger(activeProject.path, (taskLedger) => {
            const task = taskLedger.tasks.find((t) => t.id === result.taskId);
            if (task) {
              task.estimatedCostUsd = parseFloat(((task.estimatedCostUsd ?? 0) + cost).toFixed(4));
              task.updatedAt = new Date().toISOString();
            }
          }).catch(() => {});
        }
      }
      // D3: Append structured event to events.jsonl audit trail.
      await appendEvent({
        ts: new Date().toISOString(),
        event: result.exitCode !== null ? "pi_completed" : result.mode === "idle" ? "idle" : "task_skipped",
        project: activeProject.alias,
        taskId: result.taskId ?? undefined,
        exitCode: result.exitCode,
        mode: result.mode,
        role: result.role ?? undefined,
        stoppedBy: result.stoppedBy,
        taskStatus: result.taskStatus ?? undefined,
      }).catch(() => {});
      const projConfig = await loadProjectConfig(activeProject.path).catch(() => null);
      await emitLifecycleHooksAndNotifications(config, projConfig, activeProject.path, result, activeProject.alias).catch(() => {});

      // A2: Post task status back to linked issue (if configured)
      if (result.taskId && result.taskStatus) {
        postTaskStatusToIssue(activeProject.path, result.taskId, result.taskStatus).catch(() => {});
      }

      // A2: Post PR link back to issue when a promotion creates a PR
      if (result.taskId && result.promotionResultArtifactPath) {
        import("../core/fs.js")
          .then(({ readJsonFile }) => readJsonFile(result.promotionResultArtifactPath!, null as never))
          .then((artifact: { prUrl?: string | null; branch?: string | null } | null) => {
            if (artifact?.prUrl) {
              postPrLinkToIssue(activeProject.path, result.taskId!, artifact.prUrl, artifact.branch).catch(() => {});
            }
          })
          .catch(() => {});
      }

      // A2: Auto-sync issues if configured
      if (projConfig?.issueSource?.autoSync && projConfig.issueSource.token) {
        const lastSynced = projConfig.issueSource.lastSyncedAt ? new Date(projConfig.issueSource.lastSyncedAt).getTime() : 0;
        const intervalMs = (projConfig.issueSource.syncIntervalMinutes ?? 30) * 60_000;
        if (Date.now() - lastSynced >= intervalMs) {
          syncIssues(activeProject.path, projConfig.issueSource).catch(() => {});
        }
      }
    } catch (error) {
      iterationResult = error instanceof Error ? error.message : String(error);
      // A timed-out run consumed budget (no parseable usage) — charge the estimate.
      if (error instanceof RunTimeoutError) {
        chargeUsd = config.budgets.estimatedCostPerRunUsd ?? 0;
      }
    }
  }

  const postRunState = await persistTickState({
    activeProjectAlias: activeProject?.alias ?? null,
    lastIterationAt: new Date().toISOString(),
    iterationResult,
    update: (state) => {
      state.activeProject = activeProject?.alias ?? null;
      state.currentRun = null;
      chargeBudget(state, chargeUsd, new Date(startedAt));
    },
  });
  return postRunState;
}

/**
 * Apply a budget charge as a DELTA to freshly loaded state (never a stale
 * snapshot), rolling over the daily window when the calendar day changed.
 */
function chargeBudget(state: DaemonState, costUsd: number, now: Date): void {
  const currentDate = localDateStamp(now);
  if (state.budgetDate !== currentDate) {
    state.budgetDate = currentDate;
    state.budgetSpentUsd = 0;
    state.budgetBlocked = false;
  }
  if (costUsd > 0) {
    state.budgetSpentUsd = parseFloat((state.budgetSpentUsd + costUsd).toFixed(4));
    state.totalBudgetSpentUsd = parseFloat((state.totalBudgetSpentUsd + costUsd).toFixed(4));
  }
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

async function fireNotification(config: GlobalConfig, result: SchedulerResult, projectAlias: string): Promise<void> {
  const env: Record<string, string> = {
    OPENLOOP_PROJECT: projectAlias,
    OPENLOOP_TASK_ID: result.taskId ?? "",
    OPENLOOP_EVENT: "",
  };

  let eventName = "";
  let message = "";

  if (result.taskStatus === "done") {
    eventName = "task-complete";
    message = `Task ${result.taskId ?? "unknown"} completed in project ${projectAlias}`;
    if (config.notifications?.onTaskComplete) {
      env.OPENLOOP_EVENT = eventName;
      await fireShellCommand(config.notifications.onTaskComplete, env);
    }
  } else if (result.taskStatus === "failed" || result.taskStatus === "blocked") {
    eventName = "task-failed";
    message = `Task ${result.taskId ?? "unknown"} ${result.taskStatus} in project ${projectAlias}`;
    if (config.notifications?.onTaskFailed) {
      env.OPENLOOP_EVENT = eventName;
      await fireShellCommand(config.notifications.onTaskFailed, env);
    }
  } else if (result.mode === "idle") {
    eventName = "all-tasks-done";
    message = `All tasks done in project ${projectAlias}`;
    if (config.notifications?.onAllTasksDone) {
      env.OPENLOOP_EVENT = eventName;
      await fireShellCommand(config.notifications.onAllTasksDone, env);
    }
  }

  void message;
}

async function emitLifecycleHooksAndNotifications(
  config: GlobalConfig,
  projectConfig: Awaited<ReturnType<typeof loadProjectConfig>> | null,
  projectPath: string,
  result: SchedulerResult,
  projectAlias: string,
): Promise<void> {
  const initialPayloads = buildLifecyclePayloads(result, projectAlias);
  const hookNotes: string[] = [];
  let requireManualReview = false;

  for (const payload of initialPayloads) {
    const hookResult = await runLifecycleHooks({
      globalConfig: config,
      projectConfig,
      payload,
      daemonLogPath: daemonLogPath(),
    });
    hookNotes.push(...hookResult.notes.map((note) => `${payload.event}: ${note}`));
    requireManualReview ||= hookResult.requireManualReview;
  }

  if (requireManualReview && result.taskId && result.promotionAction === "queue-auto-merge") {
    const downgraded = await downgradePendingAutoMergePromotionToReview(projectPath, result.taskId);
    if (downgraded) {
      result.promotionDecision = "manual-review";
      result.promotionAction = "queue-review";
      hookNotes.push("promotion-auto-merge-queued: Downgraded to manual review by lifecycle hook.");
    }
  }

  const finalPayloads = buildLifecyclePayloads(result, projectAlias);
  await Promise.allSettled(finalPayloads.map((payload) => fireNotifications(config, payload)));
  await persistHookNotes(projectPath, result, hookNotes);
  await fireNotification(config, result, projectAlias);
}
function buildLifecyclePayloads(result: SchedulerResult, projectAlias: string): LifecycleHookPayload[] {
  const base = {
    project: projectAlias,
    taskId: result.taskId ?? "",
    timestamp: new Date().toISOString(),
    mode: result.mode,
    exitCode: result.exitCode,
    taskStatus: result.taskStatus,
    role: result.role,
    stoppedBy: result.stoppedBy,
    promotionDecision: result.promotionDecision,
    promotionAction: result.promotionAction,
    budgetSnapshotUsd: result.budgetSnapshotUsd,
    validation: result.validation,
    approvalPacketPath: result.approvalPacketPath ?? null,
  };
  const payloads: LifecycleHookPayload[] = [];

  if (result.taskStatus === "done") {
    payloads.push({
      ...base,
      event: "task-complete",
      message: `Task ${result.taskId ?? "unknown"} completed in project ${projectAlias}`,
    });
  } else if (result.taskStatus === "failed" || result.taskStatus === "blocked") {
    payloads.push({
      ...base,
      event: "task-failed",
      message: `Task ${result.taskId ?? "unknown"} ${result.taskStatus} in project ${projectAlias}`,
    });
  } else if (result.taskStatus === "awaiting-approval") {
    payloads.push({
      ...base,
      event: "task-awaiting-approval",
      message: `Task ${result.taskId ?? "unknown"} is awaiting approval in project ${projectAlias}`,
    });
  } else if (result.mode === "idle") {
    payloads.push({
      ...base,
      event: "all-tasks-done",
      message: `All tasks done in project ${projectAlias}`,
    });
  }

  if (result.taskStatus === "failed" && result.validation.some((item) => item.exitCode !== 0)) {
    payloads.push({
      ...base,
      event: "validation-failed",
      message: `Validation failed for task ${result.taskId ?? "unknown"} in project ${projectAlias}`,
    });
  }

  if (result.promotionAction === "queue-auto-merge") {
    payloads.push({
      ...base,
      event: "promotion-auto-merge-queued",
      message: `Promotion queued for auto-merge for task ${result.taskId ?? "unknown"} in project ${projectAlias}`,
    });
  } else if (result.promotionAction === "queue-review") {
    payloads.push({
      ...base,
      event: "promotion-review-queued",
      message: `Promotion queued for review for task ${result.taskId ?? "unknown"} in project ${projectAlias}`,
    });
  } else if (result.promotionAction === "block") {
    payloads.push({
      ...base,
      event: "promotion-blocked",
      message: `Promotion blocked for task ${result.taskId ?? "unknown"} in project ${projectAlias}`,
    });
  }

  return payloads;
}

async function persistHookNotes(projectPath: string, result: SchedulerResult, notes: string[]): Promise<void> {
  if (notes.length === 0) {
    return;
  }

  if (result.taskId) {
    await withTaskLedger(projectPath, (ledger) => {
      const task = ledger.tasks.find((candidate) => candidate.id === result.taskId);
      if (task) {
        task.notes = [...(task.notes ?? []), ...notes.map((note) => `Hook: ${note}`)];
        task.updatedAt = new Date().toISOString();
      }
    }).catch(() => {});
  }

  if (result.runSummaryPath) {
    const section = ["", "## Hook Notes", ...notes.map((note) => `- ${note}`), ""].join("\n");
    await fs.appendFile(result.runSummaryPath, section, "utf8").catch(() => {});
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

const SCHEDULE_MAX_CATCHUP_MINUTES = 7 * 24 * 60; // bound catch-up after long downtime

function toMinuteIso(date: Date): string {
  return new Date(Math.floor(date.getTime() / 60000) * 60000).toISOString();
}

/**
 * Fire due schedules for one project: enqueue a task for every schedule whose
 * cron matched in (lastFiredMinute, nowMinute], deduplicating against tasks
 * already created for that schedule after the last fire.
 */
async function evaluateSchedules(project: LinkedProject, now: Date): Promise<void> {
  const config = await loadProjectConfig(project.path);
  const schedules = config.schedule ?? [];
  if (schedules.length === 0) {
    return;
  }

  const nowMinute = toMinuteIso(now);
  const scheduleState = { ...(config.scheduleState ?? {}) };
  let mutated = false;

  for (const schedule of schedules) {
    try {
      await evaluateSingleSchedule({ project, now, nowMinute, schedule, lastFired: scheduleState[schedule.id] ?? null, scheduleState });
      mutated = scheduleState[schedule.id] === nowMinute || mutated;
    } catch (error) {
      // A malformed cron must not poison the project's other schedules.
      const detail = error instanceof Error ? error.message : String(error);
      await fs
        .appendFile(
          daemonLogPath(),
          `[${new Date().toISOString()}] schedule ${schedule.id} failed in ${project.alias}: ${detail}\n`,
          "utf8",
        )
        .catch(() => {});
    }
  }

  if (mutated) {
    await saveProjectConfig(project.path, { ...config, scheduleState });
  }
}

async function evaluateSingleSchedule(input: {
  project: LinkedProject;
  now: Date;
  nowMinute: string;
  schedule: ProjectSchedule;
  lastFired: string | null;
  scheduleState: Record<string, string>;
}): Promise<void> {
  const { project, now, nowMinute, schedule, scheduleState } = input;
  const lastFired = input.lastFired;
  if (!lastFired) {
    // Fresh installs don't backfill — arm the schedule from now on.
    scheduleState[schedule.id] = nowMinute;
    return;
  }

  const catchupStart = Math.max(Date.parse(lastFired), now.getTime() - SCHEDULE_MAX_CATCHUP_MINUTES * 60000);
  let fired = false;
  for (let ts = catchupStart + 60000; ts <= now.getTime(); ts += 60000) {
    if (matchesFiveFieldCron(schedule.cron, new Date(ts))) {
      fired = true;
      break;
    }
  }
  if (!fired) {
    return;
  }

  await withTaskLedger(project.path, (ledger) => {
    const alreadyQueued = ledger.tasks.some(
      (task) => task.source.type === "schedule" && task.source.ref === schedule.id && task.createdAt > lastFired,
    );
    if (alreadyQueued) {
      return;
    }
    const nowIso = new Date().toISOString();
    ledger.tasks.push({
      id: `${schedule.id}-${Math.floor(now.getTime() / 60000)}`,
      title: schedule.title,
      kind: "feature",
      status: "proposed",
      risk: "medium-risk", // conservative default for unattended scheduled work
      scope: null,
      source: { type: "schedule", ref: schedule.id },
      specId: null,
      branch: null,
      owner: "openloop",
      acceptanceCriteria: schedule.prompt ? [schedule.prompt] : ["To be defined during planning."],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "pull-request",
      notes: [`Created by schedule ${schedule.id} (${schedule.cron}).`],
      createdAt: nowIso,
      updatedAt: nowIso,
    });
  });
  scheduleState[schedule.id] = nowMinute;
  await appendEvent({
    ts: new Date().toISOString(),
    event: "schedule_fired",
    project: project.alias,
    taskId: schedule.id,
  }).catch(() => {});
}

export async function recoverStuckTasks(): Promise<void> {
  try {
    const projects = await listProjects();
    for (const project of projects) {
      try {
        const recovered = await withTaskLedger(project.path, (ledger) =>
          ledger.tasks.filter((task) => {
            if (task.status !== "in_progress") return false;
            task.status = "ready";
            task.notes = [...(task.notes ?? []), "Openloop recovered task from in_progress on daemon startup."];
            task.updatedAt = new Date().toISOString();
            return true;
          }),
        );
        if (recovered.length > 0) {
          await fs
            .appendFile(
              daemonLogPath(),
              `[${new Date().toISOString()}] recovered ${recovered.length} stuck task(s) in ${project.alias}\n`,
              "utf8",
            )
            .catch(() => {});
        }
        await reclaimStaleWorktrees(project);
      } catch {
        // ignore per-project errors during recovery
      }
    }
  } catch {
    // ignore registry errors during recovery
  }
}

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

// Reclaim worktrees whose task is no longer in_progress: a crashed run leaves
// the checkout behind and wedges the next iteration's worktree add.
async function reclaimStaleWorktrees(project: LinkedProject): Promise<void> {
  const worktreesDir = path.join(project.path, ".openloop", "worktrees");
  const entries = await fs.readdir(worktreesDir, { withFileTypes: true }).catch(() => null);
  if (!entries) {
    return;
  }

  const config = await loadProjectConfig(project.path).catch(() => null);
  const branchPrefix = config?.runtime.branchPrefix ?? "openloop/";
  const ledger = await loadTaskLedger(project.path);

  for (const entry of entries) {
    if (!entry.isDirectory()) {
      continue;
    }
    const taskId = entry.name;
    const worktreePath = path.join(worktreesDir, taskId);

    // A concurrent CLI run or second daemon writes a pid marker at worktree
    // creation; never force-remove a checkout whose owner is still alive.
    const ownerPid = await fs
      .readFile(path.join(worktreePath, ".openloop-run.pid"), "utf8")
      .then((raw) => Number.parseInt(raw, 10))
      .catch(() => null);
    if (ownerPid !== null && Number.isInteger(ownerPid) && isPidAlive(ownerPid)) {
      continue; // live cross-process run — leave it alone
    }

    const task = ledger.tasks.find((candidate) => candidate.id === taskId);
    if (task?.status === "in_progress") {
      continue; // live run — leave it alone
    }

    try {
      await removeWorktree(project.path, worktreePath, true);
      // Remove strays that git doesn't know about (never a registered worktree).
      await fs.rm(worktreePath, { recursive: true, force: true }).catch(() => {});
      // Abandoned tasks also drop their branch; done/awaiting tasks may hold
      // unpromoted work — keep theirs.
      if (task && (task.status === "blocked" || task.status === "cancelled")) {
        await deleteBranch(project.path, `${branchPrefix}${taskId}`).catch(() => {});
      }
      await fs
        .appendFile(daemonLogPath(), `[${new Date().toISOString()}] reclaimed stale worktree for ${taskId} in ${project.alias}\n`, "utf8")
        .catch(() => {});
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      await fs
        .appendFile(
          daemonLogPath(),
          `[${new Date().toISOString()}] failed to reclaim worktree for ${taskId} in ${project.alias}: ${detail}\n`,
          "utf8",
        )
        .catch(() => {});
    }
  }
}

// W3: Rotate daemon.log if it exceeds 10 MB. Keep last 3 rotated files.
const LOG_ROTATE_MAX_BYTES = 10 * 1024 * 1024; // 10 MB
const LOG_ROTATE_MAX_BACKUPS = 3;

async function rotateLogIfNeeded(logPath: string): Promise<void> {
  await rotateLogFile(logPath, LOG_ROTATE_MAX_BYTES, LOG_ROTATE_MAX_BACKUPS).catch(() => {});
}
