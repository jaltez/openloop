import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import yargs from "yargs/yargs";
import { registerDaemonCommands } from "../../src/cli/commands/service.js";
import { registerProjectCommands } from "../../src/cli/commands/project.js";
import { registerRuntimeCommands } from "../../src/cli/commands/runtime.js";
import { loadDaemonState } from "../../src/core/daemon-state.js";
import { loadTaskLedger } from "../../src/core/task-ledger.js";

const tempDirs: string[] = [];
const originalOpenloopHome = process.env.OPENLOOP_HOME;

beforeEach(() => {
  vi.spyOn(console, "log").mockImplementation(() => undefined);
});

afterEach(async () => {
  vi.restoreAllMocks();
  process.env.OPENLOOP_HOME = originalOpenloopHome;
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

test("enqueue creates a proposed feature task from a ref", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["project", "add", "demo", projectRoot]);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`, "utf8");

  await runCli(["enqueue", "--project", "demo", "--ref", "ISSUE-123"]);

  const ledger = await loadTaskLedger(projectRoot);
  expect(ledger.tasks).toHaveLength(1);
  expect(ledger.tasks[0]?.id).toBe("issue-123");
  expect(ledger.tasks[0]?.kind).toBe("feature");
  expect(ledger.tasks[0]?.status).toBe("proposed");
  expect(ledger.tasks[0]?.source.type).toBe("issue");
  expect(ledger.tasks[0]?.source.ref).toBe("ISSUE-123");
  expect(ledger.tasks[0]?.owner).toBeNull();
});

test("pause and resume persist global daemon state via service commands", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["service", "pause"]);
  let state = await loadDaemonState(appHome);
  expect(state.paused).toBe(true);
  expect(state.pausedAt).toBeTruthy();

  await runCli(["service", "resume"]);
  state = await loadDaemonState(appHome);
  expect(state.paused).toBe(false);
  expect(state.pausedAt).toBeNull();
});

test("service pause and resume act as aliases", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["service", "pause"]);
  let state = await loadDaemonState(appHome);
  expect(state.paused).toBe(true);
  expect(state.pausedAt).toBeTruthy();

  await runCli(["service", "resume"]);
  state = await loadDaemonState(appHome);
  expect(state.paused).toBe(false);
  expect(state.pausedAt).toBeNull();
});

test("pause preserves pauseRequestedAt on current run and resume clears it", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  await fs.mkdir(path.join(appHome, "run"), { recursive: true });
  await fs.writeFile(
    path.join(appHome, "run", "daemon-state.json"),
    `${JSON.stringify({
      version: 1,
      startedAt: new Date().toISOString(),
      pid: 123,
      activeProject: "demo",
      paused: false,
      pausedAt: null,
      totalBudgetSpentUsd: 0,
      budgetDate: "2026-03-09",
      budgetSpentUsd: 0,
      budgetBlocked: false,
      currentRun: {
        projectAlias: "demo",
        taskId: "task-1",
        mode: "implement",
        role: "implementer",
        startedAt: new Date().toISOString(),
        deadlineAt: new Date().toISOString(),
        attemptNumber: 1,
        pauseRequestedAt: null,
      },
      projects: [],
    }, null, 2)}\n`,
    "utf8",
  );

  await runCli(["service", "pause"]);
  let state = await loadDaemonState(appHome);
  expect(state.currentRun?.pauseRequestedAt).toBeTruthy();

  await runCli(["service", "resume"]);
  state = await loadDaemonState(appHome);
  expect(state.currentRun?.pauseRequestedAt).toBeNull();
});

test("service stop clears stale pid files instead of failing forever", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  await fs.mkdir(path.join(appHome, "run"), { recursive: true });
  await fs.writeFile(path.join(appHome, "run", "daemon.pid"), "999999\n", "utf8");

  await runCli(["service", "stop"]);

  await expect(fs.access(path.join(appHome, "run", "daemon.pid"))).rejects.toThrow();
});

test("service stop refuses to signal a live non-openloop process", async () => {
  if (process.platform !== "linux") {
    return;
  }

  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  await fs.mkdir(path.join(appHome, "run"), { recursive: true });
  await fs.writeFile(path.join(appHome, "run", "daemon.pid"), `${process.pid}\n`, "utf8");

  await expect(runCli(["service", "stop"])).rejects.toThrow("Refusing to stop");
});

async function runCli(args: string[]): Promise<void> {
  const cli = yargs().scriptName("openloop").strict().exitProcess(false).fail((message, error) => {
    throw error ?? new Error(message);
  });

  registerProjectCommands(cli);
  registerDaemonCommands(cli);
  registerRuntimeCommands(cli);

  await cli.parseAsync(args);
}