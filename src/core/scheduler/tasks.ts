import fs from "node:fs/promises";
import path from "node:path";
import type { ProjectConfig, ProjectPolicy, ProjectTask, TaskLedger } from "../types.js";
import { getConfiguredValidationNames } from "../validation-utils.js";
import { isSupportedSelfHealingTask, describeSelfHealingTask } from "./self-healing.js";
import type { WorkerRole } from "../types.js";

export function synthesizeContinuousImprovementTask(
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

export async function readSpecContent(projectPath: string, task: ProjectTask): Promise<string | null> {
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

export async function detectAndSetSpecId(projectPath: string, task: ProjectTask): Promise<void> {
  const specPath = path.join(".openloop", "specs", `${task.id}.md`);
  const fullPath = path.join(projectPath, specPath);
  try {
    await fs.access(fullPath);
    task.specId = specPath;
  } catch {
    // no spec file written by Pi — that's OK
  }
}
