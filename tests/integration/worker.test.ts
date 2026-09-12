import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { afterEach, expect, test, vi } from "vitest";
import { pauseDaemon, saveDaemonState, withDaemonState } from "../../src/core/daemon-state.js";
import { saveGlobalConfig } from "../../src/core/global-config.js";
import { addProject, markProjectInitialized } from "../../src/core/project-registry.js";
import { runWorkerTick, recoverStuckTasks } from "../../src/daemon/worker.js";
import { addWorktree } from "../../src/core/git.js";
import { initGitRepo } from "../helpers/factories.js";
import type { ProjectTask, SchedulerResult, TaskLedger } from "../../src/core/types.js";
import { RunTimeoutError } from "../../src/core/timeout.js";


const tempDirs: string[] = [];
const originalOpenloopHome = process.env.OPENLOOP_HOME;

afterEach(async () => {
  process.env.OPENLOOP_HOME = originalOpenloopHome;
  vi.restoreAllMocks();
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

test("worker tick does not run projects while paused", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");
  await pauseDaemon(appHome, "2026-03-09T10:00:00.000Z");
  const runProjectIterationFn = vi.fn();

  const state = await runWorkerTick({ startedAt: "2026-03-09T10:00:00.000Z", runProjectIterationFn: runProjectIterationFn as never });

  expect(runProjectIterationFn).not.toHaveBeenCalled();
  expect(state.paused).toBe(true);
  expect(state.activeProject).toBeNull();
  expect(state.pausedAt).toBe("2026-03-09T10:00:00.000Z");
  expect(state.currentRun).toBeNull();
});

test("worker tick blocks new runs when daily budget is exhausted", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");
  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 5 },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
    },
  }, appHome);
  await saveDaemonState({
    version: 1,
    startedAt: "2026-03-09T09:00:00.000Z",
    pid: 123,
    activeProject: null,
    paused: false,
    pausedAt: null,
    totalBudgetSpentUsd: 5,
    budgetDate: "2026-03-09",
    budgetSpentUsd: 5,
    budgetBlocked: false,
    currentRun: null,
    projects: [],
  }, appHome);

  const runProjectIterationFn = vi.fn();
  const state = await runWorkerTick({ startedAt: "2026-03-09T10:00:00.000Z", runProjectIterationFn: runProjectIterationFn as never });

  expect(runProjectIterationFn).not.toHaveBeenCalled();
  expect(state.budgetBlocked).toBe(true);
  expect(state.activeProject).toBeNull();
  expect(state.currentRun).toBeNull();
});

test("worker tick accumulates measured cost from the iteration result", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");
  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25, estimatedCostPerRunUsd: 0.1 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
  }, appHome);

  const state = await runWorkerTick({
    startedAt: "2026-03-09T10:00:00.000Z",
    runProjectIterationFn: async () => makeIterationResult({ costUsd: 0.42, costSource: "measured" }),
  });

  expect(state.budgetSpentUsd).toBe(0.42);
  expect(state.totalBudgetSpentUsd).toBe(0.42);
  // The measured cost is also accumulated on the task itself.
  const ledger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8"));
  expect(ledger.tasks[0].estimatedCostUsd).toBe(0.42);
});

test("worker tick charges the estimate when a run times out", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");
  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25, estimatedCostPerRunUsd: 0.1 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
  }, appHome);

  const state = await runWorkerTick({
    startedAt: "2026-03-09T10:00:00.000Z",
    runProjectIterationFn: async () => {
      throw new RunTimeoutError("Run exceeded timeout during Pi execution.");
    },
  });

  // Timeout-killed runs produce no parseable usage — they charge the estimate.
  expect(state.budgetSpentUsd).toBe(0.1);
  expect(state.totalBudgetSpentUsd).toBe(0.1);
});

function makeIterationResult(overrides: Partial<SchedulerResult>): SchedulerResult {
  return {
    projectAlias: "demo",
    taskId: "task-1",
    mode: "implement",
    role: "implementer",
    reason: "selected ready low-risk task",
    model: null,
    exitCode: 0,
    prompt: null,
    validation: [],
    promotionDecision: "none",
    promotionAction: "none",
    promotionArtifactPath: null,
    promotionResultArtifactPath: null,
    taskStatus: "done",
    promotedAt: null,
    stoppedBy: "none",
    attemptNumber: 1,
    dirtyTreeDetected: false,
    budgetSnapshotUsd: null,
    costUsd: null,
    costSource: null,
    ...overrides,
  };
}

test("worker tick skips a project under review backpressure", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");
  await seedPendingPromotions(projectRoot, 3);

  const runProjectIterationFn = vi.fn();
  const state = await runWorkerTick({
    startedAt: "2026-03-09T10:00:00.000Z",
    runProjectIterationFn: runProjectIterationFn as never,
  });

  expect(runProjectIterationFn).not.toHaveBeenCalled();
  expect(state.projects.find((project) => project.alias === "demo")?.lastResult).toBe("review-backpressure");
});

