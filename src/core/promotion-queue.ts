import fs from "node:fs/promises";
import path from "node:path";
import { fileExists, readJsonFile, writeJsonFile } from "./fs.js";
import { checkoutBranch, checkoutHandoffBranch, ensureCleanGitRepo, getBranchHead, getGitWorkingTreeState, getMergeBase, mergeFastForward } from "./git.js";
import { loadProjectConfig } from "./project-config.js";
import { listPromotionResultArtifacts, readPromotionResultArtifact, writePromotionResultArtifact } from "./promotion-artifacts.js";
import { loadTaskLedger, saveTaskLedger } from "./task-ledger.js";
import type { PromotionArtifact, PromotionArtifactStatus, ProjectTask, PromotionResultArtifact } from "./types.js";
import { getConfiguredValidationNames } from "./validation-utils.js";

export interface PromotionQueueItem {
  artifactPath: string;
  artifact: PromotionArtifact;
}

export interface PromotionDetail {
  artifactPath: string;
  artifact: PromotionArtifact;
  resultArtifactPath: string | null;
  resultArtifact: PromotionResultArtifact | null;
  taskBranch: string | null;
  currentBranch: string | null;
  dirty: boolean;
}

export interface PromotionHistoryEntry {
  createdAt: string;
  kind: "request" | "result";
  artifactPath: string;
  taskId: string;
  payload: PromotionArtifact | PromotionResultArtifact;
}

export async function listPromotionArtifacts(projectPath: string): Promise<PromotionQueueItem[]> {
  const promotionsDir = path.join(projectPath, ".openloop", "promotions");
  if (!(await fileExists(promotionsDir))) {
    return [];
  }

  const entries = await fs.readdir(promotionsDir, { withFileTypes: true });
  const items: PromotionQueueItem[] = [];
  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.endsWith(".json")) {
      continue;
    }
    const artifactPath = path.join(promotionsDir, entry.name);
    const artifact = await readJsonFile<PromotionArtifact>(artifactPath, {
      version: 1,
      createdAt: new Date(0).toISOString(),
      projectAlias: "",
      taskId: "",
      baseBranch: null,
      decision: "none",
      action: "none",
      effectivePromotionMode: "manual-only",
      validation: [],
      piExitCode: null,
      outcome: "error",
      status: "pending",
      processedAt: null,
      note: null,
    });
    items.push({ artifactPath, artifact });
  }

  return items.sort((left, right) => right.artifact.createdAt.localeCompare(left.artifact.createdAt));
}

export async function listPromotionArtifactsForTask(projectPath: string, taskId: string): Promise<PromotionQueueItem[]> {
  return (await listPromotionArtifacts(projectPath)).filter((item) => item.artifact.taskId === taskId);
}

export async function updatePromotionArtifact(
  projectPath: string,
  taskId: string,
  status: PromotionArtifactStatus,
  note?: string,
): Promise<PromotionQueueItem> {
  if (status === "pending") {
    throw new Error("Cannot update a promotion artifact back to pending.");
  }

  const items = await listPromotionArtifacts(projectPath);
  const match = items.find((item) => item.artifact.taskId === taskId && item.artifact.status === "pending");
  if (!match) {
    throw new Error(`No pending promotion artifact found for task: ${taskId}`);
  }

  match.artifact.status = status;
  match.artifact.processedAt = new Date().toISOString();
  match.artifact.note = note ?? null;
  await writeJsonFile(match.artifactPath, match.artifact);
  const resultArtifactPath = await writePromotionResult(projectPath, match, status, null, note ?? null);
  await syncTaskPromotionState(projectPath, taskId, match.artifactPath, status, note ?? null, undefined, resultArtifactPath);
  return match;
}

