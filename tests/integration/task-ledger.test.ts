import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { getTask, listTasks, summarizeTasks } from "../../src/core/task-ledger.js";
import type { TaskLedger } from "../../src/core/types.js";

test("getTask returns a task by id from the project ledger", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-task-ledger-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-lookup",
        title: "Lookup task",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["lookup works"],
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

  const task = await getTask(projectRoot, "task-lookup");
  expect(task.id).toBe("task-lookup");
  expect(task.title).toBe("Lookup task");
});

test("listTasks filters by status and risk and sorts by update time", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-task-list-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-old",
        title: "Older task",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["done"],
        attempts: 1,
        lastFailureSignature: null,
        promotion: "auto-merge",
        promotedAt: null,
        notes: [],
        createdAt: "2026-03-08T10:00:00.000Z",
        updatedAt: "2026-03-08T12:00:00.000Z",
      },
      {
        id: "task-new",
        title: "Newest task",
        kind: "bugfix",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["done"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        promotedAt: null,
        notes: [],
        createdAt: "2026-03-08T11:00:00.000Z",
        updatedAt: "2026-03-08T13:00:00.000Z",
      },
      {
        id: "task-blocked",
        title: "Blocked task",
        kind: "docs",
        status: "blocked",
        risk: "high-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["review needed"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "manual-only",
        promotedAt: null,
        notes: [],
        createdAt: "2026-03-08T09:00:00.000Z",
        updatedAt: "2026-03-08T09:30:00.000Z",
      },
    ],
  };

  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const doneTasks = await listTasks(projectRoot, { status: "done", risk: "low-risk" });
  expect(doneTasks.map((task) => task.id)).toEqual(["task-new", "task-old"]);

  const blockedTasks = await listTasks(projectRoot, { status: "blocked" });
  expect(blockedTasks.map((task) => task.id)).toEqual(["task-blocked"]);

  const summary = summarizeTasks(await listTasks(projectRoot));
  expect(summary.total).toBe(3);
  expect(summary.byStatus.done).toBe(2);
  expect(summary.byStatus.blocked).toBe(1);
  expect(summary.byStatus.promoted).toBe(0);
  expect(summary.byRisk["low-risk"]).toBe(2);
  expect(summary.byRisk["high-risk"]).toBe(1);
});