import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { refreshPromotion } from "../../src/cli/commands/promotion.js";
import { addProject, markProjectInitialized } from "../../src/core/project-registry.js";
import { writePromotionResultArtifact, listPromotionResultArtifacts } from "../../src/core/promotion-artifacts.js";
import { makeProjectTask } from "../helpers/factories.js";
import type { PromotionResultArtifact, TaskLedger } from "../../src/core/types.js";

const tempDirs: string[] = [];
const originalOpenloopHome = process.env.OPENLOOP_HOME;
const originalPath = process.env.PATH;

afterEach(async () => {
  process.env.OPENLOOP_HOME = originalOpenloopHome;
  process.env.PATH = originalPath;
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

async function seedProject(alias: string): Promise<{ appHome: string; projectRoot: string }> {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-refresh-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-refresh-proj-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;
  await addProject(alias, projectRoot, appHome);
  await markProjectInitialized(alias, appHome);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify(
      {
        version: 1,
        updatedAt: new Date().toISOString(),
        tasks: [makeProjectTask({ id: "pr-task", title: "PR task", status: "done", risk: "low-risk" })],
      } satisfies TaskLedger,
      null,
      2,
    )}\n`,
  );
  return { appHome, projectRoot };
}

async function seedPrResult(projectRoot: string, prUrl: string): Promise<void> {
  const artifact: PromotionResultArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "pr-task",
    sourcePromotionArtifactPath: "/tmp/source.json",
    sourcePromotionAction: "queue-review",
    sourcePromotionDecision: "manual-review",
    result: "rejected",
    branch: "openloop/pr-task",
    baseBranch: "main",
    note: null,
    prUrl,
  };
  await writePromotionResultArtifact(projectRoot, artifact);
}

test("promotion refresh marks merged PRs promoted", async () => {
  const { projectRoot } = await seedProject("demo");
  await seedPrResult(projectRoot, "https://github.com/org/repo/pull/7");

  const binDir = path.join(os.tmpdir(), `openloop-gh-stub-${Date.now()}`);
  await fs.mkdir(binDir, { recursive: true });
  tempDirs.push(binDir);
  await fs.writeFile(path.join(binDir, "gh"), `#!/bin/sh\necho '{"state":"MERGED","statusCheckRollup":[]}'\n`, { mode: 0o755 });
  process.env.PATH = `${binDir}:${originalPath}`;

  await refreshPromotion({ project: "demo", task: "pr-task" } as never);

  const ledger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(ledger.tasks[0]?.status).toBe("promoted");
  expect(ledger.tasks[0]?.promotedAt).toBeTruthy();

  const results = await listPromotionResultArtifacts(projectRoot, "pr-task");
  const refreshed = results.find((item) => item.artifact.result === "refreshed");
  expect(refreshed?.artifact.state).toBe("MERGED");
});

test("promotion refresh records failed CI checks", async () => {
  const { projectRoot } = await seedProject("demo");
  await seedPrResult(projectRoot, "https://github.com/org/repo/pull/8");

  const binDir = path.join(os.tmpdir(), `openloop-gh-stub-${Date.now()}`);
  await fs.mkdir(binDir, { recursive: true });
  tempDirs.push(binDir);
  await fs.writeFile(
    path.join(binDir, "gh"),
    `#!/bin/sh\necho '{"state":"OPEN","statusCheckRollup":[{"name":"ci/test","status":"COMPLETED","conclusion":"FAILURE"}]}'\n`,
    { mode: 0o755 },
  );
  process.env.PATH = `${binDir}:${originalPath}`;

  await refreshPromotion({ project: "demo", task: "pr-task" } as never);

  const ledger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(ledger.tasks[0]?.status).toBe("done");
  expect(ledger.tasks[0]?.notes?.some((note) => note.includes("PR checks failed: ci/test"))).toBe(true);
});

test("promotion refresh detects failed CI reported via legacy status contexts", async () => {
  const { projectRoot } = await seedProject("demo");
  await seedPrResult(projectRoot, "https://github.com/org/repo/pull/10");

  const binDir = path.join(os.tmpdir(), `openloop-gh-stub-${Date.now()}`);
  await fs.mkdir(binDir, { recursive: true });
  tempDirs.push(binDir);
  await fs.writeFile(
    path.join(binDir, "gh"),
    `#!/bin/sh\necho '{"state":"OPEN","statusCheckRollup":[{"context":"jenkins/build","state":"FAILURE"}]}'\n`,
    { mode: 0o755 },
  );
  process.env.PATH = `${binDir}:${originalPath}`;

  await refreshPromotion({ project: "demo", task: "pr-task" } as never);

  const ledger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(ledger.tasks[0]?.notes?.some((note) => note.includes("PR checks failed: jenkins/build"))).toBe(true);
});

test("promotion refresh fails cleanly when gh is unavailable", async () => {
  const { projectRoot } = await seedProject("demo");
  await seedPrResult(projectRoot, "https://github.com/org/repo/pull/9");

  const emptyBin = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-empty-bin-"));
  tempDirs.push(emptyBin);
  process.env.PATH = emptyBin;

  const originalError = console.error;
  const messages: string[] = [];
  console.error = (message: string) => {
    messages.push(message);
  };
  try {
    await refreshPromotion({ project: "demo", task: "pr-task" } as never);
  } finally {
    console.error = originalError;
  }

  expect(process.exitCode).toBe(1);
  expect(messages.join("\n")).toContain("gh");
  // No state change when gh is missing.
  const ledger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(ledger.tasks[0]?.status).toBe("done");
});
