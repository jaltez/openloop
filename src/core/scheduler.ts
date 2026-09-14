import path from "node:path";
import fs from "node:fs/promises";
import { spawn } from "node:child_process";
import { loadTaskLedger, upsertTask } from "./task-ledger.js";
import { resolveModel } from "./model-resolution.js";
import { loadProjectConfig } from "./project-config.js";
import { evaluateTaskScopePolicy, loadProjectPolicy } from "./project-policy.js";
import { loadGlobalConfig } from "./global-config.js";
import { getGitDiffFingerprint, getGitWorkingTreeState, getGitDiffStat, addWorktree, removeWorktree } from "./git.js";
import { assertProviderAvailable, runAgent, type PiRunOptions } from "./pi.js";
import { resolveProvider } from "./providers.js";
import { RunTimeoutError, withTimeout } from "./timeout.js";
import { writePromotionArtifact } from "./promotion-artifacts.js";
import { writeRunSummary } from "./run-summaries.js";
import { runConfiguredValidations, type ValidationRunner } from "./validation.js";
import { runReview } from "./review.js";
import { writeApprovalPacket } from "./approval-packets.js";
import { ensureDir, fileExists } from "./fs.js";
import type {
  AgentRunResult,
  LinkedProject,
  ProjectTask,
  PromotionArtifact,
  ReviewResult,
  SchedulerResult,
  SchedulerSelection,
  TaskLedger,
  ValidationSummary,
} from "./types.js";

// Re-export from extracted submodules for backward compatibility
export { determineWorkerRole, isSupportedSelfHealingTask, getSelfHealingBlock } from "./scheduler/self-healing.js";
export { buildPrompt, readSpecContent, detectAndSetSpecId, synthesizeContinuousImprovementTask } from "./scheduler/tasks.js";
export { decidePromotion, decidePromotionAction, resolveEffectivePromotionMode } from "./scheduler/promotion.js";
export { shouldStopForNoProgress } from "./scheduler/no-progress.js";

import { determineWorkerRole } from "./scheduler/self-healing.js";
import {
  buildPrompt,
  buildVerifierPrompt,
  readSpecContent,
  detectAndSetSpecId,
  synthesizeContinuousImprovementTask,
} from "./scheduler/tasks.js";
import { decidePromotion, decidePromotionAction, resolveEffectivePromotionMode } from "./scheduler/promotion.js";
import { getSelfHealingBlock } from "./scheduler/self-healing.js";
import { shouldStopForNoProgress } from "./scheduler/no-progress.js";

type PiRunner = (options: PiRunOptions) => Promise<AgentRunResult>;

export function selectNextTask(ledger: TaskLedger, now: Date = new Date()): SchedulerSelection {
  const readyTasks = ledger.tasks.filter((task) => task.status === "ready");

  // dependsOn gating: a ready task is only eligible when every local ref
  // resolves to a promoted/done/cancelled task. Cross-project refs are
  // reserved for future cross-repo scheduling — never satisfied here.
  const satisfiableReady = readyTasks.filter((task) =>
    (task.dependsOn ?? []).every((ref) => {
      if (ref.includes(":")) {
        return false;
      }
      const dependency = ledger.tasks.find((candidate) => candidate.id === ref);
      return dependency !== undefined && ["promoted", "done", "cancelled"].includes(dependency.status);
    }),
  );

  // Aging: a medium/high-risk task stuck ready for over 24h becomes
  // queue-equivalent to low-risk, so low-risk-first cannot starve it.
  // Within the equivalent set the oldest-updated task runs first.
  const staleCutoff = new Date(now.getTime() - 24 * 60 * 60 * 1000).toISOString();
  const equivalent = satisfiableReady.filter((task) => task.risk === "low-risk" || task.updatedAt < staleCutoff);
  if (equivalent.length > 0) {
    const oldest = [...equivalent].sort((left, right) => {
      const order = left.updatedAt.localeCompare(right.updatedAt);
      return order !== 0 ? order : left.id.localeCompare(right.id);
    })[0]!;
    const aged = oldest.risk !== "low-risk";
    return {
      task: oldest,
      mode: "implement",
      reason: aged ? `selected ready task ${oldest.id} (aged past 24h, queue-equivalent to low-risk)` : "selected ready low-risk task",
    };
  }

  const readyTask = satisfiableReady[0];
  if (readyTask) {
    return { task: readyTask, mode: "implement", reason: "selected ready task" };
  }

  const planningTask = ledger.tasks.find((task) => task.status === "proposed" || task.status === "planned");
  if (planningTask) {
    return { task: planningTask, mode: "plan", reason: "selected task requiring planning" };
  }

  if (readyTasks.length > 0) {
    const localBlockers = new Set<string>();
    const crossRefs = new Set<string>();
    for (const ref of readyTasks.flatMap((task) => task.dependsOn ?? [])) {
      if (ref.includes(":")) {
        crossRefs.add(ref);
        continue;
      }
      const dependency = ledger.tasks.find((candidate) => candidate.id === ref);
      if (!dependency || !["promoted", "done", "cancelled"].includes(dependency.status)) {
        localBlockers.add(ref);
      }
    }
    const blockerList = [...localBlockers, ...[...crossRefs].map((ref) => `${ref} (cross-project, not schedulable here)`)];
    if (blockerList.length > 0) {
      return { task: null, mode: "idle", reason: `ready tasks blocked by unsatisfied dependsOn: ${blockerList.join(", ")}` };
    }
  }

  return { task: null, mode: "idle", reason: "no eligible task found" };
}

