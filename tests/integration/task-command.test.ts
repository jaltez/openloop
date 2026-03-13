import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import yargs from "yargs/yargs";
import { registerProjectCommands } from "../../src/cli/commands/project.js";
import { registerTaskCommands } from "../../src/cli/commands/task.js";
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

test("task add persists declared scope paths", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["project", "add", "demo", projectRoot]);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`, "utf8");

  await runCli(["task", "add", "--project", "demo", "--title", "Scoped task", "--scope", "src/core/scheduler.ts", "--scope", "README.md"]);

  const ledger = await loadTaskLedger(projectRoot);
  expect(ledger.tasks).toHaveLength(1);
  expect(ledger.tasks[0]?.scope?.paths).toEqual(["src/core/scheduler.ts", "README.md"]);
});

test("project init resolves bundled templates independently of current working directory", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  const unrelatedCwd = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-cwd-"));
  tempDirs.push(appHome, projectRoot, unrelatedCwd);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["project", "add", "demo", projectRoot]);

  const previousCwd = process.cwd();
  process.chdir(unrelatedCwd);
  try {
    await runCli(["project", "init", "demo"]);
  } finally {
    process.chdir(previousCwd);
  }

  const projectConfig = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "project.json"), "utf8"));
  expect(projectConfig.project.alias).toBe("demo");
  expect(await fs.readFile(path.join(projectRoot, ".agents", "skills", "openloop", "SKILL.md"), "utf8")).toContain("Openloop Repository Skill");
});

async function runCli(args: string[]): Promise<void> {
  const cli = yargs().scriptName("openloop").strict().exitProcess(false).fail((message, error) => {
    throw error ?? new Error(message);
  });

  registerProjectCommands(cli);
  registerTaskCommands(cli);

  await cli.parseAsync(args);
}