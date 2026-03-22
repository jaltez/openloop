import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { generateRunReport, formatRunReport } from "../../src/core/run-report.js";
import { saveGlobalConfig } from "../../src/core/global-config.js";
import { writeJsonFile, ensureDir } from "../../src/core/fs.js";
import { projectsRegistryPath, runtimeDir, daemonStatePath } from "../../src/core/paths.js";
import { createDefaultDaemonState } from "../../src/core/daemon-state.js";
import { makeProjectTask } from "../helpers/factories.js";

const tempDirs: string[] = [];

afterEach(async () => {
  for (const dir of tempDirs.splice(0)) {
    await fs.rm(dir, { recursive: true, force: true });
  }
});

async function setupTestEnv() {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-report-"));
  tempDirs.push(appHome);

  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
    },
  }, appHome);

  await ensureDir(runtimeDir(appHome));
  await writeJsonFile(daemonStatePath(appHome), createDefaultDaemonState({ budgetSpentUsd: 4.23 }));

  return appHome;
}

test("generates empty report when no projects exist", async () => {
  const appHome = await setupTestEnv();
  await writeJsonFile(projectsRegistryPath(appHome), { version: 1, projects: [] });

  const report = await generateRunReport({ appHomeOverride: appHome });
  expect(report.totalCompleted).toBe(0);
  expect(report.totalFailed).toBe(0);
  expect(report.projects).toHaveLength(0);
});

test("generates report with project tasks", async () => {
  const appHome = await setupTestEnv();
  const projectDir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-proj-"));
  tempDirs.push(projectDir);

  await writeJsonFile(projectsRegistryPath(appHome), {
    version: 1,
    projects: [{ alias: "myapp", path: projectDir, defaultBranch: null, initialized: true, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }],
  });

  const openloopDir = path.join(projectDir, ".openloop");
  await ensureDir(openloopDir);

  const now = new Date().toISOString();
  const doneTask = makeProjectTask({
    id: "add-validation",
    title: "Add validation",
    status: "done",
    risk: "low-risk",
    updatedAt: now,
    lastRun: {
      completedAt: now,
      mode: "implement",
      piExitCode: 0,
      outcome: "completed",
      baseBranch: "main",
      validation: [{ name: "lint", command: "bun run lint", exitCode: 0 }, { name: "test", command: "bun run test", exitCode: 0 }],
      promotionDecision: "auto-merge-eligible",
      effectivePromotionMode: "auto-merge",
      promotionAction: "queue-auto-merge",
      promotionArtifactPath: null,
      promotionArtifactState: "applied",
      promotionResultArtifactPath: null,
    },
  });

  const failedTask = makeProjectTask({
    id: "fix-auth",
    title: "Fix auth flow",
    status: "failed",
    risk: "medium-risk",
    updatedAt: now,
    branch: "openloop/fix-auth",
    lastRun: {
      completedAt: now,
      mode: "implement",
      piExitCode: 1,
      outcome: "validation-failed",
      baseBranch: "main",
      validation: [{ name: "lint", command: "bun run lint", exitCode: 0 }, { name: "test", command: "bun run test", exitCode: 1 }],
      promotionDecision: "blocked",
      effectivePromotionMode: "pull-request",
      promotionAction: "block",
      promotionArtifactPath: null,
      promotionArtifactState: "pending",
      promotionResultArtifactPath: null,
    },
  });

  await writeJsonFile(path.join(openloopDir, "tasks.json"), {
    version: 1,
    updatedAt: now,
    tasks: [doneTask, failedTask],
  });

  const report = await generateRunReport({ appHomeOverride: appHome });
  expect(report.totalCompleted).toBe(1);
  expect(report.totalFailed).toBe(1);
  expect(report.projects).toHaveLength(1);
  expect(report.projects[0]!.alias).toBe("myapp");
  expect(report.projects[0]!.completed).toHaveLength(1);
  expect(report.projects[0]!.failed).toHaveLength(1);
  expect(report.budgetSpentUsd).toBe(4.23);
});

test("formatRunReport produces human-readable output", async () => {
  const appHome = await setupTestEnv();
  await writeJsonFile(projectsRegistryPath(appHome), { version: 1, projects: [] });

  const report = await generateRunReport({ appHomeOverride: appHome });
  const output = formatRunReport(report);
  expect(output).toContain("=== OpenLoop Summary");
  expect(output).toContain("$4.23 spent");
});

test("report respects sinceMs filter", async () => {
  const appHome = await setupTestEnv();
  const projectDir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-proj-"));
  tempDirs.push(projectDir);

  await writeJsonFile(projectsRegistryPath(appHome), {
    version: 1,
    projects: [{ alias: "myapp", path: projectDir, defaultBranch: null, initialized: true, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }],
  });

  const openloopDir = path.join(projectDir, ".openloop");
  await ensureDir(openloopDir);

  // Task updated 2 days ago — should not appear in 24h report
  const oldDate = new Date(Date.now() - 2 * 24 * 60 * 60 * 1000).toISOString();
  const oldTask = makeProjectTask({ id: "old-task", title: "Old task", status: "done", updatedAt: oldDate });

  await writeJsonFile(path.join(openloopDir, "tasks.json"), {
    version: 1,
    updatedAt: oldDate,
    tasks: [oldTask],
  });

  const report = await generateRunReport({ appHomeOverride: appHome, sinceMs: 24 * 60 * 60 * 1000 });
  expect(report.projects[0]!.completed).toHaveLength(0);
  expect(report.projects[0]!.idle).toBe(true);
});
