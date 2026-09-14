import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { saveGlobalConfig } from "../../src/core/global-config.js";
import { addProject, markProjectInitialized } from "../../src/core/project-registry.js";
import { loadDaemonState, saveDaemonState } from "../../src/core/daemon-state.js";
import { selectNextProject } from "../../src/core/project-selection.js";

const tempDirs: string[] = [];

afterEach(async () => {
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

test("selects the active project first when it has eligible queued work", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectA = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-a-"));
  const projectB = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-b-"));
  tempDirs.push(appHome, projectA, projectB);

  await addProject("a", projectA, appHome);
  await addProject("b", projectB, appHome);
  await markProjectInitialized("a", appHome);
  await markProjectInitialized("b", appHome);
  await saveGlobalConfig(
    {
      version: 1,
      model: null,
      activeProjectAlias: "b",
      budgets: { dailyCostUsd: 25 },
      runtime: {
        runTimeoutSeconds: 1800,
        maxAttemptsPerTask: 3,
        noProgressRepeatLimit: 2,
      },
    },
    appHome,
  );

  await fs.mkdir(path.join(projectA, ".openloop"), { recursive: true });
  await fs.mkdir(path.join(projectB, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(projectA, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [{ id: "a1", title: "A", kind: "feature", status: "ready", risk: "low-risk", source: { type: "human", ref: "x" }, specId: null, branch: null, owner: null, acceptanceCriteria: ["x"], attempts: 0, lastFailureSignature: null, promotion: "auto-merge", createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }] }, null, 2)}\n`,
  );
  await fs.writeFile(
    path.join(projectB, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [{ id: "b1", title: "B", kind: "feature", status: "ready", risk: "low-risk", source: { type: "human", ref: "x" }, specId: null, branch: null, owner: null, acceptanceCriteria: ["x"], attempts: 0, lastFailureSignature: null, promotion: "auto-merge", createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }] }, null, 2)}\n`,
  );

  const selected = await selectNextProject(appHome);
  expect(selected?.alias).toBe("b");
});

test("round-robin picks the eligible project with the oldest lastIterationAt", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectA = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-a-"));
  const projectB = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-b-"));
  tempDirs.push(appHome, projectA, projectB);

  await seedEligibleProject(appHome, projectA, "a");
  await seedEligibleProject(appHome, projectB, "b");
  await saveDaemonState(
    {
      version: 1,
      startedAt: "2026-03-09T09:00:00.000Z",
      pid: 1,
      activeProject: null,
      paused: false,
      pausedAt: null,
      totalBudgetSpentUsd: 0,
      budgetDate: "2026-03-09",
      budgetSpentUsd: 0,
      budgetBlocked: false,
      currentRun: null,
      projects: [
        { alias: "a", queueSize: 1, paused: false, lastIterationAt: "2026-03-09T10:00:00.000Z", lastResult: "idle", blockedTasks: 0 },
        { alias: "b", queueSize: 1, paused: false, lastIterationAt: "2026-03-09T09:00:00.000Z", lastResult: "idle", blockedTasks: 0 },
      ],
    },
    appHome,
  );

  // b waited longer — round-robin must rotate to it instead of alphabetical a.
  const selected = await selectNextProject(appHome);
  expect(selected?.alias).toBe("b");
});

test("round-robin breaks ties alphabetically and runs never-started projects first", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectA = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-a-"));
  const projectB = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-b-"));
  const projectC = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-c-"));
  tempDirs.push(appHome, projectA, projectB, projectC);

  await seedEligibleProject(appHome, projectA, "a");
  await seedEligibleProject(appHome, projectB, "b");
  await seedEligibleProject(appHome, projectC, "c");
  const sameTimestamp = "2026-03-09T09:00:00.000Z";
  await saveDaemonState(
    {
      version: 1,
      startedAt: sameTimestamp,
      pid: 1,
      activeProject: null,
      paused: false,
      pausedAt: null,
      totalBudgetSpentUsd: 0,
      budgetDate: "2026-03-09",
      budgetSpentUsd: 0,
      budgetBlocked: false,
      currentRun: null,
      projects: [
        { alias: "a", queueSize: 1, paused: false, lastIterationAt: sameTimestamp, lastResult: "idle", blockedTasks: 0 },
        { alias: "b", queueSize: 1, paused: false, lastIterationAt: sameTimestamp, lastResult: "idle", blockedTasks: 0 },
      ],
    },
    appHome,
  );

  // c has no daemon-state entry (never ran) — it goes first.
  expect((await selectNextProject(appHome))?.alias).toBe("c");

  const state = await loadDaemonState(appHome);
  // Register c with a newer iteration: a and b now tie at the oldest
  // timestamp — alphabetical picks a.
  state.projects.push({
    alias: "c",
    queueSize: 1,
    paused: false,
    lastIterationAt: "2026-03-09T11:00:00.000Z",
    lastResult: "idle",
    blockedTasks: 0,
  });
  await saveDaemonState(state, appHome);
  expect((await selectNextProject(appHome))?.alias).toBe("a");
});

async function seedEligibleProject(appHome: string, projectRoot: string, alias: string): Promise<void> {
  await addProject(alias, projectRoot, appHome);
  await markProjectInitialized(alias, appHome);
  await saveGlobalConfig(
    {
      version: 1,
      model: null,
      activeProjectAlias: null,
      budgets: { dailyCostUsd: 25 },
      runtime: {
        runTimeoutSeconds: 1800,
        maxAttemptsPerTask: 3,
        noProgressRepeatLimit: 2,
      },
    },
    appHome,
  );
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  const now = new Date().toISOString();
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: now, tasks: [{ id: `${alias}1`, title: alias, kind: "feature", status: "ready", risk: "low-risk", source: { type: "human", ref: "x" }, specId: null, branch: null, owner: null, acceptanceCriteria: ["x"], attempts: 0, lastFailureSignature: null, promotion: "auto-merge", createdAt: now, updatedAt: now }] }, null, 2)}\n`,
  );
}
