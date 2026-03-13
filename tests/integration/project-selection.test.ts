import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { saveGlobalConfig } from "../../src/core/global-config.js";
import { addProject, markProjectInitialized } from "../../src/core/project-registry.js";
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
  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: "b",
    budgets: { dailyCostUsd: 25 },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
    },
  }, appHome);

  await fs.mkdir(path.join(projectA, ".openloop"), { recursive: true });
  await fs.mkdir(path.join(projectB, ".openloop"), { recursive: true });
  await fs.writeFile(path.join(projectA, ".openloop", "tasks.json"), `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [{ id: "a1", title: "A", kind: "feature", status: "ready", risk: "low-risk", source: { type: "human", ref: "x" }, specId: null, branch: null, owner: null, acceptanceCriteria: ["x"], attempts: 0, lastFailureSignature: null, promotion: "auto-merge", createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }] }, null, 2)}\n`);
  await fs.writeFile(path.join(projectB, ".openloop", "tasks.json"), `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [{ id: "b1", title: "B", kind: "feature", status: "ready", risk: "low-risk", source: { type: "human", ref: "x" }, specId: null, branch: null, owner: null, acceptanceCriteria: ["x"], attempts: 0, lastFailureSignature: null, promotion: "auto-merge", createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() }] }, null, 2)}\n`);

  const selected = await selectNextProject(appHome);
  expect(selected?.alias).toBe("b");
});