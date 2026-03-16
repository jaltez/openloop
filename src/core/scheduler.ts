import fs from "node:fs/promises";
import path from "node:path";
import { loadTaskLedger, saveTaskLedger } from "./task-ledger.js";
import { resolveModel } from "./model-resolution.js";
import { loadProjectConfig } from "./project-config.js";
import { evaluateTaskScopePolicy, loadProjectPolicy } from "./project-policy.js";
import { loadGlobalConfig } from "./global-config.js";
import { getGitDiffFingerprint, getGitWorkingTreeState, addWorktree, removeWorktree } from "./git.js";
import { runPi, runAgent, type PiRunOptions } from "./pi.js";
import { RunTimeoutError, withTimeout } from "./timeout.js";
import { writePromotionArtifact } from "./promotion-artifacts.js";
import { writeRunSummary } from "./run-summaries.js";
import { runConfiguredValidations, type ValidationRunner } from "./validation.js";
import { ensureDir } from "./fs.js";
import { SUPPORTED_SELF_HEALING_TASK_KINDS, type LinkedProject, type ProjectConfig, type ProjectPolicy, type ProjectTask, type PromotionArtifact, type SchedulerResult, type SchedulerSelection, type TaskLedger, type TaskRunSummary, type ValidationSummary, type WorkerRole } from "./types.js";
import { getConfiguredValidationNames, hasAllRequiredAutoMergeValidations } from "./validation-utils.js";

type PiRunner = (options: PiRunOptions) => Promise<number>;

export function selectNextTask(ledger: TaskLedger): SchedulerSelection {
  const readyLowRisk = ledger.tasks.find((task) => task.status === "ready" && task.risk === "low-risk");
  if (readyLowRisk) {
    return { task: readyLowRisk, mode: "implement", reason: "selected ready low-risk task" };
  }

  const readyTask = ledger.tasks.find((task) => task.status === "ready");
  if (readyTask) {
    return { task: readyTask, mode: "implement", reason: "selected ready task" };
  }

  const planningTask = ledger.tasks.find((task) => task.status === "proposed" || task.status === "planned");
  if (planningTask) {
    return { task: planningTask, mode: "plan", reason: "selected task requiring planning" };
  }

  return { task: null, mode: "idle", reason: "no eligible task found" };
}