export async function applyPromotionArtifact(projectPath: string, taskId: string, note?: string): Promise<PromotionQueueItem> {
  await ensurePromotionWorkspaceClean(projectPath);
  const items = await listPromotionArtifacts(projectPath);
  const match = items.find((item) => item.artifact.taskId === taskId && item.artifact.status === "pending");
  if (!match) {
    throw new Error(`No pending promotion artifact found for task: ${taskId}`);
  }
  const ledger = await loadTaskLedger(projectPath);
  const task = ledger.tasks.find((candidate) => candidate.id === taskId);
  if (!task) {
    throw new Error(`No task found for promotion artifact: ${taskId}`);
  }

  if (match.artifact.action === "queue-review") {
    const branchName = await prepareReviewBranch(projectPath, taskId);
    match.artifact.status = "applied";
    match.artifact.processedAt = new Date().toISOString();
    match.artifact.note = note ?? `Prepared review branch ${branchName}`;
    await writeJsonFile(match.artifactPath, match.artifact);
    const resultArtifactPath = await writePromotionResult(projectPath, match, "applied", branchName, match.artifact.note);
    await syncTaskPromotionState(projectPath, taskId, match.artifactPath, "applied", match.artifact.note, branchName, resultArtifactPath);
    return match;
  }

  if (match.artifact.action !== "queue-auto-merge") {
    throw new Error(`Promotion action not yet supported for local apply: ${match.artifact.action}`);
  }

  await assertAutoMergeReady(projectPath, task);
  const branchName = await applyAutoMergePromotion(projectPath, taskId, match.artifact.baseBranch);
  match.artifact.status = "applied";
  match.artifact.processedAt = new Date().toISOString();
  match.artifact.note = note ?? `Merged ${branchName} into ${match.artifact.baseBranch ?? "current branch"}`;
  await writeJsonFile(match.artifactPath, match.artifact);
  const resultArtifactPath = await writePromotionResult(projectPath, match, "applied", branchName, match.artifact.note);
  await syncTaskPromotionState(projectPath, taskId, match.artifactPath, "applied", match.artifact.note, branchName, resultArtifactPath);
  return match;
}

export async function dryRunPromotionApply(projectPath: string, taskId: string): Promise<{
  wouldApply: boolean;
  action: string;
  reason: string;
}> {
  const gitState = await getGitWorkingTreeState(projectPath);
  if (gitState.dirty) {
    return { wouldApply: false, action: "none", reason: "Git working tree is dirty." };
  }
  const items = await listPromotionArtifacts(projectPath);
  const match = items.find((item) => item.artifact.taskId === taskId && item.artifact.status === "pending");
  if (!match) {
    return { wouldApply: false, action: "none", reason: `No pending promotion artifact found for task: ${taskId}` };
  }
  const ledger = await loadTaskLedger(projectPath);
  const task = ledger.tasks.find((candidate) => candidate.id === taskId);
  if (!task) {
    return { wouldApply: false, action: "none", reason: `No task found for promotion artifact: ${taskId}` };
  }

  if (match.artifact.action === "queue-review") {
    const config = await loadProjectConfig(projectPath);
    const branchName = `${config.runtime.branchPrefix}${taskId}`;
    return { wouldApply: true, action: "queue-review", reason: `Would prepare review branch ${branchName}` };
  }

  if (match.artifact.action === "queue-auto-merge") {
    try {
      await assertAutoMergeReady(projectPath, task);
      return { wouldApply: true, action: "queue-auto-merge", reason: `Would auto-merge task branch into ${match.artifact.baseBranch ?? "current branch"}` };
    } catch (error) {
      return { wouldApply: false, action: "queue-auto-merge", reason: error instanceof Error ? error.message : String(error) };
    }
  }

  return { wouldApply: false, action: match.artifact.action, reason: `Promotion action not yet supported: ${match.artifact.action}` };
}

export async function getPromotionDetail(projectPath: string, taskId: string): Promise<PromotionDetail> {
  const items = await listPromotionArtifactsForTask(projectPath, taskId);
  const match = items[0];
  if (!match) {
    throw new Error(`No promotion artifact found for task: ${taskId}`);
  }

  const ledger = await loadTaskLedger(projectPath);
  const task = ledger.tasks.find((candidate) => candidate.id === taskId);
  const gitState = await getGitWorkingTreeState(projectPath);
  const resultArtifactPath = task?.lastRun?.promotionResultArtifactPath ?? null;
  const resultArtifact = resultArtifactPath ? await readPromotionResultArtifact(resultArtifactPath) : null;
  return {
    artifactPath: match.artifactPath,
    artifact: match.artifact,
    resultArtifactPath,
    resultArtifact,
    taskBranch: task?.branch ?? null,
    currentBranch: gitState.currentBranch,
    dirty: gitState.dirty,
  };
}

export async function getPromotionHistory(projectPath: string, taskId: string): Promise<PromotionHistoryEntry[]> {
  const requests = (await listPromotionArtifacts(projectPath))
    .filter((item) => item.artifact.taskId === taskId)
    .map((item) => ({
      createdAt: item.artifact.createdAt,
      kind: "request" as const,
      artifactPath: item.artifactPath,
      taskId,
      payload: item.artifact,
    }));

  const results = (await listPromotionResultArtifacts(projectPath, taskId)).map((item) => ({
    createdAt: item.artifact.createdAt,
    kind: "result" as const,
    artifactPath: item.artifactPath,
    taskId,
    payload: item.artifact,
  }));

  return [...requests, ...results].sort((left, right) => left.createdAt.localeCompare(right.createdAt));
}

