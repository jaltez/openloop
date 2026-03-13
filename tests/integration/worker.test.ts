import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test, vi } from "vitest";
import { pauseDaemon, saveDaemonState } from "../../src/core/daemon-state.js";
import { saveGlobalConfig } from "../../src/core/global-config.js";
import { addProject, markProjectInitialized } from "../../src/core/project-registry.js";
import { runWorkerTick } from "../../src/daemon/worker.js";
import type { DaemonState } from "../../src/core/types.js";

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