/**
 * Resolve the cost of a completed agent run: a parsed positive cost wins
 * ("measured"); anything else falls back to the configured estimate.
 */
export function resolveRunCost(result: AgentRunResult, estimated: number): { costUsd: number; costSource: "measured" | "estimated" } {
  const measured = result.usage?.costUsd;
  if (typeof measured === "number" && Number.isFinite(measured) && measured > 0) {
    return { costUsd: measured, costSource: "measured" };
  }
  return { costUsd: estimated, costSource: "estimated" };
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
    reviewerRunner?: (prompt: string) => Promise<number>;
    verifierRunner?: (prompt: string) => Promise<number>;
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
  const estimatedCostPerRun = globalConfig.budgets.estimatedCostPerRunUsd ?? 0;
  if (!selection.task) {
    const generatedTask = synthesizeContinuousImprovementTask(project.alias, ledger, projectConfig, projectPolicy);
    if (generatedTask) {
      await upsertTask(project.path, generatedTask);
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
      costUsd: null,
      costSource: null,
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
    await upsertTask(project.path, task);

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
      costUsd: null,
      costSource: null,
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
    await upsertTask(project.path, task);

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
      costUsd: null,
      costSource: null,
    };
    await writeRunSummary(project.path, result);
    return result;
  }

  if (task.attempts >= maxAttemptsPerTask) {
    task.status = "blocked";
    task.lastFailureSignature = "max-attempts-reached";
    task.notes = [...(task.notes ?? []), `Openloop blocked task after reaching max attempts (${maxAttemptsPerTask}).`];
    task.updatedAt = new Date().toISOString();
    await upsertTask(project.path, task);

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
      costUsd: null,
      costSource: null,
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
  await upsertTask(project.path, task);

  // D6: Use the model-agnostic agent runner, falling back to runPi for backward compat.
  const defaultProvider = globalConfig.defaultProvider ?? undefined;
  // Dispatch-time provider gate: fail with a clear message before the agent
  // subprocess would spawn, instead of an ENOENT mid-run.
  if (!options?.piRunner) {
    assertProviderAvailable(projectConfig, globalConfig.defaultProvider ?? null);
  }
  const runner = options?.piRunner ?? ((runOptions: PiRunOptions) => runAgent(runOptions, projectConfig, defaultProvider));
  let validation: ValidationSummary[] = [];
  let promotionArtifactPath: string | null = null;
  let approvalPacketPath: string | null = null;
  let agentStarted = false;
  // W1: Include spec content in implement prompts when a spec file exists.
  const specContent = mode === "implement" ? await readSpecContent(project.path, task) : null;
  const resolvedProviderName = resolveProvider(
    projectConfig.agent?.type,
    projectConfig.agent?.command ?? null,
    globalConfig.defaultProvider ?? undefined,
  ).name;
  const builtPrompt = buildPrompt(task, mode, role, specContent, {
    providerIsPi: resolvedProviderName === "pi",
  });
  const gitState = await getInitialGitState(project.path);
  const attemptNumber = task.attempts + 1;
  const runStartedAt = Date.now();
  const beforeFingerprint = projectConfig.runtime.useWorktree
    ? null // Worktree starts clean; progress is detected from the after-state.
    : await getGitDiffFingerprint(project.path);

  // D5: Worktree path/branch for isolated runs (setup happens inside the try below).
  const useWorktree = projectConfig.runtime.useWorktree;
  const worktreeBranchName = useWorktree ? `${projectConfig.runtime.branchPrefix}${task.id}` : null;
  const worktreePath = useWorktree ? path.join(project.path, ".openloop", "worktrees", task.id) : null;

  // Control-plane path (main tree: .openloop ledger, specs, reviews) vs execution
  // path (worktree when runtime.useWorktree, else the main tree the agent edits).
  const controlPlanePath = project.path;
  const executionPath = worktreePath ?? project.path;

  const getRemainingTimeoutMs = (): number | undefined => {
    if (timeoutMs === undefined) {
      return undefined;
    }
    return Math.max(timeoutMs - (Date.now() - runStartedAt), 0);
  };

  // The worktree is per-run scratch: agent edits live on the run branch; the
  // checkout is always force-removed so leftover untracked files can't wedge it.
  const cleanupWorktree = async () => {
    if (worktreePath) {
      await removeWorktree(project.path, worktreePath, true).catch(() => {});
    }
  };

  try {
    // D5: Set up an isolated git worktree for this run if useWorktree is configured.
    // Fail closed: when isolation is explicitly requested but cannot be established,
    // abort the run rather than silently operating on the main working tree.
    if (useWorktree && worktreePath && worktreeBranchName) {
      try {
        await addWorktree(project.path, worktreePath, worktreeBranchName);
      } catch (error) {
        throw new Error(
          `Worktree isolation failed for ${worktreePath}: ` +
            `${error instanceof Error ? error.message : String(error)}. ` +
            `Aborting because useWorktree is enabled; refusing to fall back to the main working tree.`,
        );
      }
      // Liveness marker: crash recovery skips force-reclaiming a checkout
      // whose owning process is still alive (e.g. a concurrent CLI run).
      await fs.writeFile(path.join(worktreePath, ".openloop-run.pid"), `${process.pid}\n`, "utf8").catch(() => {});
      // Worktrees lack gitignored build state (node_modules etc.) — allow an
      // explicit setup step. Fail closed: a failed setup aborts the run.
      const worktreeSetupCommand = projectConfig.runtime.worktreeSetupCommand ?? null;
      if (worktreeSetupCommand) {
        const setupExitCode = await withTimeout(
          runWorktreeSetupCommand(worktreePath, worktreeSetupCommand),
          getRemainingTimeoutMs(),
          "Run exceeded timeout during worktree setup.",
        );
        if (setupExitCode !== 0) {
          throw new Error(
            `Worktree setup command failed: ${worktreeSetupCommand} (exit ${setupExitCode}). ` +
              `Aborting because useWorktree is enabled; refusing to fall back to the main working tree.`,
          );
        }
      }
    }
    const runProject = worktreePath ? { ...project, path: worktreePath } : project;
    agentStarted = true;
    const runResult = await withTimeout(
      runner({
        project: runProject,
        model: model ?? undefined,
        prompt: builtPrompt,
        timeoutMs: getRemainingTimeoutMs(),
      }),
      getRemainingTimeoutMs(),
      "Run exceeded timeout during Pi execution.",
    );
    const exitCode = runResult.exitCode;
    let outcome: TaskRunSummary["outcome"] = "completed";
    let stoppedBy: SchedulerResult["stoppedBy"] = "none";
    let reviewResult: ReviewResult | null = null;
    let verifierRequiresReview = false;

    if (exitCode === 0) {
      if (mode === "implement") {
        validation = await runConfiguredValidations(executionPath, projectConfig, options?.validationRunner, {
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
          // V1: Independent verifier — the implementer's work is judged by a
          // separate role before the task can be called done.
          const verificationEnabled = projectConfig.verification?.enabled ?? true;
          if (verificationEnabled) {
            const verifierOutcome = await runVerifierStage({
              task,
              specContent,
              controlPlanePath,
              model: model ?? undefined,
              runProject,
              runner,
              verifierRunner: options?.verifierRunner,
              getRemainingTimeoutMs,
            });
            if (verifierOutcome.failed) {
              task.status = "failed";
              task.attempts += 1;
              task.lastFailureSignature = "verifier-rejected";
              task.notes = [
                ...(task.notes ?? []),
                ...verifierOutcome.failingCriteria.map((criterion) => `Verifier rejected criterion ${criterion}.`),
              ];
              outcome = "validation-failed";
            } else {
              task.status = "done";
              task.notes = [...(task.notes ?? []), `Openloop ${mode} run succeeded.`];
              if (verifierOutcome.requiresReview) {
                task.notes = [...(task.notes ?? []), "Verifier verdict requires human review."];
                verifierRequiresReview = true;
              }
            }
          } else {
            task.status = "done";
            task.notes = [...(task.notes ?? []), `Openloop ${mode} run succeeded.`];
          }
          if (task.status === "done" && projectConfig.review?.enabled) {
            const reviewerRunner =
              options?.reviewerRunner ??
              (async (reviewPrompt: string) =>
                (
                  await runner({
                    project: runProject,
                    prompt: reviewPrompt,
                    model: model ?? undefined,
                    timeoutMs: getRemainingTimeoutMs(),
                  })
                ).exitCode);
            reviewResult = await runReview({
              controlPlanePath,
              executionPath,
              task,
              projectConfig,
              projectPolicy,
              reviewerRunner,
            }).catch(() => ({ findings: [], hasBlocking: false }) as ReviewResult);
            if (reviewResult.malformed) {
              task.notes = [...(task.notes ?? []), "Reviewer output malformed; downgraded to manual review."];
            }
            if (reviewResult.findings.length > 0) {
              task.notes = [...(task.notes ?? []), ...reviewResult.findings.map((f) => `Review [${f.severity}] ${f.rule}: ${f.message}`)];
              if (reviewResult.hasBlocking) {
                task.notes = [...(task.notes ?? []), "Openloop review found blocking issues; downgrading auto-merge to manual review."];
              }
            }
          }
        }
      } else {
        // W1: After a successful plan run, detect spec file written by Pi.
        await ensureDir(path.join(project.path, ".openloop", "specs"));
        await detectAndSetSpecId(project.path, task);
        // B2: Gate medium/high-risk tasks for human approval after planning.
        if (task.risk === "medium-risk" || task.risk === "high-risk") {
          task.status = "awaiting-approval";
          task.notes = [...(task.notes ?? []), `Openloop ${mode} run succeeded; awaiting human approval (${task.risk}).`];
        } else {
          task.status = "ready";
          task.notes = [...(task.notes ?? []), `Openloop ${mode} run succeeded.`];
        }
        outcome = "planned";
      }
    } else {
      task.status = previousStatus;
      task.attempts += 1;
      task.lastFailureSignature = `pi-exit-${exitCode}`;
      task.notes = [...(task.notes ?? []), `Openloop ${mode} run failed with exit ${exitCode}.`];
      outcome = "pi-failed";
    }

    const effectivePromotionMode = resolveEffectivePromotionMode(
      task,
      projectPolicy,
      projectConfig.risk.requirePolicyForAutoMerge,
      projectConfig,
    );
    let promotionDecision = decidePromotion(task, validation, effectivePromotionMode, projectConfig);
    if ((reviewResult?.hasBlocking || reviewResult?.malformed || verifierRequiresReview) && promotionDecision === "auto-merge-eligible") {
      promotionDecision = "manual-review";
    }
    const afterFingerprint = await getGitDiffFingerprint(executionPath);
    if (
      shouldStopForNoProgress({
        task,
        previousFailureSignature,
        previousPromotionDecision,
        currentPromotionDecision: promotionDecision,
        beforeFingerprint,
        afterFingerprint,
        noProgressRepeatLimit,
      })
    ) {
      task.status = "blocked";
      task.notes = [...(task.notes ?? []), "Openloop blocked task due to no-progress detection."];
      stoppedBy = "no-progress";
    }

    const promotionAction = decidePromotionAction(promotionDecision);
    const runCost = resolveRunCost(runResult, estimatedCostPerRun);
    // Human-approval packet for anything landing in a promotion queue: the
    // deterministic summary path is computed up front so the packet can
    // reference it before either artifact is written.
    const summaryTimestamp = new Date().toISOString().replace(/[:.]/g, "-");
    const summaryPath = path.join(controlPlanePath, ".openloop", "runs", `${summaryTimestamp}-${task.id}.md`);
    if (promotionAction === "queue-review" || promotionAction === "queue-auto-merge") {
      const diffStat = await getGitDiffStat(executionPath, gitState.currentBranch, null).catch(() => null);
      approvalPacketPath = await writeApprovalPacket(controlPlanePath, task, {
        diffStat,
        validation,
        reviewFindings: reviewResult?.findings ?? [],
        costUsd: runCost.costUsd,
        costSource: runCost.costSource,
        runSummaryPath: summaryPath,
        specPath: task.specId ? path.join(controlPlanePath, task.specId) : null,
      }).catch(() => null);
    }

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
      approvalPacketPath,
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
    await upsertTask(project.path, task);

    const result: SchedulerResult = {
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
      budgetSnapshotUsd: task.estimatedCostUsd ?? null,
      reviewFindings: reviewResult?.findings ?? undefined,
      runSummaryPath: null,
      costUsd: runCost.costUsd,
      costSource: runCost.costSource,
      approvalPacketPath,
    };
    result.runSummaryPath = await writeRunSummary(project.path, result, summaryPath);
    await writeRunTranscript(controlPlanePath, task.id, runResult).catch(() => {});
    await cleanupWorktree();
    return result;
  } catch (error) {
    task.status = previousStatus;
    task.attempts += 1;
    task.lastFailureSignature = error instanceof RunTimeoutError ? "timeout" : error instanceof Error ? error.message : String(error);
    task.notes = [...(task.notes ?? []), `Openloop ${mode} run threw an error.`];
    const afterFingerprint = await getGitDiffFingerprint(executionPath);
    const stoppedBy: SchedulerResult["stoppedBy"] =
      error instanceof RunTimeoutError
        ? "timeout"
        : shouldStopForNoProgress({
              task,
              previousFailureSignature,
              previousPromotionDecision,
              currentPromotionDecision: "blocked",
              beforeFingerprint,
              afterFingerprint,
              noProgressRepeatLimit,
            })
          ? "no-progress"
          : "none";
    if (stoppedBy === "no-progress") {
      task.status = "blocked";
      task.notes = [...(task.notes ?? []), "Openloop blocked task due to no-progress detection."];
    }
    const effectivePromotionMode = resolveEffectivePromotionMode(
      task,
      projectPolicy,
      projectConfig.risk.requirePolicyForAutoMerge,
      projectConfig,
    );
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
    await upsertTask(project.path, task);
    const catchSummaryPath = await writeRunSummary(project.path, {
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
      runSummaryPath: null,
      costUsd: agentStarted ? estimatedCostPerRun : null,
      costSource: agentStarted ? "estimated" : null,
    });
    // A packet written before the failure referenced the (never-written)
    // success-path summary — repoint it at the summary that does exist.
    if (approvalPacketPath) {
      await fixApprovalPacketSummaryPath(approvalPacketPath, catchSummaryPath);
    }
    await cleanupWorktree();
    throw error;
  }
}

async function getInitialGitState(projectPath: string): Promise<{ currentBranch: string | null }> {
  try {
    const state = await getGitWorkingTreeState(projectPath);
    return { currentBranch: state.currentBranch };
  } catch {
    return { currentBranch: null };
  }
}

/** Repoint an approval packet at the run summary that actually exists. */
async function fixApprovalPacketSummaryPath(packetPath: string, summaryPath: string): Promise<void> {
  try {
    const packet = JSON.parse(await fs.readFile(packetPath, "utf8")) as { runSummaryPath?: string | null };
    if (packet.runSummaryPath === summaryPath) {
      return;
    }
    packet.runSummaryPath = summaryPath;
    await fs.writeFile(packetPath, `${JSON.stringify(packet, null, 2)}\n`, "utf8");
  } catch {
    // Packet provenance repair is best-effort.
  }
}

interface VerifierOutcome {
  failed: boolean;
  requiresReview: boolean;
  failingCriteria: string[];
}

interface VerifierVerdict {
  index?: unknown;
  criterion?: unknown;
  verdict?: unknown;
  evidence?: unknown;
}

/**
 * Run the independent verifier and read its verdicts from the control plane.
 * Missing or malformed verdicts never silently pass — they force human review.
 */
async function runVerifierStage(input: {
  task: ProjectTask;
  specContent: string | null;
  controlPlanePath: string;
  model: string | undefined;
  runProject: LinkedProject;
  runner: PiRunner;
  verifierRunner?: (prompt: string) => Promise<number>;
  getRemainingTimeoutMs: () => number | undefined;
}): Promise<VerifierOutcome> {
  const verifierRunner =
    input.verifierRunner ??
    (async (prompt: string) =>
      (
        await input.runner({
          project: input.runProject,
          model: input.model,
          prompt,
          timeoutMs: input.getRemainingTimeoutMs(),
        })
      ).exitCode);

  const prompt = buildVerifierPrompt(input.task, input.specContent, path.join(input.controlPlanePath, ".openloop", "reviews"));
  // Verdicts from a previous attempt are never valid for this one — a
  // verifier that fails to run must degrade to requiresReview, not replay
  // old fail verdicts against a retry it never judged.
  const verdictsPath = path.join(input.controlPlanePath, ".openloop", "verifications", `${input.task.id}.json`);
  await fs.rm(verdictsPath, { force: true }).catch(() => {});
  await verifierRunner(prompt).catch(() => {});

  let verdicts: VerifierVerdict[] = [];
  let missingOrMalformed = true;
  if (await fileExists(verdictsPath)) {
    try {
      const parsed = JSON.parse(await fs.readFile(verdictsPath, "utf8")) as unknown;
      if (Array.isArray(parsed)) {
        verdicts = parsed as VerifierVerdict[];
        missingOrMalformed = false;
      }
    } catch {
      missingOrMalformed = true;
    }
  }

  if (missingOrMalformed) {
    return { failed: false, requiresReview: true, failingCriteria: [] };
  }

  const failingCriteria = verdicts
    .filter((verdict) => verdict.verdict === "fail")
    .map((verdict) =>
      typeof verdict.index === "number" ? String(verdict.index) : typeof verdict.criterion === "string" ? verdict.criterion : "unknown",
    );
  if (failingCriteria.length > 0) {
    return { failed: true, requiresReview: false, failingCriteria };
  }
  if (verdicts.some((verdict) => verdict.verdict === "needs-human")) {
    return { failed: false, requiresReview: true, failingCriteria: [] };
  }
  return { failed: false, requiresReview: false, failingCriteria: [] };
}

function runWorktreeSetupCommand(cwd: string, command: string): Promise<number> {
  const { promise, resolve, reject } = Promise.withResolvers<number>();
  const child = spawn(command, { cwd, stdio: "ignore", shell: true });
  child.on("error", (error) => reject(error));
  child.on("exit", (code) => resolve(code ?? 1));
  return promise;
}

const MAX_TRANSCRIPT_BYTES = 256 * 1024;

/** Persist the agent stdout/stderr tail next to the run summary (control plane). */
async function writeRunTranscript(controlPlanePath: string, taskId: string, result: AgentRunResult): Promise<void> {
  if (!result.stdout && !result.stderr) {
    return;
  }
  const runsDir = path.join(controlPlanePath, ".openloop", "runs");
  await ensureDir(runsDir);
  const text = result.stderr ? `${result.stdout}\n--- stderr ---\n${result.stderr}` : result.stdout;
  const buffer = Buffer.from(text, "utf8");
  const clipped = buffer.length > MAX_TRANSCRIPT_BYTES ? buffer.subarray(buffer.length - MAX_TRANSCRIPT_BYTES) : buffer;
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  await fs.writeFile(path.join(runsDir, `${timestamp}-${taskId}.transcript.log`), clipped);
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

import type { TaskRunSummary } from "./types.js";
