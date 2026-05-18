import { SUPPORTED_SELF_HEALING_TASK_KINDS, type ProjectPolicy, type ProjectTask, type WorkerRole } from "../types.js";

export function getSelfHealingBlock(task: ProjectTask, projectPolicy: ProjectPolicy): {
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

export function isSelfHealingTask(kind: ProjectTask["kind"]): boolean {
  return kind === "lint-fix" || kind === "type-fix" || kind === "localized-test-fix" || kind === "ci-heal";
}

export function isSupportedSelfHealingTask(kind: ProjectTask["kind"]): kind is (typeof SUPPORTED_SELF_HEALING_TASK_KINDS)[number] {
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

export function describeSelfHealingTask(kind: (typeof SUPPORTED_SELF_HEALING_TASK_KINDS)[number]): string {
  if (kind === "lint-fix") {
    return "lint issue";
  }
  if (kind === "type-fix") {
    return "type issue";
  }
  return "localized deterministic test failure";
}
