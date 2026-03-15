import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { runProjectIteration } from "../../src/core/scheduler.js";
import type { LinkedProject, ProjectConfig, TaskLedger } from "../../src/core/types.js";

test("runProjectIteration marks implement task done when validations pass", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-validate-pass-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const config: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "lint", testCommand: "test", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(config, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    [
      "version: 1",
      "promotion:",
      "  lowRiskMode: auto-merge",
      "  mediumRiskMode: pull-request",
      "  highRiskMode: pull-request",
      "riskClasses:",
      "  low-risk:",
      "    autoMergeAllowed: true",
      "    requiresHumanReview: false",
      "  medium-risk:",
      "    autoMergeAllowed: false",
      "    requiresHumanReview: true",
      "  high-risk:",
      "    autoMergeAllowed: false",
      "    requiresHumanReview: true",
    ].join("\n") + "\n",
    "utf8",
  );

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "impl-pass",
        title: "Implement with passing validation",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["It works"],
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

  expect(result.mode).toBe("implement");
  expect(result.validation).toHaveLength(2);
  expect(result.promotionDecision).toBe("auto-merge-eligible");
  expect(result.promotionAction).toBe("queue-auto-merge");
  expect(result.promotionArtifactPath).toBeTruthy();

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("done");
  expect(persisted.tasks[0]?.lastRun?.outcome).toBe("completed");
  expect(persisted.tasks[0]?.lastRun?.effectivePromotionMode).toBe("auto-merge");
  expect(persisted.tasks[0]?.lastRun?.promotionAction).toBe("queue-auto-merge");

  const promotionArtifact = JSON.parse(await fs.readFile(result.promotionArtifactPath as string, "utf8")) as { action: string; decision: string };
  expect(promotionArtifact.action).toBe("queue-auto-merge");
  expect(promotionArtifact.decision).toBe("auto-merge-eligible");
});

test("runProjectIteration marks implement task failed when validations fail", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-validate-fail-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const config: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "lint", testCommand: "test", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(config, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    [
      "version: 1",
      "promotion:",
      "  lowRiskMode: auto-merge",
      "  mediumRiskMode: pull-request",
      "  highRiskMode: pull-request",
      "riskClasses:",
      "  low-risk:",
      "    autoMergeAllowed: true",
      "    requiresHumanReview: false",
      "  medium-risk:",
      "    autoMergeAllowed: false",
      "    requiresHumanReview: true",
      "  high-risk:",
      "    autoMergeAllowed: false",
      "    requiresHumanReview: true",
    ].join("\n") + "\n",
    "utf8",
  );

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "impl-fail",
        title: "Implement with failing validation",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["It works"],
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

  let runCount = 0;
  const result = await runProjectIteration(project, {
    piRunner: async () => 0,
    validationRunner: async () => {
      runCount += 1;
      return runCount === 1 ? 1 : 0;
    },
  });

  expect(result.validation).toHaveLength(1);
  expect(result.promotionDecision).toBe("blocked");
  expect(result.promotionAction).toBe("block");
  expect(result.promotionArtifactPath).toBeTruthy();

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("failed");
  expect(persisted.tasks[0]?.attempts).toBe(1);
  expect(persisted.tasks[0]?.lastRun?.outcome).toBe("validation-failed");
  expect(persisted.tasks[0]?.lastRun?.promotionDecision).toBe("blocked");
  expect(persisted.tasks[0]?.lastRun?.promotionAction).toBe("block");
});

test("runProjectIteration downgrades promotion when policy disallows auto-merge", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-policy-promotion-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const config: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "lint", testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(config, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    [
      "version: 1",
      "promotion:",
      "  lowRiskMode: pull-request",
      "  mediumRiskMode: pull-request",
      "  highRiskMode: pull-request",
      "riskClasses:",
      "  low-risk:",
      "    autoMergeAllowed: true",
      "    requiresHumanReview: false",
      "  medium-risk:",
      "    autoMergeAllowed: false",
      "    requiresHumanReview: true",
      "  high-risk:",
      "    autoMergeAllowed: false",
      "    requiresHumanReview: true",
    ].join("\n") + "\n",
    "utf8",
  );

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "impl-medium",
        title: "Implement low risk change with stricter policy",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["It works"],
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

  expect(result.promotionDecision).toBe("manual-review");
  expect(result.promotionAction).toBe("queue-review");
  expect(result.promotionArtifactPath).toBeTruthy();

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.lastRun?.effectivePromotionMode).toBe("pull-request");
  expect(persisted.tasks[0]?.lastRun?.promotionAction).toBe("queue-review");
});

test("runProjectIteration downgrades auto-merge when no validation commands are configured", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-policy-no-validations-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const config: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(config, null, 2)}\n`, "utf8");
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    [
      "version: 1",
      "promotion:",
      "  lowRiskMode: auto-merge",
      "  mediumRiskMode: pull-request",
      "  highRiskMode: pull-request",
      "riskClasses:",
      "  low-risk:",
      "    autoMergeAllowed: true",
      "    requiresHumanReview: false",
      "  medium-risk:",
      "    autoMergeAllowed: false",
      "    requiresHumanReview: true",
      "  high-risk:",
      "    autoMergeAllowed: false",
      "    requiresHumanReview: true",
    ].join("\n") + "\n",
    "utf8",
  );

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "impl-no-configured-validation",
        title: "Implement low risk change without configured validations",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["It works"],
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
  });

  expect(result.promotionDecision).toBe("manual-review");
  expect(result.promotionAction).toBe("queue-review");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("done");
  expect(persisted.tasks[0]?.lastRun?.effectivePromotionMode).toBe("auto-merge");
  expect(persisted.tasks[0]?.lastRun?.promotionDecision).toBe("manual-review");
});

test("runProjectIteration stops with timeout when Pi exceeds the configured run timeout", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-timeout-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const config: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(config, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "impl-timeout",
        title: "Implement with timeout",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["It works"],
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

  await expect(runProjectIteration(project, {
    timeoutMs: 10,
    piRunner: async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
      return 0;
    },
  })).rejects.toThrow("timeout");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.attempts).toBe(1);
  expect(persisted.tasks[0]?.lastFailureSignature).toBe("timeout");
  expect(persisted.tasks[0]?.lastRun?.outcome).toBe("error");
});

test("runProjectIteration blocks repeated no-progress failures", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-no-progress-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const config: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(config, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "impl-no-progress",
        title: "Implement with repeated failures",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["It works"],
        attempts: 1,
        lastFailureSignature: "pi-exit-1",
        promotion: "auto-merge",
        notes: ["openloop:no-progress:failure-pi-exit-1:1"],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 1,
          outcome: "pi-failed",
          baseBranch: null,
          validation: [],
          promotionDecision: "none",
          effectivePromotionMode: "auto-merge",
          promotionAction: "none",
          promotionArtifactPath: null,
          promotionArtifactState: "pending",
          promotionResultArtifactPath: null,
        },
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
    noProgressRepeatLimit: 2,
    piRunner: async () => 1,
  });

  expect(result.stoppedBy).toBe("no-progress");
  expect(result.taskStatus).toBe("blocked");

  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.status).toBe("blocked");
  expect(persisted.tasks[0]?.notes?.some((note) => note.includes("openloop:no-progress:failure-pi-exit-1:2"))).toBe(true);
});