import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { expect, test } from "vitest";
import { applyPromotionArtifact, getPromotionDetail, getPromotionHistory, listPromotionArtifacts, listPromotionArtifactsForTask, updatePromotionArtifact } from "../../src/core/promotion-queue.js";
import { writePromotionArtifact } from "../../src/core/promotion-artifacts.js";
import type { PromotionArtifact, TaskLedger } from "../../src/core/types.js";

const execFileAsync = promisify(execFile);

test("promotion queue lists and applies pending artifacts", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-queue-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-1",
    baseBranch: null,
    decision: "manual-review",
    action: "queue-review",
    effectivePromotionMode: "pull-request",
    validation: [],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
  const artifactPath = await writePromotionArtifact(projectRoot, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-1",
        title: "Pending promotion",
        kind: "feature",
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
        notes: [],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 0,
          outcome: "completed",
          baseBranch: null,
          validation: [],
          promotionDecision: "manual-review",
          effectivePromotionMode: "pull-request",
          promotionAction: "queue-review",
          promotionArtifactPath: artifactPath,
          promotionArtifactState: "pending",
          promotionResultArtifactPath: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const listed = await listPromotionArtifacts(projectRoot);
  expect(listed).toHaveLength(1);
  expect(listed[0]?.artifact.status).toBe("pending");

  const updated = await updatePromotionArtifact(projectRoot, "task-1", "applied", "review completed");
  expect(updated.artifact.status).toBe("applied");
  expect(updated.artifact.note).toBe("review completed");

  const persistedArtifact = JSON.parse(await fs.readFile(artifactPath, "utf8")) as PromotionArtifact;
  expect(persistedArtifact.status).toBe("applied");

  const persistedLedger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persistedLedger.tasks[0]?.lastRun?.promotionArtifactState).toBe("applied");
  expect(persistedLedger.tasks[0]?.lastRun?.promotionResultArtifactPath).toBeTruthy();
  expect(persistedLedger.tasks[0]?.notes?.at(-1)).toContain("review completed");

  const resultArtifact = JSON.parse(
    await fs.readFile(persistedLedger.tasks[0]?.lastRun?.promotionResultArtifactPath as string, "utf8"),
  ) as { result: string; sourcePromotionAction: string };
  expect(resultArtifact.result).toBe("applied");
  expect(resultArtifact.sourcePromotionAction).toBe("queue-review");
});

