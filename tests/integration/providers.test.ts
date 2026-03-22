import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import yargs from "yargs/yargs";
import { registerConfigCommands } from "../../src/cli/commands/config.js";
import { registerProjectCommands } from "../../src/cli/commands/project.js";
import { getProvider, listProviders, resolveProvider, createCustomProvider, PROVIDER_NAMES } from "../../src/core/providers.js";
import { loadGlobalConfig } from "../../src/core/global-config.js";
import { loadProjectConfig } from "../../src/core/project-config.js";
import { runAgent } from "../../src/core/pi.js";
import type { LinkedProject, ProjectConfig } from "../../src/core/types.js";

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

// --- Provider registry tests ---

test("listProviders returns all built-in providers", () => {
  const providers = listProviders();
  const names = providers.map((p) => p.name);
  expect(names).toContain("pi");
  expect(names).toContain("claude");
  expect(names).toContain("aider");
  expect(names).toContain("codex");
  expect(names).toContain("opencode");
});

test("PROVIDER_NAMES matches listProviders names", () => {
  const providers = listProviders();
  expect([...PROVIDER_NAMES]).toEqual(providers.map((p) => p.name));
});

test("getProvider returns undefined for unknown provider", () => {
  expect(getProvider("nonexistent")).toBeUndefined();
});

test("getProvider returns known providers", () => {
  expect(getProvider("pi")?.name).toBe("pi");
  expect(getProvider("claude")?.label).toBe("Claude Code");
  expect(getProvider("codex")?.label).toBe("OpenAI Codex");
  expect(getProvider("opencode")?.label).toBe("OpenCode");
});

test("resolveProvider falls back to pi when no type specified", () => {
  const provider = resolveProvider(undefined, null);
  expect(provider.name).toBe("pi");
});

test("resolveProvider uses specified agent type", () => {
  const provider = resolveProvider("claude", null);
  expect(provider.name).toBe("claude");
});

test("resolveProvider uses defaultProvider when agentType is undefined", () => {
  const provider = resolveProvider(undefined, null, "aider");
  expect(provider.name).toBe("aider");
});

test("resolveProvider creates custom provider when type is custom", () => {
  const provider = resolveProvider("custom", "my-script.sh");
  expect(provider.name).toBe("custom");
  expect(provider.label).toContain("my-script.sh");
});

test("createCustomProvider marks itself as always available", () => {
  const provider = createCustomProvider("echo hello");
  expect(provider.checkAvailable()).toBe(true);
});

// --- runAgent with provider dispatch ---

test("runAgent dispatches to the configured provider binary", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-provider-test-"));
  tempDirs.push(dir);

  // Create a fake agent binary that exits 0
  const fakeBin = path.join(dir, "claude");
  await fs.writeFile(fakeBin, "#!/bin/sh\nexit 0\n", "utf8");
  await fs.chmod(fakeBin, 0o755);

  const origPath = process.env.PATH;
  process.env.PATH = `${dir}:${origPath}`;

  try {
    const project: LinkedProject = {
      alias: "test",
      path: dir,
      defaultBranch: null,
      initialized: true,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };

    const config: ProjectConfig = {
      version: 1,
      project: { alias: "test", repoRoot: dir, initializedAt: null },
      pi: { model: null, promptFiles: [] },
      agent: { type: "claude", command: null },
      runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
      validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
      risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    };

    const code = await runAgent({ prompt: "hello", project }, config);
    expect(code).toBe(0);
  } finally {
    process.env.PATH = origPath;
  }
});

test("runAgent uses defaultProvider when project has no agent config", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-provider-default-"));
  tempDirs.push(dir);

  // Create a fake "aider" binary
  const fakeBin = path.join(dir, "aider");
  await fs.writeFile(fakeBin, "#!/bin/sh\nexit 0\n", "utf8");
  await fs.chmod(fakeBin, 0o755);

  const origPath = process.env.PATH;
  process.env.PATH = `${dir}:${origPath}`;

  try {
    const project: LinkedProject = {
      alias: "test",
      path: dir,
      defaultBranch: null,
      initialized: true,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };

    const config: ProjectConfig = {
      version: 1,
      project: { alias: "test", repoRoot: dir, initializedAt: null },
      pi: { model: null, promptFiles: [] },
      runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
      validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
      risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    };

    // Pass "aider" as the default provider
    const code = await runAgent({ prompt: "hello", project }, config, "aider");
    expect(code).toBe(0);
  } finally {
    process.env.PATH = origPath;
  }
});

test("runAgent dispatches custom provider with OPENLOOP_PROMPT env", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-provider-custom-"));
  tempDirs.push(dir);

  const config: ProjectConfig = {
    version: 1,
    project: { alias: "test", repoRoot: dir, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    agent: { type: "custom", command: "echo" },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };

  const project: LinkedProject = {
    alias: "test",
    path: dir,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const code = await runAgent({ prompt: "hello world", project }, config);
  expect(code).toBe(0);
});

// --- CLI commands ---

test("config set-provider persists global default provider", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["config", "set-provider", "claude"]);

  const config = await loadGlobalConfig(appHome);
  expect(config.defaultProvider).toBe("claude");
});

test("config list-providers outputs provider info", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempDirs.push(appHome);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["config", "list-providers"]);

  const logCalls = (console.log as ReturnType<typeof vi.fn>).mock.calls.flat().join("\n");
  expect(logCalls).toContain("pi");
  expect(logCalls).toContain("claude");
  expect(logCalls).toContain("codex");
  expect(logCalls).toContain("custom");
});

test("config project-set-agent persists project agent config", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await runCli(["project", "add", "demo", projectRoot]);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    JSON.stringify({
      version: 1,
      project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
      pi: { model: null, promptFiles: [] },
      runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
      validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
      risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    }, null, 2),
    "utf8",
  );

  await runCli(["config", "project-set-agent", "demo", "claude"]);

  const config = await loadProjectConfig(projectRoot);
  expect(config.agent?.type).toBe("claude");
  expect(config.agent?.command).toBeNull();
});

async function runCli(args: string[]): Promise<void> {
  const cli = yargs().scriptName("openloop").strict().exitProcess(false).fail((message, error) => {
    throw error ?? new Error(message);
  });

  registerConfigCommands(cli);
  registerProjectCommands(cli);

  await cli.parseAsync(args);
}
