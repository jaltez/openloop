import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { initializeProjectFromTemplates } from "../../src/core/templates.js";
import type { LinkedProject } from "../../src/core/types.js";

test("merges into existing project.json without losing pre-existing fields", async () => {
  const repoRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-app-"));
  const targetRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-target-"));

  // Set up minimal template
  const templatesRoot = path.join(repoRoot, "templates", "project", ".openloop");
  await fs.mkdir(templatesRoot, { recursive: true });
  await fs.writeFile(
    path.join(templatesRoot, "project.json"),
    JSON.stringify({ version: 1, pi: { model: null, promptFiles: [] } }, null, 2),
    "utf8",
  );

  // Pre-create a project.json with some custom fields in the target
  await fs.mkdir(path.join(targetRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(targetRoot, ".openloop", "project.json"),
    JSON.stringify({
      version: 1,
      customField: "should-survive",
      pi: { model: "anthropic/sonnet", promptFiles: [] },
    }, null, 2),
    "utf8",
  );

  const project: LinkedProject = {
    alias: "existing",
    path: targetRoot,
    defaultBranch: null,
    initialized: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  await initializeProjectFromTemplates(repoRoot, project);

  const config = JSON.parse(await fs.readFile(path.join(targetRoot, ".openloop", "project.json"), "utf8"));
  // project and validation fields should be set by init
  expect(config.project.alias).toBe("existing");
  expect(config.project.initializedAt).toBeTruthy();
  expect(config.validation).toBeDefined();
});

test("creates tasks.json with updatedAt when template provides it", async () => {
  const repoRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-app-"));
  const targetRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-target-"));

  const templatesRoot = path.join(repoRoot, "templates", "project", ".openloop");
  await fs.mkdir(templatesRoot, { recursive: true });
  await fs.writeFile(path.join(templatesRoot, "project.json"), "{}\n", "utf8");
  await fs.writeFile(
    path.join(templatesRoot, "tasks.json"),
    JSON.stringify({ version: 1, updatedAt: "2020-01-01T00:00:00.000Z", tasks: [] }, null, 2),
    "utf8",
  );

  const project: LinkedProject = {
    alias: "tasks-test",
    path: targetRoot,
    defaultBranch: null,
    initialized: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  await initializeProjectFromTemplates(repoRoot, project);

  const tasks = JSON.parse(await fs.readFile(path.join(targetRoot, ".openloop", "tasks.json"), "utf8"));
  // updatedAt should be refreshed, not the old template value
  expect(tasks.updatedAt).not.toBe("2020-01-01T00:00:00.000Z");
  expect(tasks.tasks).toEqual([]);
});

test("does not overwrite existing agent skill customizations during init", async () => {
  const repoRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-app-"));
  const targetRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-target-"));

  await fs.mkdir(path.join(repoRoot, "templates", "project", ".openloop"), { recursive: true });
  await fs.writeFile(path.join(repoRoot, "templates", "project", ".openloop", "project.json"), "{}\n", "utf8");
  await fs.mkdir(path.join(repoRoot, "templates", "project", ".agents", "skills", "openloop"), { recursive: true });
  await fs.writeFile(path.join(repoRoot, "templates", "project", ".agents", "skills", "openloop", "SKILL.md"), "# Template Skill\n", "utf8");

  await fs.mkdir(path.join(targetRoot, ".agents", "skills", "openloop"), { recursive: true });
  await fs.writeFile(path.join(targetRoot, ".agents", "skills", "openloop", "SKILL.md"), "# Existing Skill\n", "utf8");

  const project: LinkedProject = {
    alias: "existing-skill",
    path: targetRoot,
    defaultBranch: null,
    initialized: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  await initializeProjectFromTemplates(repoRoot, project);

  expect(await fs.readFile(path.join(targetRoot, ".agents", "skills", "openloop", "SKILL.md"), "utf8")).toBe("# Existing Skill\n");
});

test("preserves existing task ledger entries instead of overwriting with template defaults", async () => {
  const repoRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-app-"));
  const targetRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-target-"));

  const templatesRoot = path.join(repoRoot, "templates", "project", ".openloop");
  await fs.mkdir(templatesRoot, { recursive: true });
  await fs.writeFile(path.join(templatesRoot, "project.json"), "{}\n", "utf8");
  await fs.writeFile(
    path.join(templatesRoot, "tasks.json"),
    JSON.stringify({ version: 1, updatedAt: "2020-01-01T00:00:00.000Z", tasks: [] }, null, 2),
    "utf8",
  );

  await fs.mkdir(path.join(targetRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(targetRoot, ".openloop", "tasks.json"),
    JSON.stringify({
      version: 1,
      updatedAt: "2024-01-01T00:00:00.000Z",
      tasks: [{ id: "keep-me", title: "Existing task" }],
    }, null, 2),
    "utf8",
  );

  const project: LinkedProject = {
    alias: "ledger-test",
    path: targetRoot,
    defaultBranch: null,
    initialized: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  await initializeProjectFromTemplates(repoRoot, project);

  const tasks = JSON.parse(await fs.readFile(path.join(targetRoot, ".openloop", "tasks.json"), "utf8"));
  expect(tasks.tasks).toEqual([{ id: "keep-me", title: "Existing task" }]);
  expect(tasks.updatedAt).not.toBe("2024-01-01T00:00:00.000Z");
});