export async function runProjectIteration(
  project: LinkedProject,
  options?: {
    modelOverride?: string;
    piRunner?: PiRunner;
    validationRunner?: ValidationRunner;
    timeoutMs?: number;
    maxAttemptsPerTask?: number;
    noProgressRepeatLimit?: number;
  },
): Promise<SchedulerResult> {
  const ledger = await loadTaskLedger(project.path);
  const model = await resolveModel(project.path, options?.modelOverride);
  const globalConfig = await loadGlobalConfig();
  const projectConfig = await loadProjectConfig(project.path);
  const projectPolicy = await loadProjectPolicy(project.path);
  let selection = selectNextTask(ledger);
  const timeoutMs = options?.timeoutMs ?? globalConfig.runtime.runTimeoutSeconds * 1000;
  const maxAttemptsPerTask = options?.maxAttemptsPerTask ?? globalConfig.runtime.maxAttemptsPerTask;
  const noProgressRepeatLimit = options?.noProgressRepeatLimit ?? globalConfig.runtime.noProgressRepeatLimit;

  if (!selection.task) {
    const generatedTask = synthesizeContinuousImprovementTask(project.alias, ledger, projectConfig, projectPolicy);
    if (generatedTask) {
      ledger.tasks.push(generatedTask);
      await saveTaskLedger(project.path, ledger);
      selection = {
        task: generatedTask,
        mode: "plan",
        reason: `generated continuous improvement backlog task: ${generatedTask.title}`,
      };
    }
  }

  if (!selection.task) {
    const result: SchedulerResult = {
      projectAlias: project.alias,
      taskId: null,
      mode: "idle",
      role: null,
      reason: selection.reason,
      model,
      exitCode: null,
      prompt: null,
      validation: [],
      promotionDecision: "none",
      promotionAction: "none",
      promotionArtifactPath: null,
      promotionResultArtifactPath: null,
      taskStatus: null,
      promotedAt: null,
      stoppedBy: "none",
      attemptNumber: null,
      dirtyTreeDetected: false,
      budgetSnapshotUsd: null,
    };
    await writeRunSummary(project.path, result);
    return result;
  }

  const task = selection.task;
  const mode = selection.mode;
  if (mode === "idle") {
    throw new Error("Scheduler selected a task but remained idle.");
  }
  const role = determineWorkerRole(task, mode);

  const scopeDecision = evaluateTaskScopePolicy(task, projectPolicy, projectConfig.risk.defaultUnknownAreaClassification);
  if (scopeDecision.note) {
    task.notes = [...(task.notes ?? []), scopeDecision.note];
  }
  if (scopeDecision.adjustedRisk !== task.risk) {
    task.risk = scopeDecision.adjustedRisk;
  }
  if (scopeDecision.blocked) {
    task.status = "blocked";
    task.lastFailureSignature = scopeDecision.failureSignature;
    task.updatedAt = new Date().toISOString();
    await saveTaskLedger(project.path, ledger);

    const result: SchedulerResult = {
      projectAlias: project.alias,
      taskId: task.id,
      mode,
      role,
      reason: `${selection.reason}; ${scopeDecision.reason ?? "task blocked by project scope policy"}`,
      model,
      exitCode: null,
      prompt: null,
      validation: [],
      promotionDecision: "none",
      promotionAction: "none",
      promotionArtifactPath: null,
      promotionResultArtifactPath: null,
      taskStatus: task.status,
      promotedAt: task.promotedAt ?? null,
      stoppedBy: "none",
      attemptNumber: null,
      dirtyTreeDetected: false,
      budgetSnapshotUsd: null,
    };
    await writeRunSummary(project.path, result);
    return result;
  }

  const selfHealingBlock = getSelfHealingBlock(task, projectPolicy);
  if (selfHealingBlock) {
    task.status = "blocked";
    task.lastFailureSignature = selfHealingBlock.failureSignature;
    task.notes = [...(task.notes ?? []), selfHealingBlock.note];
    task.updatedAt = new Date().toISOString();
    await saveTaskLedger(project.path, ledger);

    const result: SchedulerResult = {
      projectAlias: project.alias,
      taskId: task.id,
      mode,
      role,
      reason: `${selection.reason}; ${selfHealingBlock.reason}`,
      model,
      exitCode: null,
      prompt: null,
      validation: [],
      promotionDecision: "none",
      promotionAction: "none",
      promotionArtifactPath: null,
      promotionResultArtifactPath: null,
      taskStatus: task.status,
      promotedAt: task.promotedAt ?? null,
      stoppedBy: "none",
      attemptNumber: null,
      dirtyTreeDetected: false,
      budgetSnapshotUsd: null,
    };
    await writeRunSummary(project.path, result);
    return result;
  }

  if (task.attempts >= maxAttemptsPerTask) {
    task.status = "blocked";
    task.lastFailureSignature = "max-attempts-reached";
    task.notes = [...(task.notes ?? []), `Openloop blocked task after reaching max attempts (${maxAttemptsPerTask}).`];
    task.updatedAt = new Date().toISOString();
    await saveTaskLedger(project.path, ledger);

    const result: SchedulerResult = {
      projectAlias: project.alias,
      taskId: task.id,
      mode,
      role,
      reason: `${selection.reason}; max attempts reached`,
      model,
      exitCode: null,
      prompt: null,
      validation: [],
      promotionDecision: "none",
      promotionAction: "none",
      promotionArtifactPath: null,
      promotionResultArtifactPath: null,
      taskStatus: task.status,
      promotedAt: task.promotedAt ?? null,
      stoppedBy: "none",
      attemptNumber: task.attempts + 1,
      dirtyTreeDetected: false,
      budgetSnapshotUsd: null,
    };
    await writeRunSummary(project.path, result);
    return result;
  }

  const previousStatus = task.status;
  const previousFailureSignature = task.lastFailureSignature;
  const previousPromotionDecision = task.lastRun?.promotionDecision ?? null;
  task.status = "in_progress";
  task.owner = "openloop";
  task.updatedAt = new Date().toISOString();
  await saveTaskLedger(project.path, ledger);

  // D6: Use the model-agnostic agent runner, falling back to runPi for backward compat.
  const runner = options?.piRunner ?? ((runOptions: PiRunOptions) => runAgent(runOptions, projectConfig));
  let validation: ValidationSummary[] = [];
  let promotionArtifactPath: string | null = null;
  // W1: Include spec content in implement prompts when a spec file exists.
  const specContent = mode === "implement" ? await readSpecContent(project.path, task) : null;
  const builtPrompt = buildPrompt(task, mode, role, specContent);
  const gitState = await getInitialGitState(project.path);
  const attemptNumber = task.attempts + 1;
  const runStartedAt = Date.now();
  const beforeFingerprint = await getGitDiffFingerprint(project.path);

  // D5: Set up an isolated git worktree for this run if useWorktree is configured.
  const useWorktree = projectConfig.runtime.useWorktree;
  const worktreeBranchName = useWorktree
    ? `${projectConfig.runtime.branchPrefix}${task.id}`
    : null;
  const worktreePath = useWorktree
    ? path.join(project.path, ".openloop", "worktrees", task.id)
    : null;

  if (useWorktree && worktreePath && worktreeBranchName) {
    try {
      await addWorktree(project.path, worktreePath, worktreeBranchName);
    } catch {
      // Worktree setup failure: fall back to running in main tree.
    }
  }

  // The project context passed to Pi — use worktree path if available.
  const runProject = worktreePath
    ? { ...project, path: worktreePath }
    : project;

  const getRemainingTimeoutMs = (): number | undefined => {
    if (timeoutMs === undefined) {
      return undefined;
    }
    return Math.max(timeoutMs - (Date.now() - runStartedAt), 0);
  };

  const cleanupWorktree = async () => {
    if (worktreePath) {
      await removeWorktree(project.path, worktreePath).catch(() => {});
    }
  };

  try {
    const exitCode = await withTimeout(
      runner({
        project: runProject,
        model: model ?? undefined,
        prompt: builtPrompt,
        timeoutMs: getRemainingTimeoutMs(),
      }),
      getRemainingTimeoutMs(),
      "Run exceeded timeout during Pi execution.",
    );
    let outcome: TaskRunSummary["outcome"] = "completed";
    let stoppedBy: SchedulerResult["stoppedBy"] = "none";

    if (exitCode === 0) {
      if (mode === "implement") {
        validation = await runConfiguredValidations(project.path, projectConfig, options?.validationRunner, {
          getTimeoutMs: getRemainingTimeoutMs,
        });
        const validationFailed = validation.some((item) => item.exitCode !== 0);
        if (validationFailed) {
          task.status = "failed";
          task.attempts += 1;
          task.lastFailureSignature = `validation-${validation.find((item) => item.exitCode !== 0)?.name ?? "unknown"}`;
          task.notes = [...(task.notes ?? []), "Openloop implement run passed Pi but failed validation."];
          outcome = "validation-failed";
        } else {
          task.status = "done";
          task.notes = [...(task.notes ?? []), `Openloop ${mode} run succeeded.`];
        }
      } else {
        // W1: After a successful plan run, detect spec file written by Pi.
        await ensureDir(path.join(project.path, ".openloop", "specs"));
        await detectAndSetSpecId(project.path, task);
        task.status = "ready";
        task.notes = [...(task.notes ?? []), `Openloop ${mode} run succeeded.`];
        outcome = "planned";
      }
    } else {
      task.status = previousStatus;
      task.attempts += 1;
      task.lastFailureSignature = `pi-exit-${exitCode}`;
      task.notes = [...(task.notes ?? []), `Openloop ${mode} run failed with exit ${exitCode}.`];
      outcome = "pi-failed";
    }

    const effectivePromotionMode = resolveEffectivePromotionMode(task, projectPolicy, projectConfig.risk.requirePolicyForAutoMerge, projectConfig);
    const promotionDecision = decidePromotion(task, validation, effectivePromotionMode, projectConfig);
    const afterFingerprint = await getGitDiffFingerprint(project.path);
    if (shouldStopForNoProgress({
      task,
      previousFailureSignature,
      previousPromotionDecision,
      currentPromotionDecision: promotionDecision,
      beforeFingerprint,
      afterFingerprint,
      noProgressRepeatLimit,
    })) {
      task.status = "blocked";
      task.notes = [...(task.notes ?? []), "Openloop blocked task due to no-progress detection."];
      stoppedBy = "no-progress";
    }

    const promotionAction = decidePromotionAction(promotionDecision);
    promotionArtifactPath = await maybeWritePromotionArtifact(project.path, {
      projectAlias: project.alias,
      taskId: task.id,
      baseBranch: gitState.currentBranch,
      decision: promotionDecision,
      action: promotionAction,
      effectivePromotionMode,
      validation,
      piExitCode: exitCode,
      outcome,
    });
    task.lastRun = {
      completedAt: new Date().toISOString(),
      mode,
      role,
      piExitCode: exitCode,
      outcome,
      baseBranch: gitState.currentBranch,
      validation,
      promotionDecision,
      effectivePromotionMode,
      promotionAction,
      promotionArtifactPath,
      promotionArtifactState: "pending",
      promotionResultArtifactPath: null,
    };

    task.updatedAt = new Date().toISOString();
    await saveTaskLedger(project.path, ledger);

    const result = {
      projectAlias: project.alias,
      taskId: task.id,
      mode,
      role,
      reason: selection.reason,
      model,
      exitCode,
      prompt: builtPrompt,
      validation,
      promotionDecision,
      promotionAction,
      promotionArtifactPath,
      promotionResultArtifactPath: task.lastRun?.promotionResultArtifactPath ?? null,
      taskStatus: task.status,
      promotedAt: task.promotedAt ?? null,
      stoppedBy,
      attemptNumber,
      dirtyTreeDetected: beforeFingerprint !== afterFingerprint,
      budgetSnapshotUsd: null,
    };
    await writeRunSummary(project.path, result);
    await cleanupWorktree();
    return result;
  } catch (error) {
    task.status = previousStatus;
    task.attempts += 1;
    task.lastFailureSignature = error instanceof RunTimeoutError ? "timeout" : error instanceof Error ? error.message : String(error);
    task.notes = [...(task.notes ?? []), `Openloop ${mode} run threw an error.`];
    const afterFingerprint = await getGitDiffFingerprint(project.path);
    const stoppedBy: SchedulerResult["stoppedBy"] = error instanceof RunTimeoutError ? "timeout" : shouldStopForNoProgress({
      task,
      previousFailureSignature,
      previousPromotionDecision,
      currentPromotionDecision: "blocked",
      beforeFingerprint,
      afterFingerprint,
      noProgressRepeatLimit,
    }) ? "no-progress" : "none";
    if (stoppedBy === "no-progress") {
      task.status = "blocked";
      task.notes = [...(task.notes ?? []), "Openloop blocked task due to no-progress detection."];
    }
    const effectivePromotionMode = resolveEffectivePromotionMode(task, projectPolicy, projectConfig.risk.requirePolicyForAutoMerge, projectConfig);
    const promotionAction = decidePromotionAction("blocked");
    promotionArtifactPath = await maybeWritePromotionArtifact(project.path, {
      projectAlias: project.alias,
      taskId: task.id,
      baseBranch: gitState.currentBranch,
      decision: "blocked",
      action: promotionAction,
      effectivePromotionMode,
      validation,
      piExitCode: null,
      outcome: "error",
    });
    task.lastRun = {
      completedAt: new Date().toISOString(),
      mode,
      role,
      piExitCode: null,
      outcome: "error",
      baseBranch: gitState.currentBranch,
      validation,
      promotionDecision: "blocked",
      effectivePromotionMode,
      promotionAction,
      promotionArtifactPath,
      promotionArtifactState: "pending",
      promotionResultArtifactPath: null,
    };
    task.updatedAt = new Date().toISOString();
    await saveTaskLedger(project.path, ledger);
    await writeRunSummary(project.path, {
      projectAlias: project.alias,
      taskId: task.id,
      mode,
      role,
      reason: selection.reason,
      model,
      exitCode: null,
      prompt: builtPrompt,
      validation,
      promotionDecision: "blocked",
      promotionAction,
      promotionArtifactPath,
      promotionResultArtifactPath: task.lastRun?.promotionResultArtifactPath ?? null,
      taskStatus: task.status,
      promotedAt: task.promotedAt ?? null,
      stoppedBy,
      attemptNumber,
      dirtyTreeDetected: beforeFingerprint !== afterFingerprint,
      budgetSnapshotUsd: null,
    });
    await cleanupWorktree();
    throw error;
  }
}