test("worker tick disables review backpressure when the limit is zero", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");
  await seedPendingPromotions(projectRoot, 5);
  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25, estimatedCostPerRunUsd: 0.1 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2, maxPendingReviewsPerProject: 0 },
  }, appHome);

  const runProjectIterationFn = vi.fn(async () => makeIterationResult({}));
  const state = await runWorkerTick({
    startedAt: "2026-03-09T10:00:00.000Z",
    runProjectIterationFn: runProjectIterationFn as never,
  });

  expect(runProjectIterationFn).toHaveBeenCalledTimes(1);
  expect(state.projects.find((project) => project.alias === "demo")?.lastResult).not.toBe("review-backpressure");
});


test("a pause arriving mid-run is not reverted by the tick's post-run save", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");

  const state = await runWorkerTick({
    startedAt: "2026-03-09T10:00:00.000Z",
    runProjectIterationFn: async () => {
      // A concurrent `openloop service pause` (or MCP openloop_pause) lands
      // while the agent run is in flight — after the tick loaded its state.
      await pauseDaemon(appHome, "2026-03-09T10:00:05.000Z");
      return makeIterationResult({});
    },
  });

  // The post-run persist must merge into fresh state, not clobber the pause.
  expect(state.paused).toBe(true);
  expect(state.pausedAt).toBe("2026-03-09T10:00:05.000Z");
  // The run still completed and charged its estimate (pause stops NEW runs).
  expect(state.currentRun).toBeNull();
  expect(state.budgetSpentUsd).toBe(0.1);
});

test("a budget write arriving mid-run is not lost by the tick's charge", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");

  const state = await runWorkerTick({
    startedAt: "2026-03-09T10:00:00.000Z",
    runProjectIterationFn: async () => {
      // An external writer (e.g. another openloop process) charges mid-run,
      // under the current budget window like any real charger would.
      await withDaemonState((fresh) => {
        fresh.budgetDate = "2026-03-09";
        fresh.budgetSpentUsd = 0.5;
        fresh.totalBudgetSpentUsd = 0.5;
      }, appHome);
      return makeIterationResult({});
    },
  });

  // The tick's 0.1 charge is a delta on fresh state: 0.5 + 0.1, not 0.1.
  expect(state.budgetSpentUsd).toBe(0.6);
  expect(state.totalBudgetSpentUsd).toBe(0.6);
});

async function seedPendingPromotions(projectRoot: string, count: number): Promise<void> {
  const promotionsDir = path.join(projectRoot, ".openloop", "promotions");
  await fs.mkdir(promotionsDir, { recursive: true });
  for (let index = 0; index < count; index++) {
    await fs.writeFile(
      path.join(promotionsDir, `2026-03-09T10-00-0${index}-000Z-task-${index}.json`),
      `${JSON.stringify({
        version: 1,
        createdAt: `2026-03-09T10:00:0${index}.000Z`,
        projectAlias: "demo",
        taskId: `task-${index}`,
        baseBranch: null,
        decision: "manual-review",
        action: "queue-review",
        effectivePromotionMode: "pull-request",
        validation: [],
        piExitCode: 0,
        outcome: "completed",
        status: "pending",
        processedAt: null,
        note: null,
      }, null, 2)}\n`,
      "utf8",
    );
  }
}

test("worker tick enqueues tasks from due schedules without backfilling fresh installs", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");
  await writeScheduleConfig(projectRoot, { s1: { cron: "* * * * *", title: "T" } }, null);

  // First tick: schedule is armed but does not backfill.
  await runWorkerTick({
    startedAt: "2026-03-09T10:00:30.000Z",
    now: new Date("2026-03-09T10:00:30.000Z"),
    runProjectIterationFn: async () => makeIterationResult({}),
  });
  let tasks = await readTasks(projectRoot);

  // Second tick one minute later: the every-minute cron fired in between.
  await runWorkerTick({
    startedAt: "2026-03-09T10:01:30.000Z",
    now: new Date("2026-03-09T10:01:30.000Z"),
    runProjectIterationFn: async () => makeIterationResult({}),
  });
  tasks = await readTasks(projectRoot);
  const scheduleTasks = tasks.filter((task) => task.source.type === "schedule");
  expect(scheduleTasks).toHaveLength(1);
  expect(scheduleTasks[0]).toMatchObject({
    title: "T",
    source: { type: "schedule", ref: "s1" },
    risk: "medium-risk",
    status: "proposed",
  });

  // Third tick within the same fired minute: no duplicate.
  await runWorkerTick({
    startedAt: "2026-03-09T10:01:45.000Z",
    now: new Date("2026-03-09T10:01:45.000Z"),
    runProjectIterationFn: async () => makeIterationResult({}),
  });
  tasks = await readTasks(projectRoot);
  expect(tasks.filter((task) => task.source.type === "schedule")).toHaveLength(1);

  // scheduleState persisted to project config at the fired minute.
  const config = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "project.json"), "utf8"));
  expect(config.scheduleState.s1).toBe("2026-03-09T10:01:00.000Z");
});

