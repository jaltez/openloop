import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { writeRunSummary } from "../../src/core/run-summaries.js";

test("writes run summaries into the target project's control plane", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-summary-"));
  const filePath = await writeRunSummary(projectRoot, {
    projectAlias: "demo",
    taskId: "task-1",
    mode: "plan",
    role: "sdd-planner",
    reason: "selected task requiring planning",
    model: "anthropic/test-model",
    exitCode: 0,
    validation: [],
    promotionDecision: "none",
    promotionAction: "none",
    promotionArtifactPath: null,
    promotionResultArtifactPath: null,
    taskStatus: "ready",
    promotedAt: null,
    stoppedBy: "none",
    attemptNumber: 1,
    dirtyTreeDetected: false,
    budgetSnapshotUsd: 0,
  });

  const content = await fs.readFile(filePath, "utf8");
  expect(content).toContain("project: demo");
  expect(content).toContain("task: task-1");
  expect(content).toContain("mode: plan");
  expect(content).toContain("role: sdd-planner");
  expect(content).toContain("promotionAction: none");
  expect(content).toContain("taskStatus: ready");
  expect(content).toContain("stoppedBy: none");
  expect(content).toContain("attemptNumber: 1");
  expect(content).toContain("dirtyTreeDetected: false");
  expect(content).toContain("budgetSnapshotUsd: 0");
});