test("applyPromotionArtifact prepares a review branch for queue-review actions", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-apply-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.email", "test@example.com"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.name", "Openloop Test"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello\n", "utf8");
  await execFileAsync("git", ["add", "README.md"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "init"], { cwd: projectRoot });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    `${JSON.stringify(
      {
        version: 1,
        project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
        pi: { model: null, promptFiles: [] },
        runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
        validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
        risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-review",
    baseBranch: "master",
    decision: "manual-review",
    action: "queue-review",
    effectivePromotionMode: "pull-request",
    validation: [],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
  const artifactPath = await writePromotionArtifact(projectRoot, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-review",
        title: "Prepare review branch",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["branch exists"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 0,
          outcome: "completed",
          baseBranch: "master",
          validation: [],
          promotionDecision: "manual-review",
          effectivePromotionMode: "pull-request",
          promotionAction: "queue-review",
          promotionArtifactPath: artifactPath,
          promotionArtifactState: "pending",
          promotionResultArtifactPath: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await execFileAsync("git", ["add", ".openloop"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "control plane"], { cwd: projectRoot });

  const applied = await applyPromotionArtifact(projectRoot, "task-review");
  expect(applied.artifact.status).toBe("applied");
  expect(applied.artifact.note).toContain("openloop/task-review");

  const currentBranch = await execFileAsync("git", ["branch", "--show-current"], { cwd: projectRoot });
  expect(currentBranch.stdout.trim()).toBe("openloop/task-review");

  const persistedLedger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persistedLedger.tasks[0]?.branch).toBe("openloop/task-review");
  expect(persistedLedger.tasks[0]?.lastRun?.promotionArtifactState).toBe("applied");
  expect(persistedLedger.tasks[0]?.lastRun?.promotionResultArtifactPath).toBeTruthy();
  expect(persistedLedger.tasks[0]?.status).toBe("done");
  expect(persistedLedger.tasks[0]?.promotedAt ?? null).toBeNull();
});

test("applyPromotionArtifact rejects promotion apply on a dirty git tree", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-dirty-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.email", "test@example.com"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.name", "Openloop Test"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello\n", "utf8");
  await execFileAsync("git", ["add", "README.md"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "init"], { cwd: projectRoot });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    `${JSON.stringify(
      {
        version: 1,
        project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
        pi: { model: null, promptFiles: [] },
        runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
        validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
        risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  const artifact = await writePromotionArtifact(projectRoot, {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-dirty",
    baseBranch: "master",
    decision: "manual-review",
    action: "queue-review",
    effectivePromotionMode: "pull-request",
    validation: [],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  });

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-dirty",
        title: "Dirty promotion",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["blocked"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 0,
          outcome: "completed",
          baseBranch: "master",
          validation: [],
          promotionDecision: "manual-review",
          effectivePromotionMode: "pull-request",
          promotionAction: "queue-review",
          promotionArtifactPath: artifact,
          promotionArtifactState: "pending",
          promotionResultArtifactPath: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await execFileAsync("git", ["add", ".openloop"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "control plane"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "dirty\n", "utf8");

  await expect(applyPromotionArtifact(projectRoot, "task-dirty")).rejects.toThrow("Cannot apply promotion on a dirty git tree.");
});

test("getPromotionDetail returns artifact, branch, and git working tree state", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-detail-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.email", "test@example.com"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.name", "Openloop Test"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello\n", "utf8");
  await execFileAsync("git", ["add", "README.md"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "init"], { cwd: projectRoot });

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-detail",
    baseBranch: "master",
    decision: "manual-review",
    action: "queue-review",
    effectivePromotionMode: "pull-request",
    validation: [],
    piExitCode: 0,
    outcome: "completed",
    status: "applied",
    processedAt: new Date().toISOString(),
    note: "Prepared review branch openloop/task-detail",
  };
  const artifactPath = await writePromotionArtifact(projectRoot, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-detail",
        title: "Inspect review branch",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: "openloop/task-detail",
        owner: "openloop",
        acceptanceCriteria: ["detail visible"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 0,
          outcome: "completed",
          baseBranch: "master",
          validation: [],
          promotionDecision: "manual-review",
          effectivePromotionMode: "pull-request",
          promotionAction: "queue-review",
          promotionArtifactPath: artifactPath,
          promotionArtifactState: "applied",
          promotionResultArtifactPath: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  await execFileAsync("git", ["checkout", "-b", "openloop/task-detail"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello detail\n", "utf8");

  const detail = await getPromotionDetail(projectRoot, "task-detail");
  expect(detail.artifact.taskId).toBe("task-detail");
  expect(detail.resultArtifact).toBeNull();
  expect(detail.taskBranch).toBe("openloop/task-detail");
  expect(detail.currentBranch).toBe("openloop/task-detail");
  expect(detail.dirty).toBe(true);
});

test("applyPromotionArtifact fast-forwards queue-auto-merge changes into the base branch", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-automerge-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.email", "test@example.com"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.name", "Openloop Test"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello\n", "utf8");
  await execFileAsync("git", ["add", "README.md"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "init"], { cwd: projectRoot });
  const currentBranch = await execFileAsync("git", ["branch", "--show-current"], { cwd: projectRoot });
  const baseBranch = currentBranch.stdout.trim();
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    `${JSON.stringify(
      {
        version: 1,
        project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
        pi: { model: null, promptFiles: [] },
        runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
        validation: { lintCommand: null, testCommand: "bun test", typecheckCommand: null },
        risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
      },
      null,
      2,
    )}\n`,
    "utf8",
  );
  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-auto",
    baseBranch,
    decision: "auto-merge-eligible",
    action: "queue-auto-merge",
    effectivePromotionMode: "auto-merge",
    validation: [{ name: "test", command: "bun test", exitCode: 0 }],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
  const artifactPath = await writePromotionArtifact(projectRoot, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-auto",
        title: "Auto merge task",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: "openloop/task-auto",
        owner: "openloop",
        acceptanceCriteria: ["merged"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 0,
          outcome: "completed",
          baseBranch,
          validation: [{ name: "test", command: "bun test", exitCode: 0 }],
          promotionDecision: "auto-merge-eligible",
          effectivePromotionMode: "auto-merge",
          promotionAction: "queue-auto-merge",
          promotionArtifactPath: artifactPath,
          promotionArtifactState: "pending",
          promotionResultArtifactPath: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await execFileAsync("git", ["add", ".openloop"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "control plane"], { cwd: projectRoot });
  await execFileAsync("git", ["checkout", "-b", "openloop/task-auto"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello automerge\n", "utf8");
  await execFileAsync("git", ["add", "README.md"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "openloop: complete task-auto"], { cwd: projectRoot });
  await execFileAsync("git", ["checkout", baseBranch], { cwd: projectRoot });

  const applied = await applyPromotionArtifact(projectRoot, "task-auto");
  expect(applied.artifact.status).toBe("applied");
  expect(applied.artifact.note).toContain(`Merged openloop/task-auto into ${baseBranch}`);

  const branchAfter = await execFileAsync("git", ["branch", "--show-current"], { cwd: projectRoot });
  expect(branchAfter.stdout.trim()).toBe(baseBranch);
  const log = await execFileAsync("git", ["log", "--format=%s", "-1"], { cwd: projectRoot });
  expect(log.stdout.trim()).toBe("openloop: complete task-auto");

  const persistedLedger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persistedLedger.tasks[0]?.branch).toBe("openloop/task-auto");
  expect(persistedLedger.tasks[0]?.lastRun?.promotionArtifactState).toBe("applied");
  expect(persistedLedger.tasks[0]?.lastRun?.promotionResultArtifactPath).toBeTruthy();
  expect(persistedLedger.tasks[0]?.status).toBe("promoted");
  expect(persistedLedger.tasks[0]?.promotedAt).toBeTruthy();

  const detail = await getPromotionDetail(projectRoot, "task-auto");
  expect(detail.resultArtifact?.result).toBe("applied");
  expect(detail.resultArtifact?.sourcePromotionAction).toBe("queue-auto-merge");
  expect(detail.resultArtifactPath).toBeTruthy();
});

test("applyPromotionArtifact rejects auto-merge when no validations were persisted", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-no-validation-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.email", "test@example.com"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.name", "Openloop Test"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello\n", "utf8");
  await execFileAsync("git", ["add", "README.md"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "init"], { cwd: projectRoot });
  const currentBranch = await execFileAsync("git", ["branch", "--show-current"], { cwd: projectRoot });
  const baseBranch = currentBranch.stdout.trim();
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    `${JSON.stringify({
      version: 1,
      project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
      pi: { model: null, promptFiles: [] },
      runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
      validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
      risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    }, null, 2)}\n`,
    "utf8",
  );
  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-no-validation",
    baseBranch,
    decision: "auto-merge-eligible",
    action: "queue-auto-merge",
    effectivePromotionMode: "auto-merge",
    validation: [],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
  const artifactPath = await writePromotionArtifact(projectRoot, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-no-validation",
        title: "Auto merge without validation",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["blocked"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 0,
          outcome: "completed",
          baseBranch,
          validation: [],
          promotionDecision: "auto-merge-eligible",
          effectivePromotionMode: "auto-merge",
          promotionAction: "queue-auto-merge",
          promotionArtifactPath: artifactPath,
          promotionArtifactState: "pending",
          promotionResultArtifactPath: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await execFileAsync("git", ["add", ".openloop"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "control plane"], { cwd: projectRoot });

  await expect(applyPromotionArtifact(projectRoot, "task-no-validation")).rejects.toThrow("Auto-merge requires at least one configured validation command.");
});

test("applyPromotionArtifact rejects auto-merge when base branch drifted", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-drift-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.email", "test@example.com"], { cwd: projectRoot });
  await execFileAsync("git", ["config", "user.name", "Openloop Test"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello\n", "utf8");
  await execFileAsync("git", ["add", "README.md"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "init"], { cwd: projectRoot });
  const currentBranch = await execFileAsync("git", ["branch", "--show-current"], { cwd: projectRoot });
  const baseBranch = currentBranch.stdout.trim();
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    `${JSON.stringify({
      version: 1,
      project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
      pi: { model: null, promptFiles: [] },
      runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
      validation: { lintCommand: null, testCommand: "bun test", typecheckCommand: null },
      risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    }, null, 2)}\n`,
    "utf8",
  );

  await execFileAsync("git", ["checkout", "-b", "openloop/task-drift"], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "README.md"), "hello branch\n", "utf8");
  await execFileAsync("git", ["add", "README.md"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "branch work"], { cwd: projectRoot });
  await execFileAsync("git", ["checkout", baseBranch], { cwd: projectRoot });
  await fs.writeFile(path.join(projectRoot, "BASE.txt"), "base drift\n", "utf8");
  await execFileAsync("git", ["add", "BASE.txt"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "base drift"], { cwd: projectRoot });

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-drift",
    baseBranch,
    decision: "auto-merge-eligible",
    action: "queue-auto-merge",
    effectivePromotionMode: "auto-merge",
    validation: [{ name: "test", command: "bun test", exitCode: 0 }],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
  const artifactPath = await writePromotionArtifact(projectRoot, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-drift",
        title: "Auto merge with drift",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: "openloop/task-drift",
        owner: "openloop",
        acceptanceCriteria: ["blocked"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 0,
          outcome: "completed",
          baseBranch,
          validation: [{ name: "test", command: "bun test", exitCode: 0 }],
          promotionDecision: "auto-merge-eligible",
          effectivePromotionMode: "auto-merge",
          promotionAction: "queue-auto-merge",
          promotionArtifactPath: artifactPath,
          promotionArtifactState: "pending",
          promotionResultArtifactPath: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await execFileAsync("git", ["add", ".openloop"], { cwd: projectRoot });
  await execFileAsync("git", ["commit", "-m", "control plane"], { cwd: projectRoot });

  await expect(applyPromotionArtifact(projectRoot, "task-drift")).rejects.toThrow(`Base branch has drifted since task branch split from ${baseBranch}.`);
});

test("getPromotionHistory returns promotion request and result artifacts in order", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-history-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: "2026-03-09T10:00:00.000Z",
    projectAlias: "demo",
    taskId: "task-history",
    baseBranch: null,
    decision: "manual-review",
    action: "queue-review",
    effectivePromotionMode: "pull-request",
    validation: [],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
  const artifactPath = await writePromotionArtifact(projectRoot, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "task-history",
        title: "Promotion history",
        kind: "feature",
        status: "done",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: "openloop",
        acceptanceCriteria: ["history visible"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "pull-request",
        notes: [],
        lastRun: {
          completedAt: new Date().toISOString(),
          mode: "implement",
          piExitCode: 0,
          outcome: "completed",
          baseBranch: null,
          validation: [],
          promotionDecision: "manual-review",
          effectivePromotionMode: "pull-request",
          promotionAction: "queue-review",
          promotionArtifactPath: artifactPath,
          promotionArtifactState: "pending",
          promotionResultArtifactPath: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  await updatePromotionArtifact(projectRoot, "task-history", "rejected", "not now");

  const history = await getPromotionHistory(projectRoot, "task-history");
  expect(history).toHaveLength(2);
  expect(history[0]?.kind).toBe("request");
  expect(history[1]?.kind).toBe("result");
  expect((history[1]?.payload as { result: string }).result).toBe("rejected");
});

test("listPromotionArtifactsForTask filters promotion requests by task id", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-promotion-filter-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  await writePromotionArtifact(projectRoot, {
    version: 1,
    createdAt: "2026-03-09T10:00:00.000Z",
    projectAlias: "demo",
    taskId: "task-a",
    baseBranch: null,
    decision: "manual-review",
    action: "queue-review",
    effectivePromotionMode: "pull-request",
    validation: [],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  });
  await writePromotionArtifact(projectRoot, {
    version: 1,
    createdAt: "2026-03-09T10:01:00.000Z",
    projectAlias: "demo",
    taskId: "task-b",
    baseBranch: null,
    decision: "manual-review",
    action: "queue-review",
    effectivePromotionMode: "pull-request",
    validation: [],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  });

  const filtered = await listPromotionArtifactsForTask(projectRoot, "task-b");
  expect(filtered).toHaveLength(1);
  expect(filtered[0]?.artifact.taskId).toBe("task-b");
});