function shouldStopForNoProgress(input: {
  task: ProjectTask;
  previousFailureSignature: string | null;
  previousPromotionDecision: SchedulerResult["promotionDecision"] | null;
  currentPromotionDecision: SchedulerResult["promotionDecision"];
  beforeFingerprint: string | null;
  afterFingerprint: string | null;
  noProgressRepeatLimit: number;
}): boolean {
  const observations: string[] = [];
  const currentFailureSignature = input.task.lastFailureSignature;

  if (currentFailureSignature && input.previousFailureSignature === currentFailureSignature) {
    observations.push(`failure-${sanitizeObservationKey(currentFailureSignature)}`);
  }

  if (input.previousPromotionDecision === "blocked" && input.currentPromotionDecision === "blocked") {
    observations.push("promotion-blocked");
  }

  if (input.beforeFingerprint === input.afterFingerprint) {
    observations.push("diff-unchanged");
  }

  let blocked = false;
  for (const observation of observations) {
    const count = nextNoProgressCount(input.task.notes ?? [], observation);
    input.task.notes = [...(input.task.notes ?? []), `openloop:no-progress:${observation}:${count}`];
    if (count >= input.noProgressRepeatLimit) {
      blocked = true;
    }
  }

  return blocked;
}

function nextNoProgressCount(notes: string[], key: string): number {
  const prefix = `openloop:no-progress:${key}:`;
  for (let index = notes.length - 1; index >= 0; index -= 1) {
    const note = notes[index];
    if (!note?.startsWith(prefix)) {
      continue;
    }
    const count = Number(note.slice(prefix.length));
    return Number.isFinite(count) ? count + 1 : 1;
  }
  return 1;
}