async function prepareReviewBranch(projectPath: string, taskId: string): Promise<string> {
  const config = await loadProjectConfig(projectPath);
  const branchName = `${config.runtime.branchPrefix}${taskId}`;
  await checkoutHandoffBranch(projectPath, branchName);
  return branchName;
}

async function applyAutoMergePromotion(projectPath: string, taskId: string, baseBranch: string | null): Promise<string> {
  const config = await loadProjectConfig(projectPath);
  const branchName = `${config.runtime.branchPrefix}${taskId}`;
  const initialState = await getGitWorkingTreeState(projectPath);
  const mergeBaseBranch = baseBranch ?? initialState.currentBranch;
  if (!mergeBaseBranch) {
    throw new Error("Cannot determine base branch for auto-merge promotion.");
  }

  await checkoutHandoffBranch(projectPath, branchName);
  await ensureCleanGitRepo(projectPath);

  const baseHeadBeforeMerge = await getBranchHead(projectPath, mergeBaseBranch);
  const mergeBase = await getMergeBase(projectPath, mergeBaseBranch, branchName);
  if (mergeBase !== baseHeadBeforeMerge) {
    throw new Error(`Base branch has drifted since task branch split from ${mergeBaseBranch}.`);
  }

  await checkoutBranch(projectPath, mergeBaseBranch);
  await mergeFastForward(projectPath, branchName);
  return branchName;
}

async function assertAutoMergeReady(projectPath: string, task: ProjectTask): Promise<void> {
  if (!task.lastRun) {
    throw new Error("Task has no last run summary for auto-merge promotion.");
  }
  if (task.risk !== "low-risk") {
    throw new Error("Auto-merge is restricted to low-risk tasks.");
  }
  if (task.lastRun.outcome !== "completed") {
    throw new Error(`Task is not in a completed state for auto-merge: ${task.lastRun.outcome}`);
  }
  if (task.lastRun.promotionDecision !== "auto-merge-eligible") {
    throw new Error(`Task is not eligible for auto-merge: ${task.lastRun.promotionDecision}`);
  }

  const projectConfig = await loadProjectConfig(projectPath);
  const configuredValidationNames = getConfiguredValidationNames(projectConfig);
  if (configuredValidationNames.length === 0) {
    throw new Error("Auto-merge requires at least one configured validation command.");
  }

  if (task.lastRun.validation.length === 0) {
    throw new Error("Auto-merge requires at least one successful validation result.");
  }
  if (task.lastRun.validation.some((item) => item.exitCode !== 0)) {
    throw new Error("Auto-merge requires all persisted validations to pass.");
  }
  for (const requiredName of configuredValidationNames) {
    if (!task.lastRun.validation.some((item) => item.name === requiredName && item.exitCode === 0)) {
      throw new Error(`Auto-merge requires configured validation '${requiredName}' to pass.`);
    }
  }
}

async function ensurePromotionWorkspaceClean(projectPath: string): Promise<void> {
  const state = await getGitWorkingTreeState(projectPath);
  if (state.dirty) {
    throw new Error("Cannot apply promotion on a dirty git tree.");
  }
}

async function syncTaskPromotionState(
  projectPath: string,
  taskId: string,
  artifactPath: string,
  status: PromotionArtifactStatus,
  note: string | null,
  branchName?: string,
  resultArtifactPath?: string,
): Promise<void> {
  const ledger = await loadTaskLedger(projectPath);
  const task = ledger.tasks.find((candidate) => candidate.id === taskId);
  if (!task?.lastRun || task.lastRun.promotionArtifactPath !== artifactPath) {
    return;
  }

  task.lastRun.promotionArtifactState = status;
  task.lastRun.promotionResultArtifactPath = resultArtifactPath ?? task.lastRun.promotionResultArtifactPath ?? null;
  if (branchName) {
    task.branch = branchName;
  }
  if (status === "applied" && task.lastRun.promotionAction === "queue-auto-merge") {
    task.status = "promoted";
    task.promotedAt = new Date().toISOString();
  }
  task.notes = [...(task.notes ?? []), `Promotion artifact ${status}${note ? `: ${note}` : "."}`];
  task.updatedAt = new Date().toISOString();
  await saveTaskLedger(projectPath, ledger);
}

async function writePromotionResult(
  projectPath: string,
  item: PromotionQueueItem,
  result: "applied" | "rejected",
  branch: string | null,
  note: string | null,
): Promise<string> {
  return writePromotionResultArtifact(projectPath, {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: item.artifact.projectAlias,
    taskId: item.artifact.taskId,
    sourcePromotionArtifactPath: item.artifactPath,
    sourcePromotionAction: item.artifact.action,
    sourcePromotionDecision: item.artifact.decision,
    result,
    branch,
    baseBranch: item.artifact.baseBranch,
    note,
  });
}