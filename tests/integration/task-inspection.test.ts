import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { writePromotionArtifact, writePromotionResultArtifact } from "../../src/core/promotion-artifacts.js";
import { getTaskInspection } from "../../src/core/task-inspection.js";
import type { PromotionArtifact, PromotionResultArtifact, TaskLedger } from "../../src/core/types.js";

test("getTaskInspection returns task data without promotion metadata when no artifacts exist", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-task-inspection-empty-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-plain",
        title: "Plain task",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["visible"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        promotedAt: null,
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };

  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const inspection = await getTaskInspection(projectRoot, "task-plain");
  expect(inspection.task.id).toBe("task-plain");
  expect(inspection.latestPromotionRequest).toBeNull();
  expect(inspection.latestPromotionResult).toBeNull();
  expect(inspection.promotionHistory).toEqual([]);
});

test("getTaskInspection includes latest promotion request, result, and merged history", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-task-inspection-history-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-promoted",
        title: "Promoted task",
        kind: "feature",
        status: "promoted",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: "openloop/task-promoted",
        owner: "openloop",
        acceptanceCriteria: ["visible"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        promotedAt: "2026-03-09T10:05:00.000Z",
        notes: [],
        createdAt: "2026-03-09T09:00:00.000Z",
        updatedAt: "2026-03-09T10:05:00.000Z",
      },
    ],
  };

  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const request: PromotionArtifact = {
    version: 1,
    createdAt: "2026-03-09T10:00:00.000Z",
    projectAlias: "demo",
    taskId: "task-promoted",
    baseBranch: "main",
    decision: "auto-merge-eligible",
    action: "queue-auto-merge",
    effectivePromotionMode: "auto-merge",
    validation: [{ name: "test", command: "bun test", exitCode: 0 }],
    piExitCode: 0,
    outcome: "completed",
    status: "applied",
    processedAt: "2026-03-09T10:05:00.000Z",
    note: "Merged automatically",
  };
  const result: PromotionResultArtifact = {
    version: 1,
    createdAt: "2026-03-09T10:05:00.000Z",
    projectAlias: "demo",
    taskId: "task-promoted",
    sourcePromotionArtifactPath: "ignored-for-test",
    sourcePromotionAction: "queue-auto-merge",
    sourcePromotionDecision: "auto-merge-eligible",
    result: "applied",
    branch: "openloop/task-promoted",
    baseBranch: "main",
    note: "Merged automatically",
  };

  await writePromotionArtifact(projectRoot, request);
  await writePromotionResultArtifact(projectRoot, result);

  const inspection = await getTaskInspection(projectRoot, "task-promoted");
  expect(inspection.latestPromotionRequest?.artifact.taskId).toBe("task-promoted");
  expect(inspection.latestPromotionResult?.artifact.result).toBe("applied");
  expect(inspection.promotionHistory).toHaveLength(2);
  expect(inspection.promotionHistory.map((entry) => entry.kind)).toEqual(["request", "result"]);
});