function sanitizeObservationKey(value: string): string {
  return value.replace(/[^a-z0-9-]+/gi, "-").replace(/^-|-$/g, "").toLowerCase();
}

async function getInitialGitState(projectPath: string): Promise<{ currentBranch: string | null }> {
  try {
    const state = await getGitWorkingTreeState(projectPath);
    return { currentBranch: state.currentBranch };
  } catch {
    return { currentBranch: null };
  }
}

function decidePromotionAction(decision: SchedulerResult["promotionDecision"]): SchedulerResult["promotionAction"] {
  if (decision === "auto-merge-eligible") {
    return "queue-auto-merge";
  }
  if (decision === "manual-review") {
    return "queue-review";
  }
  if (decision === "blocked") {
    return "block";
  }
  return "none";
}

async function maybeWritePromotionArtifact(
  projectPath: string,
  artifactInput: Omit<PromotionArtifact, "version" | "createdAt" | "status" | "processedAt" | "note">,
): Promise<string | null> {
  if (artifactInput.action === "none") {
    return null;
  }

  const artifact: PromotionArtifact = {
    version: 1,
    createdAt: new Date().toISOString(),
    ...artifactInput,
    status: "pending",
    processedAt: null,
    note: null,
  };
  return writePromotionArtifact(projectPath, artifact);
}

