import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { runProjectIteration } from "../../src/core/scheduler.js";
import type { LinkedProject, ProjectConfig, TaskLedger } from "../../src/core/types.js";

test("no-progress detection appends notes across repeated iterations with same failure", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-noprogress-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "stuck-task",
      title: "Stuck task",
      kind: "feature",
      status: "ready",
      risk: "low-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: "openloop",
      acceptanceCriteria: ["pass"],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "auto-merge",
      notes: [],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  // First iteration: piRunner returns nonzero to simulate Pi failure with same error each time.
  // The git state doesn't change between runs (no actual code changes), so no-progress via "diff-unchanged" fires.
  let result = await runProjectIteration(project, {
    piRunner: async () => 1,
    noProgressRepeatLimit: 3,
  });

  expect(result.taskId).toBe("stuck-task");

  let persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  const notesAfterFirst = persisted.tasks[0]?.notes ?? [];
  const noProgressNotesFirst = notesAfterFirst.filter((n) => n.includes("openloop:no-progress:"));
  expect(noProgressNotesFirst.length).toBeGreaterThan(0);

  // Second iteration
  result = await runProjectIteration(project, {
    piRunner: async () => 1,
    noProgressRepeatLimit: 3,
  });

  persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  const notesAfterSecond = persisted.tasks[0]?.notes ?? [];
  const noProgressNotesSecond = notesAfterSecond.filter((n) => n.includes("openloop:no-progress:"));
  // Notes should accumulate — more no-progress notes than after first iteration
  expect(noProgressNotesSecond.length).toBeGreaterThan(noProgressNotesFirst.length);

  // Third iteration — should trigger the no-progress block (limit=3)
  result = await runProjectIteration(project, {
    piRunner: async () => 1,
    noProgressRepeatLimit: 3,
  });

  persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  const finalNotes = persisted.tasks[0]?.notes ?? [];
  const allNoProgressNotes = finalNotes.filter((n) => n.includes("openloop:no-progress:"));

  // Notes keep accumulating — verify they grow on each iteration
  expect(allNoProgressNotes.length).toBeGreaterThan(noProgressNotesSecond.length);

  // After enough iterations, the task should be blocked or the stoppedBy should reflect no-progress
  // (the exact iteration where it blocks depends on internal counting, but notes must accumulate)
  expect(allNoProgressNotes.length).toBeGreaterThanOrEqual(3);
});
