import type { ProjectConfig, ProjectPolicy, ProjectTask, SchedulerResult, ValidationSummary } from "../types.js";
import { getConfiguredValidationNames, hasAllRequiredAutoMergeValidations } from "../validation-utils.js";

export function decidePromotion(
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

export function decidePromotionAction(decision: SchedulerResult["promotionDecision"]): SchedulerResult["promotionAction"] {
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

export function resolveEffectivePromotionMode(
  task: ProjectTask,
  projectPolicy: ProjectPolicy,
  requirePolicyForAutoMerge: boolean,
  _projectConfig?: ProjectConfig,
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