function decidePromotion(
  task: ProjectTask,
  validation: ValidationSummary[],
  effectivePromotionMode: ProjectTask["promotion"],
  projectConfig: ProjectConfig,
): SchedulerResult["promotionDecision"] {
  if (validation.some((item) => item.exitCode !== 0)) {
    return "blocked";
  }

  if (task.status !== "done") {
    return "none";
  }

  if (effectivePromotionMode === "auto-merge") {
    // S3: If no validations are configured, auto-merge is unsafe regardless of risk tier.
    if (getConfiguredValidationNames(projectConfig).length === 0) {
      console.warn(
        `[openloop] Warning: no validation commands configured for project. Forcing manual-only promotion. ` +
        `Run 'openloop config project-set-validation' to configure lint/test/typecheck commands.`,
      );
      return "manual-review";
    }
    if (!hasAllRequiredAutoMergeValidations(validation, projectConfig)) {
      return "manual-review";
    }
    return "auto-merge-eligible";
  }

  return "manual-review";
}

function resolveEffectivePromotionMode(
  task: ProjectTask,
  projectPolicy: ProjectPolicy,
  requirePolicyForAutoMerge: boolean,
  projectConfig?: ProjectConfig,
): ProjectTask["promotion"] {
  const policyMode = getPolicyPromotionMode(task.risk, projectPolicy);
  if (task.promotion === "manual-only" || policyMode === "manual-only") {
    return "manual-only";
  }

  if (task.promotion === "pull-request" || policyMode === "pull-request") {
    return "pull-request";
  }

  if (task.risk !== "low-risk") {
    return "pull-request";
  }

  if (requirePolicyForAutoMerge && !projectPolicy.riskClasses[task.risk].autoMergeAllowed) {
    return "pull-request";
  }

  return "auto-merge";
}

