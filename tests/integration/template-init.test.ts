import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { initializeProjectFromTemplates } from "../../src/core/templates.js";
import type { LinkedProject } from "../../src/core/types.js";

test("materializes project templates into the target repository", async () => {
  const repoRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-app-"));
  const targetRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-target-"));
  const templatesRoot = path.join(repoRoot, "templates", "project", ".openloop");
  await fs.mkdir(templatesRoot, { recursive: true });
  await fs.writeFile(path.join(templatesRoot, "project.json"), "{}\n", "utf8");
  await fs.mkdir(path.join(repoRoot, "templates", "project", ".agents", "skills", "openloop"), { recursive: true });
  await fs.writeFile(path.join(repoRoot, "templates", "project", ".agents", "skills", "openloop", "SKILL.md"), "# Target\n", "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: targetRoot,
    defaultBranch: null,
    initialized: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  await initializeProjectFromTemplates(repoRoot, project);

  const projectConfig = JSON.parse(await fs.readFile(path.join(targetRoot, ".openloop", "project.json"), "utf8"));
  expect(projectConfig.project.alias).toBe("demo");
  expect(await fs.readFile(path.join(targetRoot, ".agents", "skills", "openloop", "SKILL.md"), "utf8")).toContain("# Target");
});