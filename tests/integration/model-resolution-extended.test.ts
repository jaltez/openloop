import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { saveGlobalConfig } from "../../src/core/global-config.js";
import { resolveModel } from "../../src/core/model-resolution.js";

const tempDirs: string[] = [];

afterEach(async () => {
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

function writeProjectConfig(projectRoot: string, model: string | null): Promise<void> {
  return fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    JSON.stringify({
      version: 1,
      project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
      pi: { model, promptFiles: [] },
      runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
      validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
      risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    }, null, 2),
    "utf8",
  );
}

test("returns null when no model is set anywhere", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);

  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await writeProjectConfig(projectRoot, null);
  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
  }, appHome);

  const result = await resolveModel(projectRoot, undefined, appHome);
  expect(result).toBeNull();
});

test("returns project model when only project model is set", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);

  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await writeProjectConfig(projectRoot, "anthropic/sonnet");
  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
  }, appHome);

  const result = await resolveModel(projectRoot, undefined, appHome);
  expect(result).toBe("anthropic/sonnet");
});

test("returns global model when only global model is set", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);

  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await writeProjectConfig(projectRoot, null);
  await saveGlobalConfig({
    version: 1,
    model: "openai/gpt-4o",
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
  }, appHome);

  const result = await resolveModel(projectRoot, undefined, appHome);
  expect(result).toBe("openai/gpt-4o");
});

test("CLI override takes precedence even when empty string is passed", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);

  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await writeProjectConfig(projectRoot, "anthropic/sonnet");
  await saveGlobalConfig({
    version: 1,
    model: "openai/gpt-4o",
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
  }, appHome);

  // Empty string should fall through to project model
  const result = await resolveModel(projectRoot, "", appHome);
  expect(result).toBe("anthropic/sonnet");
});