function getPolicyPromotionMode(risk: ProjectTask["risk"], projectPolicy: ProjectPolicy): ProjectTask["promotion"] {
  if (risk === "low-risk") {
    return projectPolicy.promotion.lowRiskMode;
  }
  if (risk === "medium-risk") {
    return projectPolicy.promotion.mediumRiskMode;
  }
  return projectPolicy.promotion.highRiskMode;
}

function getSelfHealingBlock(task: ProjectTask, projectPolicy: ProjectPolicy): {
  reason: string;
  note: string;
  failureSignature: string;
} | null {
  if (!isSelfHealingTask(task.kind)) {
    return null;
  }

  if (!projectPolicy.selfHealing.enabled) {
    return {
      reason: "self-healing is disabled by project policy",
      note: "Openloop blocked self-healing task because self-healing is disabled by project policy.",
      failureSignature: "self-healing-disabled",
    };
  }

  if (!isSupportedSelfHealingTask(task.kind)) {
    return {
      reason: `self-healing task kind '${task.kind}' is outside the V1 scope`,
      note: `Openloop blocked self-healing task kind '${task.kind}' because V1 only supports ledger-driven lint-fix and type-fix tasks.`,
      failureSignature: "self-healing-kind-not-supported",
    };
  }

  if (!projectPolicy.selfHealing.allowedTaskKinds.includes(task.kind)) {
    return {
      reason: `self-healing task kind '${task.kind}' is not allowed by project policy`,
      note: `Openloop blocked self-healing task kind '${task.kind}' because it is not allowlisted in project policy.`,
      failureSignature: "self-healing-kind-not-allowed",
    };
  }

  return null;
}

