import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test, vi } from "vitest";
import { determineWorkerRole, runProjectIteration, selectNextTask } from "../../src/core/scheduler.js";
import type { LinkedProject, ProjectConfig, TaskLedger } from "../../src/core/types.js";

test("determineWorkerRole maps planner, implementer, improver, and healer roles", () => {
  expect(determineWorkerRole({
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
  }, "plan")).toBe("sdd-planner");

  expect(determineWorkerRole({
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
  }, "implement")).toBe("implementer");

  expect(determineWorkerRole({
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
  }, "implement")).toBe("ci-healer");

  expect(determineWorkerRole({
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
  }, "plan")).toBe("repo-improver");
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

test("runProjectIteration executes ready medium-risk tasks and queues manual review", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-medium-ready-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
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
    piRunner: async () => 0,
    validationRunner: async () => 0,
  });

  expect(result.taskId).toBe("medium-ready");
  expect(result.mode).toBe("implement");
  expect(result.taskStatus).toBe("done");
  expect(result.promotionDecision).toBe("manual-review");
  expect(result.promotionAction).toBe("queue-review");
});

test("runProjectIteration moves proposed tasks to ready after a successful planning pass", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: "anthropic/project-model", promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
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
    piRunner: async () => 0,
  });

  expect(result.mode).toBe("plan");
  expect(result.taskId).toBe("plan-me");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("ready");
});

test("runProjectIteration auto-generates a validation discovery task for idle projects without validation commands", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-improve-validation-"));
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
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`, "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, {
    piRunner: async () => 0,
  });

  expect(result.mode).toBe("plan");
  expect(result.taskId).toBe("improve-validation-setup");
  expect(result.reason).toContain("generated continuous improvement backlog task");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks).toHaveLength(1);
  expect(persisted.tasks[0]?.kind).toBe("discovery");
  expect(persisted.tasks[0]?.status).toBe("ready");
  expect(persisted.tasks[0]?.source.ref).toBe("continuous-improvement:validation-setup");
});

test("runProjectIteration auto-generates a scope proposal task when validation exists but scope policy is missing", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-improve-scope-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "npm run lint", testCommand: "npm test", typecheckCommand: "npm run typecheck" },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`, "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, {
    piRunner: async () => 0,
  });

  expect(result.mode).toBe("plan");
  expect(result.taskId).toBe("define-scope-policy");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks).toHaveLength(1);
  expect(persisted.tasks[0]?.kind).toBe("scope-proposal");
  expect(persisted.tasks[0]?.status).toBe("ready");
  expect(persisted.tasks[0]?.promotion).toBe("manual-only");
});

test("runProjectIteration auto-generates a targeted test-command task when validation is partially configured", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-improve-test-command-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "npm run lint", testCommand: null, typecheckCommand: "npm run typecheck" },
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
      "  highRiskAreas: []",
    ].join("\n") + "\n",
    "utf8",
  );
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify({ version: 1, updatedAt: new Date().toISOString(), tasks: [] }, null, 2)}\n`, "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };

  const result = await runProjectIteration(project, {
    piRunner: async () => 0,
  });

  expect(result.mode).toBe("plan");
  expect(result.taskId).toBe("define-test-command");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks).toHaveLength(1);
  expect(persisted.tasks[0]?.source.ref).toBe("continuous-improvement:test-command");
  expect(persisted.tasks[0]?.status).toBe("ready");
});

test("runProjectIteration blocks tasks that exceed max attempts before execution", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-scheduler-attempts-"));
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

  const piRunner = vi.fn(async () => 0);
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
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: "npm test", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    [
      "version: 1",
      "selfHealing:",
      "  enabled: true",
      "  allowedTaskKinds:",
      "    - localized-test-fix",
    ].join("\n") + "\n",
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

  const piRunner = vi.fn(async () => 0);
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
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: "npm test", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    [
      "version: 1",
      "selfHealing:",
      "  enabled: true",
      "  allowedTaskKinds:",
      "    - ci-heal",
    ].join("\n") + "\n",
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

  const piRunner = vi.fn(async () => 0);
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
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
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
      "  denyGlobs:",
      "    - src/secrets/**",
      "  highRiskAreas: []",
    ].join("\n") + "\n",
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

  const piRunner = vi.fn(async () => 0);
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
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
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
    piRunner: async () => 0,
    validationRunner: async () => 0,
  });

  expect(result.taskStatus).toBe("done");
  expect(result.promotionAction).toBe("queue-review");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.risk).toBe("high-risk");
  expect(persisted.tasks[0]?.notes?.some((note) => note.includes("high-risk areas"))).toBe(true);
});