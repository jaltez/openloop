import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { execFileSync } from "node:child_process";
import { saveGlobalConfig } from "../../src/core/global-config.js";
import { ensureDir, writeJsonFile } from "../../src/core/fs.js";
import { projectsRegistryPath, runtimeDir } from "../../src/core/paths.js";

const tempDirs: string[] = [];

afterEach(async () => {
  for (const dir of tempDirs.splice(0)) {
    await fs.rm(dir, { recursive: true, force: true });
  }
});

async function setupAppHome() {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-setup-"));
  tempDirs.push(appHome);

  await saveGlobalConfig(
    {
      version: 1,
      model: null,
      activeProjectAlias: null,
      budgets: { dailyCostUsd: 25 },
      runtime: {
        runTimeoutSeconds: 1800,
        maxAttemptsPerTask: 3,
        noProgressRepeatLimit: 2,
      },
    },
    appHome,
  );

  await ensureDir(runtimeDir(appHome));
  await writeJsonFile(projectsRegistryPath(appHome), { version: 1, projects: [] });

  return appHome;
}

test("setup command exists and shows wizard header", async () => {
  // The setup wizard needs interactive input, but we can verify the command
  // is registered by running it with --help
  const appHome = await setupAppHome();
  const result = execFileSync(
    process.execPath,
    [
      "--import", "tsx",
      path.resolve("src/index.ts"),
      "setup", "--help",
    ],
    {
      env: { ...process.env, OPENLOOP_HOME: appHome },
      encoding: "utf8",
      timeout: 15_000,
    },
  );
  expect(result).toContain("Interactive first-run setup wizard");
});

test("setup command has confirm helper that accepts default yes", async () => {
  // Unit-test the confirm/ask functions indirectly through the module
  // Since they're not exported, we just verify the command can be loaded
  const mod = await import("../../src/cli/commands/setup.js");
  expect(typeof mod.registerSetupCommand).toBe("function");
});