function isSelfHealingTask(kind: ProjectTask["kind"]): boolean {
  return kind === "lint-fix" || kind === "type-fix" || kind === "localized-test-fix" || kind === "ci-heal";
}

function isSupportedSelfHealingTask(kind: ProjectTask["kind"]): kind is (typeof SUPPORTED_SELF_HEALING_TASK_KINDS)[number] {
  return SUPPORTED_SELF_HEALING_TASK_KINDS.includes(kind as (typeof SUPPORTED_SELF_HEALING_TASK_KINDS)[number]);
}

export function determineWorkerRole(task: ProjectTask, mode: "implement" | "plan"): WorkerRole {
  if (task.kind === "discovery" || task.kind === "scope-proposal") {
    return "repo-improver";
  }
  if (isSupportedSelfHealingTask(task.kind)) {
    return "ci-healer";
  }
  if (mode === "plan") {
    return "sdd-planner";
  }
  return "implementer";
}

export function buildPrompt(task: ProjectTask, mode: "implement" | "plan", role: WorkerRole, specContent?: string | null): string {
  const header = mode === "implement"
    ? isSupportedSelfHealingTask(task.kind)
      ? `Repair the following ${describeSelfHealingTask(task.kind)} with the smallest viable change.`
      : "Implement the following task."
    : "Plan the following task and prepare it for implementation.";
  const lines = [
    header,
    `Task ID: ${task.id}`,
    `Title: ${task.title}`,
    `Kind: ${task.kind}`,
    `Assigned Role: ${role}`,
    `Risk: ${task.risk}`,
    `Source: ${task.source.type} / ${task.source.ref}`,
  ];

  if (mode === "implement" && isSupportedSelfHealingTask(task.kind)) {
    lines.push("Self-Healing Scope: Ledger-driven repair limited to lint-fix, type-fix, and localized-test-fix tasks.");
  }

  if (mode === "plan") {
    lines.push(`Spec Output: Write your implementation plan to .openloop/specs/${task.id}.md before finishing.`);
  }

  if (task.scope?.paths && task.scope.paths.length > 0) {
    lines.push("Scope Paths:", ...task.scope.paths.map((scopePath) => `- ${scopePath}`));
  }

  lines.push("Acceptance Criteria:", ...task.acceptanceCriteria.map((criterion) => `- ${criterion}`));

  if (mode === "implement" && specContent) {
    lines.push("", "## Implementation Spec", specContent);
  }

  return lines.join("\n");
}

async function readSpecContent(projectPath: string, task: ProjectTask): Promise<string | null> {
  const specId = task.specId;
  if (!specId) {
    return null;
  }
  try {
    return await fs.readFile(path.join(projectPath, specId), "utf8");
  } catch {
    return null;
  }
}

async function detectAndSetSpecId(projectPath: string, task: ProjectTask): Promise<void> {
  const specPath = path.join(".openloop", "specs", `${task.id}.md`);
  const fullPath = path.join(projectPath, specPath);
  try {
    await fs.access(fullPath);
    task.specId = specPath;
  } catch {
    // no spec file written by Pi — that's OK
  }
}

