import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { expect, test } from "vitest";
import { applyPromotionArtifact } from "../../src/core/promotion-queue.js";
import { writePromotionArtifact } from "../../src/core/promotion-artifacts.js";
import type { PromotionArtifact, TaskLedger } from "../../src/core/types.js";

const execFileAsync = promisify(execFile);

function makeProjectConfig(): string {
  return JSON.stringify({
    version: 1,
    project: { alias: "demo", repoRoot: "/tmp", initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "echo lint", testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  }, null, 2);
}

test("auto-merge rejects when base branch has drifted", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-drift-"));
  await fs.mkdir(path.join(dir, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: dir });
  await execFileAsync("git", ["config", "user.email", "test@test.com"], { cwd: dir });
  await execFileAsync("git", ["config", "user.name", "Test"], { cwd: dir });
  await fs.writeFile(path.join(dir, "README.md"), "init\n", "utf8");
  await fs.writeFile(path.join(dir, ".openloop", "project.json"), `${makeProjectConfig()}\n`, "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "initial"], { cwd: dir });

  const mainBranch = (await execFileAsync("git", ["branch", "--show-current"], { cwd: dir })).stdout.trim();

  // Create the task branch with a commit
  await execFileAsync("git", ["checkout", "-b", "openloop/task-drift"], { cwd: dir });
  await fs.writeFile(path.join(dir, "feature.txt"), "feature work\n", "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "task work"], { cwd: dir });

  // Go back to main and add a diverging commit
  await execFileAsync("git", ["checkout", mainBranch], { cwd: dir });
  await fs.writeFile(path.join(dir, "other.txt"), "other work\n", "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "main diverged"], { cwd: dir });

  // Go back to task branch for promotion
  await execFileAsync("git", ["checkout", "openloop/task-drift"], { cwd: dir });

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-drift",
    baseBranch: mainBranch,
    decision: "auto-merge-eligible",
    action: "queue-auto-merge",
    effectivePromotionMode: "auto-merge",
    validation: [{ name: "lint", command: "echo lint", exitCode: 0 }],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
  const artifactPath = await writePromotionArtifact(dir, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "task-drift",
      title: "Drift test",
      kind: "feature",
      status: "done",
      risk: "low-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: "openloop/task-drift",
      owner: "openloop",
      acceptanceCriteria: ["done"],
      attempts: 1,
      lastFailureSignature: null,
      promotion: "auto-merge",
      notes: [],
      lastRun: {
        completedAt: new Date().toISOString(),
        mode: "implement",
        piExitCode: 0,
        outcome: "completed",
        baseBranch: mainBranch,
        validation: [{ name: "lint", command: "echo lint", exitCode: 0 }],
        promotionDecision: "auto-merge-eligible",
        effectivePromotionMode: "auto-merge",
        promotionAction: "queue-auto-merge",
        promotionArtifactPath: artifactPath,
        promotionArtifactState: "pending",
        promotionResultArtifactPath: null,
      },
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  };
  await fs.writeFile(path.join(dir, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "add promotion artifacts"], { cwd: dir });

  await expect(applyPromotionArtifact(dir, "task-drift")).rejects.toThrow("drifted");
});

test("auto-merge rejects when task risk is not low-risk", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-risk-reject-"));
  await fs.mkdir(path.join(dir, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: dir });
  await execFileAsync("git", ["config", "user.email", "test@test.com"], { cwd: dir });
  await execFileAsync("git", ["config", "user.name", "Test"], { cwd: dir });
  await fs.writeFile(path.join(dir, "README.md"), "init\n", "utf8");
  await fs.writeFile(path.join(dir, ".openloop", "project.json"), `${makeProjectConfig()}\n`, "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "initial"], { cwd: dir });

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-medrisk",
    baseBranch: null,
    decision: "auto-merge-eligible",
    action: "queue-auto-merge",
    effectivePromotionMode: "auto-merge",
    validation: [{ name: "lint", command: "echo lint", exitCode: 0 }],
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
  const artifactPath = await writePromotionArtifact(dir, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "task-medrisk",
      title: "Medium risk auto-merge attempt",
      kind: "feature",
      status: "done",
      risk: "medium-risk",
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: "openloop",
      acceptanceCriteria: ["done"],
      attempts: 1,
      lastFailureSignature: null,
      promotion: "auto-merge",
      notes: [],
      lastRun: {
        completedAt: new Date().toISOString(),
        mode: "implement",
        piExitCode: 0,
        outcome: "completed",
        baseBranch: null,
        validation: [{ name: "lint", command: "echo lint", exitCode: 0 }],
        promotionDecision: "auto-merge-eligible",
        effectivePromotionMode: "auto-merge",
        promotionAction: "queue-auto-merge",
        promotionArtifactPath: artifactPath,
        promotionArtifactState: "pending",
        promotionResultArtifactPath: null,
      },
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }],
  };
  await fs.writeFile(path.join(dir, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "add promotion artifacts"], { cwd: dir });

  await expect(applyPromotionArtifact(dir, "task-medrisk")).rejects.toThrow("low-risk");
});

test("auto-merge rejects when no validation commands are configured", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-no-validation-"));
  await fs.mkdir(path.join(dir, ".openloop"), { recursive: true });
  await execFileAsync("git", ["init"], { cwd: dir });
  await execFileAsync("git", ["config", "user.email", "test@test.com"], { cwd: dir });
  await execFileAsync("git", ["config", "user.name", "Test"], { cwd: dir });
  await fs.writeFile(path.join(dir, "README.md"), "init\n", "utf8");

  // No validation commands
  const noValidationConfig = JSON.stringify({
    version: 1,
    project: { alias: "demo", repoRoot: "/tmp", initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  }, null, 2);
  await fs.writeFile(path.join(dir, ".openloop", "project.json"), `${noValidationConfig}\n`, "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "initial"], { cwd: dir });

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId: "task-noval",
    baseBranch: null,
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
  const artifactPath = await writePromotionArtifact(dir, artifact);

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [{
      id: "task-noval",
      title: "No validation auto-merge attempt",
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
      notes: [],
      lastRun: {
        completedAt: new Date().toISOString(),
        mode: "implement",
        piExitCode: 0,
        outcome: "completed",
        baseBranch: null,
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
    }],
  };
  await fs.writeFile(path.join(dir, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "add promotion artifacts"], { cwd: dir });

  await expect(applyPromotionArtifact(dir, "task-noval")).rejects.toThrow("validation command");
});