async function writeScheduleConfig(
  projectRoot: string,
  schedules: Record<string, { cron: string; title: string; prompt?: string }>,
  scheduleState: Record<string, string> | null,
): Promise<void> {
  const configPath = path.join(projectRoot, ".openloop", "project.json");
  const existing = await fs.readFile(configPath, "utf8").then((raw) => JSON.parse(raw), () => ({}));
  await fs.writeFile(
    configPath,
    `${JSON.stringify({
      ...existing,
      schedule: Object.entries(schedules).map(([id, schedule]) => ({ id, ...schedule })),
      scheduleState: scheduleState ?? undefined,
    }, null, 2)}\n`,
    "utf8",
  );
}


async function readTasks(projectRoot: string): Promise<ProjectTask[]> {
  // Test-owned fixture file: loadTaskLedger would apply defaults; read raw JSON.
  const raw = await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8");
  const parsed = JSON.parse(raw) as TaskLedger;
  return parsed.tasks;
}

test("recoverStuckTasks reclaims stale worktrees and drops abandoned branches", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;
  await initGitRepo(projectRoot);

  await addProject("demo", projectRoot, appHome);
  await markProjectInitialized("demo", appHome);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  const now = new Date().toISOString();
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({
      version: 1,
      updatedAt: now,
      tasks: [
        { id: "blocked-task", status: "blocked" },
        { id: "done-task", status: "done" },
      ].map((partial) => ({
        title: partial.id,
        kind: "feature",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: [],
        attempts: 1,
        lastFailureSignature: null,
        promotion: "pull-request",
        createdAt: now,
        updatedAt: now,
        ...partial,
      })),
    }, null, 2)}\n`,
    "utf8",
  );

  const worktreesDir = path.join(projectRoot, ".openloop", "worktrees");
  await fs.mkdir(worktreesDir, { recursive: true });
  await addWorktree(projectRoot, path.join(worktreesDir, "blocked-task"), "openloop/blocked-task");
  await addWorktree(projectRoot, path.join(worktreesDir, "done-task"), "openloop/done-task");

  await recoverStuckTasks();

  // Both stale worktree checkouts are gone...
  await expect(fs.stat(path.join(worktreesDir, "blocked-task"))).rejects.toThrow();
  await expect(fs.stat(path.join(worktreesDir, "done-task"))).rejects.toThrow();

  // ...the blocked task's branch is dropped, the done task's branch is kept
  // (it may hold unpromoted work).
  expectBranch(projectRoot, "openloop/blocked-task", false);
  expectBranch(projectRoot, "openloop/done-task", true);
});

function expectBranch(projectRoot: string, branch: string, exists: boolean): void {
  let present = true;
  try {
    execFileSync("git", ["rev-parse", "--verify", `refs/heads/${branch}`], { cwd: projectRoot, stdio: "ignore" });
  } catch {
    present = false;
  }
  expect(present).toBe(exists);
}

test("worker tick persists lastDigestDate so daily-digest fires once per day", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await seedRunnableProject(appHome, projectRoot, "demo", "task-1");

  const state = await runWorkerTick({
    startedAt: "2026-03-09T10:00:00.000Z",
    now: new Date("2026-03-09T10:00:00.000Z"),
    runProjectIterationFn: async () => makeIterationResult({}),
  });

  // The fired digest date survives the state save — before the fix every
  // save dropped it and the hook re-armed on each tick.
  expect(state.lastDigestDate).toBe("2026-03-09");

  // A tick on the same calendar day must not change it.
  const sameDay = await runWorkerTick({
    startedAt: "2026-03-09T12:00:00.000Z",
    now: new Date("2026-03-09T12:00:00.000Z"),
    runProjectIterationFn: async () => makeIterationResult({}),
  });
  expect(sameDay.lastDigestDate).toBe("2026-03-09");

  // The next calendar day advances it.
  const nextDay = await runWorkerTick({
    startedAt: "2026-03-10T00:30:00.000Z",
    now: new Date("2026-03-10T00:30:00.000Z"),
    runProjectIterationFn: async () => makeIterationResult({}),
  });
  expect(nextDay.lastDigestDate).toBe("2026-03-10");
});

async function seedRunnableProject(appHome: string, projectRoot: string, alias: string, taskId: string): Promise<void> {
  await addProject(alias, projectRoot, appHome);
  await markProjectInitialized(alias, appHome);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({
      version: 1,
      updatedAt: new Date().toISOString(),
      tasks: [
        {
          id: taskId,
          title: "Queued task",
          kind: "feature",
          status: "ready",
          risk: "low-risk",
          source: { type: "human", ref: "test" },
          specId: null,
          branch: null,
          owner: null,
          acceptanceCriteria: ["x"],
          attempts: 0,
          lastFailureSignature: null,
          promotion: "auto-merge",
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
        },
      ],
    }, null, 2)}\n`,
    "utf8",
  );
}