function synthesizeContinuousImprovementTask(
  projectAlias: string,
  ledger: TaskLedger,
  projectConfig: ProjectConfig,
  projectPolicy: ProjectPolicy,
): ProjectTask | null {
  const existingRefs = new Set(ledger.tasks.map((task) => task.source.ref));
  const configuredValidationNames = getConfiguredValidationNames(projectConfig);

  if (configuredValidationNames.length === 0 && !existingRefs.has("continuous-improvement:validation-setup")) {
    return createContinuousImprovementTask(projectAlias, {
      id: "improve-validation-setup",
      title: "Define validation commands for unattended runs",
      kind: "discovery",
      ref: "continuous-improvement:validation-setup",
      acceptanceCriteria: [
        "Project-local validation commands are defined for at least one unattended validation step.",
        "The task ledger captures the follow-up implementation scope clearly enough for execution.",
      ],
    });
  }

  if (projectConfig.validation.testCommand === null && !existingRefs.has("continuous-improvement:test-command")) {
    return createContinuousImprovementTask(projectAlias, {
      id: "define-test-command",
      title: "Define unattended test command",
      kind: "discovery",
      ref: "continuous-improvement:test-command",
      acceptanceCriteria: [
        "Project-local runtime config defines a test command suitable for unattended validation.",
        "The task ledger captures enough follow-up detail to implement and verify the command safely.",
      ],
    });
  }

  if (projectConfig.validation.typecheckCommand === null && !existingRefs.has("continuous-improvement:typecheck-command")) {
    return createContinuousImprovementTask(projectAlias, {
      id: "define-typecheck-command",
      title: "Define unattended typecheck command",
      kind: "discovery",
      ref: "continuous-improvement:typecheck-command",
      acceptanceCriteria: [
        "Project-local runtime config defines a typecheck command suitable for unattended validation.",
        "The task ledger captures enough follow-up detail to implement and verify the command safely.",
      ],
    });
  }

  if (projectConfig.validation.lintCommand === null && !existingRefs.has("continuous-improvement:lint-command")) {
    return createContinuousImprovementTask(projectAlias, {
      id: "define-lint-command",
      title: "Define unattended lint command",
      kind: "discovery",
      ref: "continuous-improvement:lint-command",
      acceptanceCriteria: [
        "Project-local runtime config defines a lint command suitable for unattended validation.",
        "The task ledger captures enough follow-up detail to implement and verify the command safely.",
      ],
    });
  }

  const missingScopePolicy = projectPolicy.scope.allowGlobs.length === 0
    && projectPolicy.scope.denyGlobs.length === 0
    && projectPolicy.scope.highRiskAreas.length === 0;
  if (missingScopePolicy && !existingRefs.has("continuous-improvement:scope-policy")) {
    return createContinuousImprovementTask(projectAlias, {
      id: "define-scope-policy",
      title: "Define unattended scope policy",
      kind: "scope-proposal",
      ref: "continuous-improvement:scope-policy",
      acceptanceCriteria: [
        "The project policy defines unattended allowlists, denied areas, or high-risk areas.",
        "The resulting policy is specific enough for scheduler-side scope enforcement.",
      ],
    });
  }

  return null;
}

function createContinuousImprovementTask(
  projectAlias: string,
  input: {
    id: string;
    title: string;
    kind: ProjectTask["kind"];
    ref: string;
    acceptanceCriteria: string[];
  },
): ProjectTask {
  const now = new Date().toISOString();

  return {
    id: input.id,
    title: input.title,
    kind: input.kind,
    status: "proposed",
    risk: "medium-risk",
    scope: null,
    source: {
      type: "discovery",
      ref: input.ref,
    },
    specId: null,
    branch: null,
    owner: projectAlias,
    acceptanceCriteria: input.acceptanceCriteria,
    attempts: 0,
    lastFailureSignature: null,
    promotion: "manual-only",
    notes: ["Created automatically during an idle continuous improvement pass."],
    createdAt: now,
    updatedAt: now,
  };
}

function describeSelfHealingTask(kind: (typeof SUPPORTED_SELF_HEALING_TASK_KINDS)[number]): string {
  if (kind === "lint-fix") {
    return "lint issue";
  }
  if (kind === "type-fix") {
    return "type issue";
  }
  return "localized deterministic test failure";
}