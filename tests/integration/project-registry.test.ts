import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { addProject, listProjects } from "../../src/core/project-registry.js";

const tempHomes: string[] = [];

afterEach(async () => {
  for (const tempHome of tempHomes.splice(0)) {
    await fs.rm(tempHome, { recursive: true, force: true });
  }
});

test("registers linked projects by alias", async () => {
  const tempHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempHomes.push(tempHome);

  await addProject("demo", "/tmp/demo", tempHome);
  const projects = await listProjects(tempHome);

  expect(projects).toHaveLength(1);
  expect(projects[0]?.alias).toBe("demo");
  expect(projects[0]?.path).toBe("/tmp/demo");
});