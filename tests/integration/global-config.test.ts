import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { loadGlobalConfig, saveGlobalConfig } from "../../src/core/global-config.js";

const tempHomes: string[] = [];

afterEach(async () => {
  for (const tempHome of tempHomes.splice(0)) {
    await fs.rm(tempHome, { recursive: true, force: true });
  }
});

test("loads global config defaults with runtime settings", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempHomes.push(appHome);

  const config = await loadGlobalConfig(appHome);
  expect(config.budgets.dailyCostUsd).toBe(25);
  expect(config.runtime.runTimeoutSeconds).toBe(1800);
  expect(config.runtime.maxAttemptsPerTask).toBe(3);
  expect(config.runtime.noProgressRepeatLimit).toBe(2);
});

test("merges missing runtime fields from older global config files", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  tempHomes.push(appHome);

  await saveGlobalConfig({
    version: 1,
    model: "demo/model",
    activeProjectAlias: null,
    budgets: { dailyCostUsd: 10 },
    runtime: {
      runTimeoutSeconds: 1800,
      maxAttemptsPerTask: 3,
      noProgressRepeatLimit: 2,
    },
  }, appHome);

  await fs.writeFile(
    path.join(appHome, "config.json"),
    JSON.stringify({ version: 1, model: "demo/model", activeProjectAlias: null, budgets: { dailyCostUsd: 10 } }, null, 2),
    "utf8",
  );

  const config = await loadGlobalConfig(appHome);
  expect(config.model).toBe("demo/model");
  expect(config.budgets.dailyCostUsd).toBe(10);
  expect(config.runtime.runTimeoutSeconds).toBe(1800);
  expect(config.runtime.maxAttemptsPerTask).toBe(3);
  expect(config.runtime.noProgressRepeatLimit).toBe(2);
});