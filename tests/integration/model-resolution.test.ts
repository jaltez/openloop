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

test("resolves model with precedence CLI > project > global", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);

  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    JSON.stringify({
      version: 1,
      project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
      pi: { model: "anthropic/project-model", promptFiles: [] },
      runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
      validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
      risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    }, null, 2),
    "utf8",
  );

  await saveGlobalConfig({
    version: 1,
    model: "openai/global-model",
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
    },
  }, appHome);

  expect(await resolveModel(projectRoot, "google/cli-model", appHome)).toBe("google/cli-model");
  expect(await resolveModel(projectRoot, undefined, appHome)).toBe("anthropic/project-model");

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

  expect(await resolveModel(projectRoot, undefined, appHome)).toBe("openai/global-model");
});