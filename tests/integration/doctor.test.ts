import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { runDoctorChecks, formatDoctorResults } from "../../src/core/doctor.js";
import { saveGlobalConfig } from "../../src/core/global-config.js";
import { writeJsonFile, ensureDir } from "../../src/core/fs.js";
import { projectsRegistryPath, globalConfigPath, runtimeDir } from "../../src/core/paths.js";

const tempDirs: string[] = [];

afterEach(async () => {
  for (const dir of tempDirs.splice(0)) {
    await fs.rm(dir, { recursive: true, force: true });
  }
});

test("doctor reports ok for valid global config", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-doctor-"));
  tempDirs.push(appHome);

  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
    },
  }, appHome);

  await writeJsonFile(projectsRegistryPath(appHome), { version: 1, projects: [] });
  await ensureDir(runtimeDir(appHome));

  const results = await runDoctorChecks(appHome);
  const configCheck = results.find((r) => r.label === "Global config");
  expect(configCheck).toBeDefined();
  expect(configCheck!.status).toBe("ok");
});

test("doctor warns on zero budget ceiling", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-doctor-"));
  tempDirs.push(appHome);

  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 0 },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
    },
  }, appHome);

  await writeJsonFile(projectsRegistryPath(appHome), { version: 1, projects: [] });

  const results = await runDoctorChecks(appHome);
  const budgetCheck = results.find((r) => r.label === "Budget ceiling");
  expect(budgetCheck).toBeDefined();
  expect(budgetCheck!.status).toBe("warn");
});

test("doctor warns when no projects are linked", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-doctor-"));
  tempDirs.push(appHome);

  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
    },
  }, appHome);
  await writeJsonFile(projectsRegistryPath(appHome), { version: 1, projects: [] });

  const results = await runDoctorChecks(appHome);
  const projectsCheck = results.find((r) => r.label === "Linked projects");
  expect(projectsCheck).toBeDefined();
  expect(projectsCheck!.status).toBe("warn");
  expect(projectsCheck!.detail).toContain("No projects linked");
});

test("doctor fails on invalid JSON config", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-doctor-"));
  tempDirs.push(appHome);

  await fs.writeFile(globalConfigPath(appHome), "{{invalid json", "utf8");
  await writeJsonFile(projectsRegistryPath(appHome), { version: 1, projects: [] });

  const results = await runDoctorChecks(appHome);
  const configCheck = results.find((r) => r.label === "Global config");
  expect(configCheck).toBeDefined();
  expect(configCheck!.status).toBe("fail");
});

test("doctor detects git on PATH", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-doctor-"));
  tempDirs.push(appHome);

  await saveGlobalConfig({
    version: 1,
    model: null,
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 25 },
    runtime: { runTimeoutSeconds: 1800, maxAttemptsPerTask: 3, noProgressRepeatLimit: 2 },
  }, appHome);
  await writeJsonFile(projectsRegistryPath(appHome), { version: 1, projects: [] });

  const results = await runDoctorChecks(appHome);
  const gitCheck = results.find((r) => r.label === "git");
  expect(gitCheck).toBeDefined();
  expect(gitCheck!.status).toBe("ok");
});

test("formatDoctorResults produces human-readable output", () => {
  const results = [
    { label: "git", status: "ok" as const, detail: "found at /usr/bin/git" },
    { label: "pi", status: "fail" as const, detail: "'pi' not found on PATH" },
    { label: "Budget", status: "warn" as const, detail: "Budget is 0" },
  ];

  const output = formatDoctorResults(results);
  expect(output).toContain("✅ git:");
  expect(output).toContain("❌ pi:");
  expect(output).toContain("⚠️ Budget:");
});
