import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { addTask, getTask, listTasks, loadTaskLedger, summarizeTasks, withTaskLedger } from "../../src/core/task-ledger.js";
import type { ProjectTask, TaskLedger } from "../../src/core/types.js";

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

test("parallel addTask calls under the ledger lock lose nothing", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-task-lock-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  await Promise.all(
    Array.from({ length: 50 }, (_, index) =>
      addTask(projectRoot, {
        id: `task-${index}`,
        title: `Task ${index}`,
        kind: "feature",
        status: "proposed",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: [],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      }),
    ),
  );

  const raw = await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8");
  const ledger = JSON.parse(raw) as TaskLedger;
  expect(ledger.tasks).toHaveLength(50);

  // The lock file is released after the final mutation.
  await expect(fs.stat(path.join(projectRoot, ".openloop", ".tasks.lock"))).rejects.toThrow();
});

test("withTaskLedger breaks a stale lock instead of hanging", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-task-stale-lock-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  // Simulate a lock left behind by a crashed holder (older than the stale window).
  const lockPath = path.join(projectRoot, ".openloop", ".tasks.lock");
  await fs.writeFile(lockPath, "999999\n", "utf8");
  const stale = new Date(Date.now() - 11 * 60 * 1000);
  await fs.utimes(lockPath, stale, stale);

  await withTaskLedger(projectRoot, (ledger) => {
    ledger.tasks.push({
      id: "after-stale",
      title: "Recovered",
      kind: "feature",
      status: "ready",
      risk: "low-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: [],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "pull-request",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    });
  });

  const ledger = await loadTaskLedger(projectRoot);
  expect(ledger.tasks.map((task) => task.id)).toEqual(["after-stale"]);
});

test("withTaskLedger rejects while another process holds a fresh lock", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-task-fresh-lock-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  await fs.writeFile(path.join(projectRoot, ".openloop", ".tasks.lock"), `${process.pid}\n`, "utf8");

  await expect(withTaskLedger(projectRoot, () => {})).rejects.toThrow("Task ledger is locked by another openloop process");
});

test("addTask rejects unknown local dependsOn ids and accepts cross-project refs", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-task-depends-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const base = {
    id: "existing-dep",
    title: "Existing",
    kind: "feature",
    status: "promoted",
    risk: "low-risk",
    source: { type: "human", ref: "test" },
    specId: null,
    branch: null,
    owner: null,
    acceptanceCriteria: [],
    attempts: 0,
    lastFailureSignature: null,
    promotion: "pull-request",
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  } as ProjectTask;
  await addTask(projectRoot, { ...base });

  const dependent = {
    ...base,
    id: "dependent-task",
    title: "Dependent",
    status: "proposed",
    dependsOn: ["missing-dep"],
  } as ProjectTask;
  await expect(addTask(projectRoot, { ...dependent })).rejects.toThrow("Unknown task id in dependsOn: missing-dep");

  const crossProject = {
    ...base,
    id: "cross-task",
    title: "Cross",
    status: "proposed",
    dependsOn: ["other-project:task-1", "existing-dep"],
  } as ProjectTask;
  await expect(addTask(projectRoot, { ...crossProject })).resolves.toBeUndefined();
});
