import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import yargs from "yargs/yargs";
import { registerProjectCommands } from "../../src/cli/commands/project.js";
import { registerTaskCommands } from "../../src/cli/commands/task.js";
import { runProjectIteration } from "../../src/core/scheduler.js";
import { loadTaskLedger } from "../../src/core/task-ledger.js";
import type { LinkedProject, ProjectConfig, TaskLedger } from "../../src/core/types.js";

const tempDirs: string[] = [];
const originalOpenloopHome = process.env.OPENLOOP_HOME;

beforeEach(() => {
  vi.spyOn(console, "log").mockImplementation(() => undefined);
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

afterEach(async () => {
  vi.restoreAllMocks();
  process.env.OPENLOOP_HOME = originalOpenloopHome;
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

test("plan run gates medium-risk tasks to awaiting-approval", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-gate-medium-"));
  tempDirs.push(projectRoot);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "gate-me",
      title: "Medium risk task",
      kind: "feature",
      status: "proposed",
      risk: "medium-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: [],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "pull-request",
      notes: [],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, { piRunner: async () => 0 });

  expect(result.mode).toBe("plan");
  expect(result.taskStatus).toBe("awaiting-approval");

  const persisted = await loadTaskLedger(projectRoot);
  expect(persisted.tasks[0]?.status).toBe("awaiting-approval");
  expect(persisted.tasks[0]?.notes).toContainEqual(expect.stringContaining("awaiting human approval"));
});

test("plan run gates high-risk tasks to awaiting-approval", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-gate-high-"));
  tempDirs.push(projectRoot);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "high-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "gate-me-high",
      title: "High risk task",
      kind: "feature",
      status: "proposed",
      risk: "high-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: [],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "pull-request",
      notes: [],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, { piRunner: async () => 0 });

  expect(result.mode).toBe("plan");
  expect(result.taskStatus).toBe("awaiting-approval");
});

test("plan run lets low-risk tasks go straight to ready", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-gate-low-"));
  tempDirs.push(projectRoot);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "low-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "gate-me-low",
      title: "Low risk task",
      kind: "feature",
      status: "proposed",
      risk: "low-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: [],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "pull-request",
      notes: [],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, { piRunner: async () => 0 });

  expect(result.mode).toBe("plan");
  expect(result.taskStatus).toBe("ready");

  const persisted = await loadTaskLedger(projectRoot);
  expect(persisted.tasks[0]?.status).toBe("ready");
});

test("task approve transitions awaiting-approval to ready", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["project", "add", "demo", projectRoot]);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "awaiting-task",
      title: "Awaiting approval task",
      kind: "feature",
      status: "awaiting-approval",
      risk: "medium-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: [],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "pull-request",
      notes: [],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  await runCli(["task", "approve", "--project", "demo", "--task", "awaiting-task"]);

  const persisted = await loadTaskLedger(projectRoot);
  expect(persisted.tasks[0]?.status).toBe("ready");
  expect(persisted.tasks[0]?.notes).toContainEqual("Approved via 'task approve'.");
});

test("task approve fails when task is not awaiting-approval", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["project", "add", "demo", projectRoot]);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "ready-task",
      title: "Ready task",
      kind: "feature",
      status: "ready",
      risk: "medium-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: [],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "pull-request",
      notes: [],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  await runCli(["task", "approve", "--project", "demo", "--task", "ready-task"]);

  expect(process.exitCode).toBe(1);
  process.exitCode = undefined;
});

async function runCli(args: string[]): Promise<void> {
  const cli = yargs().scriptName("openloop").strict().exitProcess(false).fail((message, error) => {
    throw error ?? new Error(message);
  });

  registerProjectCommands(cli);
  registerTaskCommands(cli);

  await cli.parseAsync(args);
}
