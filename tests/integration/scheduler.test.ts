import { execFileSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test, vi } from "vitest";
import { determineWorkerRole, runProjectIteration, selectNextTask } from "../../src/core/scheduler.js";
import type { LinkedProject, ProjectConfig, ProjectTask, TaskLedger } from "../../src/core/types.js";
import { fakeAgentRun, initGitRepo } from "../helpers/factories.js";

test("determineWorkerRole maps planner, implementer, improver, and healer roles", () => {
  expect(
    determineWorkerRole(
      {
        id: "plan-me",
        title: "Plan me",
        kind: "feature",
        status: "proposed",
        risk: "medium-risk",
        scope: null,
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["plan"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      "plan",
    ),
  ).toBe("sdd-planner");

  expect(
    determineWorkerRole(
      {
        id: "impl-me",
        title: "Implement me",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        scope: null,
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["implement"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      "implement",
    ),
  ).toBe("implementer");

  expect(
    determineWorkerRole(
      {
        id: "heal-me",
        title: "Heal me",
        kind: "lint-fix",
        status: "ready",
        risk: "low-risk",
        scope: null,
        source: { type: "ci", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["heal"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      "implement",
    ),
  ).toBe("ci-healer");

  expect(
    determineWorkerRole(
      {
        id: "improve-me",
        title: "Improve me",
        kind: "discovery",
        status: "proposed",
        risk: "medium-risk",
        scope: null,
        source: { type: "discovery", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["improve"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "manual-only",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      "plan",
    ),
  ).toBe("repo-improver");
});

test("selectNextTask prioritizes ready self-healing work over planning tasks", () => {
  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "plan-me",
        title: "Plan me",
        kind: "feature",
        status: "proposed",
        risk: "medium-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Create a plan"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      {
        id: "lint-fix-1",
        title: "Fix lint error",
        kind: "lint-fix",
        status: "ready",
        risk: "low-risk",
        source: { type: "ci", ref: "lint" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Lint passes"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };

  const selection = selectNextTask(ledger);

  expect(selection.task?.id).toBe("lint-fix-1");
  expect(selection.mode).toBe("implement");
});

test("selectNextTask falls back to ready medium-risk tasks when no low-risk task is available", () => {
  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "medium-ready",
        title: "Ready medium risk task",
        kind: "feature",
        status: "ready",
        risk: "medium-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Implement me"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      {
        id: "plan-later",
        title: "Plan later",
        kind: "feature",
        status: "proposed",
        risk: "medium-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Plan me"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };

  const selection = selectNextTask(ledger);

  expect(selection.task?.id).toBe("medium-ready");
  expect(selection.mode).toBe("implement");
});

test("selectNextTask promotes medium-risk tasks that aged past 24h over fresher low-risk work", () => {
  const now = new Date("2026-03-10T12:00:00.000Z");
  const fresh = new Date("2026-03-10T11:00:00.000Z").toISOString();
  const stale = new Date("2026-03-09T08:00:00.000Z").toISOString();
  const ledger: TaskLedger = {
    version: 1,
    updatedAt: now.toISOString(),
    tasks: [
      {
        id: "fresh-low",
        title: "Fresh low-risk",
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
        notes: [],
        createdAt: fresh,
        updatedAt: fresh,
      },
      {
        id: "stale-medium",
        title: "Stale medium-risk",
        kind: "feature",
        status: "ready",
        risk: "medium-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: [],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: stale,
        updatedAt: stale,
      },
    ],
  };

  const selection = selectNextTask(ledger, now);

  expect(selection.task?.id).toBe("stale-medium");
  expect(selection.mode).toBe("implement");
  expect(selection.reason).toContain("aged past 24h");
});

test("selectNextTask keeps low-risk priority when medium-risk work is fresh", () => {
  const now = new Date("2026-03-10T12:00:00.000Z");
  const recent = new Date("2026-03-10T11:30:00.000Z").toISOString();
  const task = (id: string, risk: "low-risk" | "medium-risk"): ProjectTask => ({
    id,
    title: id,
    kind: "feature",
    status: "ready",
    risk,
    source: { type: "human", ref: "test" },
    specId: null,
    branch: null,
    owner: null,
    acceptanceCriteria: [],
    attempts: 0,
    lastFailureSignature: null,
    promotion: "pull-request",
    notes: [],
    createdAt: recent,
    updatedAt: recent,
  });
  const ledger: TaskLedger = {
    version: 1,
    updatedAt: recent,
    tasks: [task("fresh-medium", "medium-risk"), task("fresh-low", "low-risk")],
  };

  const selection = selectNextTask(ledger, now);

  expect(selection.task?.id).toBe("fresh-low");
  expect(selection.reason).toContain("low-risk");
});

test("selectNextTask skips ready tasks with unsatisfied dependsOn and names the blocker", () => {
  const now = new Date("2026-03-10T12:00:00.000Z");
  const recent = new Date("2026-03-10T11:00:00.000Z").toISOString();
  const task = (id: string, extra?: Partial<ProjectTask>): ProjectTask => ({
    id,
    title: id,
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
    notes: [],
    createdAt: recent,
    updatedAt: recent,
    ...extra,
  });
  const ledger: TaskLedger = {
    version: 1,
    updatedAt: recent,
    tasks: [task("blocked-a", { dependsOn: ["missing-dep"] }), task("blocked-b", { dependsOn: ["still-ready"] }), task("still-ready")],
  };

  const selection = selectNextTask(ledger, now);

  expect(selection.task?.id).toBe("still-ready");
  expect(selection.mode).toBe("implement");

  // With every ready task blocked, the idle reason names the blockers.
  const allBlocked: TaskLedger = {
    version: 1,
    updatedAt: recent,
    tasks: [task("blocked-a", { dependsOn: ["missing-dep"] })],
  };
  const blockedSelection = selectNextTask(allBlocked, now);
  expect(blockedSelection.task).toBeNull();
  expect(blockedSelection.reason).toContain("unsatisfied dependsOn: missing-dep");
});

test("selectNextTask treats promoted dependencies as satisfied", () => {
  const now = new Date("2026-03-10T12:00:00.000Z");
  const recent = new Date("2026-03-10T11:00:00.000Z").toISOString();
  const task = (id: string, status: ProjectTask["status"], extra?: Partial<ProjectTask>): ProjectTask => ({
    id,
    title: id,
    kind: "feature",
    status,
    risk: "low-risk",
    source: { type: "human", ref: "test" },
    specId: null,
    branch: null,
    owner: null,
    acceptanceCriteria: [],
    attempts: 0,
    lastFailureSignature: null,
    promotion: "pull-request",
    notes: [],
    createdAt: recent,
    updatedAt: recent,
    ...extra,
  });
  const ledger: TaskLedger = {
    version: 1,
    updatedAt: recent,
    tasks: [task("dependent", "ready", { dependsOn: ["promoted-dep"] }), task("promoted-dep", "promoted")],
  };

  const selection = selectNextTask(ledger, now);

  expect(selection.task?.id).toBe("dependent");
});

test("selectNextTask never schedules cross-project dependsOn and names it in the idle reason", () => {
  const now = new Date("2026-03-10T12:00:00.000Z");
  const recent = new Date("2026-03-10T11:00:00.000Z").toISOString();
  const task: ProjectTask = {
    id: "cross-dep",
    title: "cross-dep",
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
    notes: [],
    createdAt: recent,
    updatedAt: recent,
    dependsOn: ["other-project:task-9"],
  };
  const ledger: TaskLedger = { version: 1, updatedAt: recent, tasks: [task] };

  const first = selectNextTask(ledger, now);
  expect(first.task).toBeNull();
  expect(first.reason).toContain("other-project:task-9");
  expect(first.reason).toContain("cross-project");

  const second = selectNextTask(ledger, now);
  expect(second.task).toBeNull();

  // Blocking is surfaced, not silently dropped.
  expect(second.reason).toContain("other-project:task-9");
});

test("runProjectIteration executes ready medium-risk tasks and queues manual review", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-medium-ready-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "npm run lint", testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "medium-ready",
        title: "Ready medium risk task",
        kind: "feature",
        status: "ready",
        risk: "medium-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Implement me"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
    validationRunner: async () => 0,
  });

  expect(result.taskId).toBe("medium-ready");
  expect(result.mode).toBe("implement");
  expect(result.taskStatus).toBe("done");
  expect(result.promotionDecision).toBe("manual-review");
  expect(result.promotionAction).toBe("queue-review");
});

test("runProjectIteration moves proposed medium-risk tasks to awaiting-approval after a successful planning pass", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: "anthropic/project-model", promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "plan-me",
        title: "Plan me",
        kind: "feature",
        status: "proposed",
        risk: "medium-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Create a plan"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
  });

  expect(result.mode).toBe("plan");
  expect(result.taskId).toBe("plan-me");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("awaiting-approval");
});

test("runProjectIteration auto-generates a validation discovery task for idle projects without validation commands", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-improve-validation-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`,
    "utf8",
  );

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
  });

  expect(result.mode).toBe("plan");
  expect(result.taskId).toBe("improve-validation-setup");
  expect(result.reason).toContain("generated continuous improvement backlog task");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks).toHaveLength(1);
  expect(persisted.tasks[0]?.kind).toBe("discovery");
  expect(persisted.tasks[0]?.status).toBe("awaiting-approval");
  expect(persisted.tasks[0]?.source.ref).toBe("continuous-improvement:validation-setup");
});

test("runProjectIteration auto-generates a scope proposal task when validation exists but scope policy is missing", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-improve-scope-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "npm run lint", testCommand: "npm test", typecheckCommand: "npm run typecheck" },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`,
    "utf8",
  );

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
  });

  expect(result.mode).toBe("plan");
  expect(result.taskId).toBe("define-scope-policy");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks).toHaveLength(1);
  expect(persisted.tasks[0]?.kind).toBe("scope-proposal");
  expect(persisted.tasks[0]?.status).toBe("awaiting-approval");
  expect(persisted.tasks[0]?.promotion).toBe("manual-only");
});

test("runProjectIteration auto-generates a targeted test-command task when validation is partially configured", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-improve-test-command-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "npm run lint", testCommand: null, typecheckCommand: "npm run typecheck" },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    ["version: 1", "scope:", "  allowGlobs:", "    - src/**", "  denyGlobs: []", "  highRiskAreas: []"].join("\n") + "\n",
    "utf8",
  );
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`,
    "utf8",
  );

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
  });

  expect(result.mode).toBe("plan");
  expect(result.taskId).toBe("define-test-command");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks).toHaveLength(1);
  expect(persisted.tasks[0]?.source.ref).toBe("continuous-improvement:test-command");
  expect(persisted.tasks[0]?.status).toBe("awaiting-approval");
});

test("runProjectIteration blocks tasks that exceed max attempts before execution", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-attempts-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "attempt-limit",
        title: "Attempt limit",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Create a plan"],
        attempts: 3,
        lastFailureSignature: "validation-test",
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const piRunner = vi.fn(async () => fakeAgentRun());
  const result = await runProjectIteration(project, {
    piRunner,
    maxAttemptsPerTask: 3,
  });

  expect(piRunner).not.toHaveBeenCalled();
  expect(result.taskStatus).toBe("blocked");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("blocked");
  expect(persisted.tasks[0]?.lastFailureSignature).toBe("max-attempts-reached");
});

test("runProjectIteration allows localized deterministic test self-healing tasks", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-self-heal-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: "npm test", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    verification: { enabled: false },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    ["version: 1", "selfHealing:", "  enabled: true", "  allowedTaskKinds:", "    - localized-test-fix"].join("\n") + "\n",
    "utf8",
  );

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "heal-test-1",
        title: "Repair a localized test",
        kind: "localized-test-fix",
        status: "ready",
        risk: "low-risk",
        source: { type: "ci", ref: "failing test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["The failing test is repaired"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const piRunner = vi.fn(async () => fakeAgentRun());
  const result = await runProjectIteration(project, {
    piRunner,
    validationRunner: async () => 0,
  });

  expect(piRunner).toHaveBeenCalledTimes(1);
  expect(result.taskStatus).toBe("done");
  expect(result.reason).not.toContain("outside the V1 scope");
  expect(result.promotionAction).toBe("queue-review");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("done");
  expect(persisted.tasks[0]?.lastFailureSignature).toBeNull();
});

test("runProjectIteration blocks unsupported self-healing task kinds without invoking Pi", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-self-heal-unsupported-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: "npm test", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    ["version: 1", "selfHealing:", "  enabled: true", "  allowedTaskKinds:", "    - ci-heal"].join("\n") + "\n",
    "utf8",
  );

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "heal-ci-1",
        title: "Repair a CI issue",
        kind: "ci-heal",
        status: "ready",
        risk: "low-risk",
        source: { type: "ci", ref: "failing ci" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["The CI issue is repaired"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const piRunner = vi.fn(async () => fakeAgentRun());
  const result = await runProjectIteration(project, {
    piRunner,
  });

  expect(piRunner).not.toHaveBeenCalled();
  expect(result.taskStatus).toBe("blocked");
  expect(result.reason).toContain("outside the V1 scope");
  expect(result.promotionAction).toBe("none");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("blocked");
  expect(persisted.tasks[0]?.lastFailureSignature).toBe("self-healing-kind-not-supported");
});

test("runProjectIteration blocks tasks that target denied policy paths", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-policy-denied-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    ["version: 1", "scope:", "  allowGlobs:", "    - src/**", "  denyGlobs:", "    - src/secrets/**", "  highRiskAreas: []"].join("\n") +
      "\n",
    "utf8",
  );

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "denied-scope",
        title: "Touch denied path",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        scope: { paths: ["src/secrets/token.ts"] },
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["blocked"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const piRunner = vi.fn(async () => fakeAgentRun());
  const result = await runProjectIteration(project, { piRunner });

  expect(piRunner).not.toHaveBeenCalled();
  expect(result.taskStatus).toBe("blocked");
  expect(result.reason).toContain("denied paths");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.lastFailureSignature).toBe("policy-scope-denied");
});

test("runProjectIteration escalates high-risk policy areas before promotion decisions", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-policy-high-risk-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "npm run lint", testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    [
      "version: 1",
      "scope:",
      "  allowGlobs:",
      "    - src/**",
      "  denyGlobs: []",
      "  highRiskAreas:",
      "    - src/payments/**",
      "promotion:",
      "  lowRiskMode: auto-merge",
      "  mediumRiskMode: pull-request",
      "  highRiskMode: pull-request",
    ].join("\n") + "\n",
    "utf8",
  );

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "high-risk-scope",
        title: "Touch payments code",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        scope: { paths: ["src/payments/charge.ts"] },
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["manual review"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
    validationRunner: async () => 0,
  });

  expect(result.taskStatus).toBe("done");
  expect(result.promotionAction).toBe("queue-review");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.risk).toBe("high-risk");
  expect(persisted.tasks[0]?.notes?.some((note) => note.includes("high-risk areas"))).toBe(true);
});

test("runProjectIteration aborts when useWorktree is enabled but worktree creation fails", async () => {
  // No git repo initialized — git worktree add will fail, triggering fail-closed.
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-worktree-fail-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: true, branchPrefix: "openloop/" },
    validation: { lintCommand: "npm run lint", testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "worktree-task",
        title: "Isolated task",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Do the thing"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const piRunner = vi.fn(async () => fakeAgentRun());

  await expect(
    runProjectIteration(project, {
      piRunner,
      validationRunner: async () => 0,
    }),
  ).rejects.toThrow(/Worktree isolation failed/);

  // Fail closed: the agent must never execute when isolation cannot be established.
  expect(piRunner).not.toHaveBeenCalled();

  // The task is recorded as a failed attempt and reverted, not left stuck in-progress.
  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.attempts).toBe(1);
  expect(persisted.tasks[0]?.status).toBe("ready");
});

test("runProjectIteration downgrades auto-merge when review detects a hardcoded secret", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-review-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  // Real git repo so getDiffPatch has an actual diff to analyze.
  execFileSync("git", ["init"], { cwd: projectRoot });
  execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: projectRoot });
  execFileSync("git", ["config", "user.name", "Test"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "init.txt"), "init\n");
  execFileSync("git", ["add", "."], { cwd: projectRoot });
  execFileSync("git", ["commit", "-m", "init"], { cwd: projectRoot });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    review: { enabled: true },
    validation: { lintCommand: "npm run lint", testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "secret-task",
        title: "Add config",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Add config"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  // Mock runner simulates the agent writing a hardcoded AWS key into a tracked file.
  const piRunner = async () => {
    await fs.writeFile(path.join(projectRoot, "init.txt"), "AWS_KEY=AKIAIOSFODNN7EXAMPLE\n");
    return fakeAgentRun();
  };

  const result = await runProjectIteration(project, {
    piRunner,
    validationRunner: async () => 0,
  });

  // Without review this would be auto-merge-eligible. The deterministic secret
  // detection catches what validation structurally cannot, downgrading to manual-review.
  expect(result.taskStatus).toBe("done");
  expect(result.promotionDecision).toBe("manual-review");
  expect(result.promotionAction).toBe("queue-review");
  expect(result.reviewFindings?.some((f) => f.rule.includes("secret-detection"))).toBe(true);

  // Queued promotions produce a human-approval packet with full provenance.
  expect(result.approvalPacketPath).toBe(path.join(projectRoot, ".openloop", "approvals", "secret-task.json"));
  const packet = JSON.parse(await fs.readFile(result.approvalPacketPath!, "utf8"));
  expect(packet.taskId).toBe("secret-task");
  expect(packet.risk).toBe("low-risk");
  expect(packet.costSource).toBe("estimated");
  expect(packet.reviewFindings.some((f: { rule: string }) => f.rule.includes("secret-detection"))).toBe(true);
  expect(packet.diffStat).toEqual(expect.arrayContaining([{ file: "init.txt", added: 1, removed: 1 }]));
  expect(await fs.stat(packet.runSummaryPath)).toBeDefined();
  // The promotion artifact references the packet.
  const artifact = JSON.parse(await fs.readFile(result.promotionArtifactPath!, "utf8"));
  expect(artifact.approvalPacketPath).toBe(result.approvalPacketPath);
});

test("runProjectIteration downgrades auto-merge when reviewer output is malformed", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-malformed-review-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  execFileSync("git", ["init"], { cwd: projectRoot });
  execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: projectRoot });
  execFileSync("git", ["config", "user.name", "Test"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "init.txt"), "init\n");
  execFileSync("git", ["add", "."], { cwd: projectRoot });
  execFileSync("git", ["commit", "-m", "init"], { cwd: projectRoot });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    review: { enabled: true },
    validation: { lintCommand: null, testCommand: "true", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "malformed-review-task",
        title: "Add config",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Add config"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const piRunner = async () => {
    await fs.writeFile(path.join(projectRoot, "init.txt"), "changed\n");
    return fakeAgentRun();
  };

  // The reviewer writes an unparseable file: fail closed, not "no findings".
  const reviewerRunner = async () => {
    await fs.mkdir(path.join(projectRoot, ".openloop", "reviews"), { recursive: true });
    await fs.writeFile(path.join(projectRoot, ".openloop", "reviews", "malformed-review-task.json"), "{ this is not json", "utf8");
    return 0;
  };

  const result = await runProjectIteration(project, {
    piRunner,
    validationRunner: async () => 0,
    reviewerRunner,
  });

  expect(result.taskStatus).toBe("done");
  expect(result.promotionDecision).toBe("manual-review");
  expect(result.promotionAction).toBe("queue-review");
  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.notes?.some((note) => note.includes("Reviewer output malformed"))).toBe(true);
});

test("runProjectIteration executes the agent and validations inside the worktree in worktree mode", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-worktree-ok-"));
  await initGitRepo(projectRoot);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: true, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: "test -f touched-by-agent.txt", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "worktree-happy",
        title: "Isolated task",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Do the thing"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const worktreePath = path.join(projectRoot, ".openloop", "worktrees", "worktree-happy");

  // The agent writes its file wherever it is told to run — the worktree.
  const piRunner = async (options: { project: LinkedProject }) => {
    await fs.writeFile(path.join(options.project.path, "touched-by-agent.txt"), "agent output\n");
    return fakeAgentRun({ stdout: "agent transcript output\n", usage: { costUsd: 0.42 } });
  };

  // Run the real test command in the path validation receives: pre-fix this
  // executed on the main tree where touched-by-agent.txt does not exist.
  const validationRunner = vi.fn(async (projectPath: string, command: string) => {
    try {
      execFileSync("sh", ["-c", command], { cwd: projectPath, stdio: "ignore" });
      return 0;
    } catch {
      return 1;
    }
  });

  const result = await runProjectIteration(project, { piRunner, validationRunner });

  expect(result.taskStatus).toBe("done");
  expect(result.validation).toEqual([{ name: "test", command: "test -f touched-by-agent.txt", exitCode: 0 }]);
  expect(validationRunner).toHaveBeenCalledTimes(1);
  expect(validationRunner.mock.calls[0]?.[0]).toBe(worktreePath);

  // Measured provider cost flows through to the scheduler result.
  expect(result.costUsd).toBe(0.42);
  expect(result.costSource).toBe("measured");

  // The agent transcript is persisted to the control-plane runs directory.
  const runsDir = path.join(projectRoot, ".openloop", "runs");
  const transcripts = (await fs.readdir(runsDir)).filter((name) => name.endsWith(".transcript.log"));
  expect(transcripts).toHaveLength(1);
  const transcript = await fs.readFile(path.join(runsDir, transcripts[0]!), "utf8");
  expect(transcript).toContain("agent transcript output");

  // The worktree is cleaned up after the run.
  await expect(fs.stat(worktreePath)).rejects.toThrow();
});

test("runProjectIteration fails closed when the worktree setup command fails", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-worktree-setup-"));
  await initGitRepo(projectRoot);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: true, branchPrefix: "openloop/", worktreeSetupCommand: "exit 3" },
    validation: { lintCommand: null, testCommand: "true", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "setup-fail-task",
        title: "Isolated task",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Do the thing"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
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

  const piRunner = vi.fn(async () => fakeAgentRun());
  const validationRunner = vi.fn(async () => 0);

  await expect(runProjectIteration(project, { piRunner, validationRunner })).rejects.toThrow(/Worktree setup command failed: exit 3/);

  expect(piRunner).not.toHaveBeenCalled();
  expect(validationRunner).not.toHaveBeenCalled();

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.attempts).toBe(1);
  expect(persisted.tasks[0]?.status).toBe("ready");
});
