import fs from "node:fs/promises";
import path from "node:path";
import { parse } from "yaml";
import { SUPPORTED_SELF_HEALING_TASK_KINDS, type ProjectPolicy, type ProjectTask } from "./types.js";

export interface ScopePolicyDecision {
  blocked: boolean;
  adjustedRisk: ProjectTask["risk"];
  reason: string | null;
  note: string | null;
  failureSignature: string | null;
}

export const DEFAULT_PROJECT_POLICY: ProjectPolicy = {
  version: 1,
  scope: {
    allowGlobs: [],
    denyGlobs: [],
    highRiskAreas: [],
  },
  riskClasses: {
    "low-risk": {
      autoMergeAllowed: true,
      requiresHumanReview: false,
    },
    "medium-risk": {
      autoMergeAllowed: false,
      requiresHumanReview: true,
    },
    "high-risk": {
      autoMergeAllowed: false,
      requiresHumanReview: true,
    },
  },
  selfHealing: {
    enabled: true,
    allowedTaskKinds: [...SUPPORTED_SELF_HEALING_TASK_KINDS],
  },
  promotion: {
    lowRiskMode: "auto-merge",
    mediumRiskMode: "pull-request",
    highRiskMode: "pull-request",
  },
};

export async function loadProjectPolicy(projectPath: string): Promise<ProjectPolicy> {
  const policyPath = path.join(projectPath, ".openloop", "policy.yaml");
  try {
    const raw = await fs.readFile(policyPath, "utf8");
    const parsed = parse(raw) as Partial<ProjectPolicy> | null;
    return mergePolicy(parsed);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      return structuredClone(DEFAULT_PROJECT_POLICY);
    }
    throw error;
  }
}

export function evaluateTaskScopePolicy(
  task: ProjectTask,
  policy: ProjectPolicy,
  defaultUnknownAreaClassification: ProjectTask["risk"],
): ScopePolicyDecision {
  const scopePaths = normalizeScopePaths(task.scope?.paths ?? []);
  const hasScopeRules = policy.scope.allowGlobs.length > 0 || policy.scope.denyGlobs.length > 0 || policy.scope.highRiskAreas.length > 0;

  if (scopePaths.length === 0) {
    if (!hasScopeRules) {
      return {
        blocked: false,
        adjustedRisk: task.risk,
        reason: null,
        note: null,
        failureSignature: null,
      };
    }

    const adjustedRisk = maxRisk(task.risk, defaultUnknownAreaClassification);
    if (adjustedRisk === task.risk) {
      return {
        blocked: false,
        adjustedRisk,
        reason: null,
        note: null,
        failureSignature: null,
      };
    }

    return {
      blocked: false,
      adjustedRisk,
      reason: null,
      note: `Openloop classified task scope conservatively as ${adjustedRisk} because the task did not declare scope paths while project scope rules are enabled.`,
      failureSignature: null,
    };
  }

  const deniedPaths = scopePaths.filter((candidate) => matchesAnyGlob(candidate, policy.scope.denyGlobs));
  if (deniedPaths.length > 0) {
    return {
      blocked: true,
      adjustedRisk: task.risk,
      reason: `task scope hits denied paths: ${deniedPaths.join(", ")}`,
      note: `Openloop blocked task scope because it targets denied paths: ${deniedPaths.join(", ")}.`,
      failureSignature: "policy-scope-denied",
    };
  }

  if (policy.scope.allowGlobs.length > 0) {
    const outsideAllowlist = scopePaths.filter((candidate) => !matchesAnyGlob(candidate, policy.scope.allowGlobs));
    if (outsideAllowlist.length > 0) {
      return {
        blocked: true,
        adjustedRisk: task.risk,
        reason: `task scope is outside allowed unattended paths: ${outsideAllowlist.join(", ")}`,
        note: `Openloop blocked task scope because it falls outside the unattended allowlist: ${outsideAllowlist.join(", ")}.`,
        failureSignature: "policy-scope-not-allowlisted",
      };
    }
  }

  const highRiskMatches = scopePaths.filter((candidate) => matchesAnyGlob(candidate, policy.scope.highRiskAreas));
  if (highRiskMatches.length > 0) {
    return {
      blocked: false,
      adjustedRisk: maxRisk(task.risk, "high-risk"),
      reason: null,
      note: `Openloop escalated task risk to high-risk because scope touches high-risk areas: ${highRiskMatches.join(", ")}.`,
      failureSignature: null,
    };
  }

  return {
    blocked: false,
    adjustedRisk: task.risk,
    reason: null,
    note: null,
    failureSignature: null,
  };
}

function mergePolicy(policy: Partial<ProjectPolicy> | null): ProjectPolicy {
  const configuredAllowedTaskKinds = policy?.selfHealing?.allowedTaskKinds ?? DEFAULT_PROJECT_POLICY.selfHealing.allowedTaskKinds;

  return {
    version: 1,
    scope: {
      allowGlobs: policy?.scope?.allowGlobs ?? [],
      denyGlobs: policy?.scope?.denyGlobs ?? [],
      highRiskAreas: policy?.scope?.highRiskAreas ?? [],
    },
    riskClasses: {
      "low-risk": {
        autoMergeAllowed: policy?.riskClasses?.["low-risk"]?.autoMergeAllowed ?? DEFAULT_PROJECT_POLICY.riskClasses["low-risk"].autoMergeAllowed,
        requiresHumanReview: policy?.riskClasses?.["low-risk"]?.requiresHumanReview ?? DEFAULT_PROJECT_POLICY.riskClasses["low-risk"].requiresHumanReview,
      },
      "medium-risk": {
        autoMergeAllowed: policy?.riskClasses?.["medium-risk"]?.autoMergeAllowed ?? DEFAULT_PROJECT_POLICY.riskClasses["medium-risk"].autoMergeAllowed,
        requiresHumanReview: policy?.riskClasses?.["medium-risk"]?.requiresHumanReview ?? DEFAULT_PROJECT_POLICY.riskClasses["medium-risk"].requiresHumanReview,
      },
      "high-risk": {
        autoMergeAllowed: policy?.riskClasses?.["high-risk"]?.autoMergeAllowed ?? DEFAULT_PROJECT_POLICY.riskClasses["high-risk"].autoMergeAllowed,
        requiresHumanReview: policy?.riskClasses?.["high-risk"]?.requiresHumanReview ?? DEFAULT_PROJECT_POLICY.riskClasses["high-risk"].requiresHumanReview,
      },
    },
    selfHealing: {
      enabled: policy?.selfHealing?.enabled ?? DEFAULT_PROJECT_POLICY.selfHealing.enabled,
      allowedTaskKinds: configuredAllowedTaskKinds.filter((kind): kind is ProjectPolicy["selfHealing"]["allowedTaskKinds"][number] =>
        SUPPORTED_SELF_HEALING_TASK_KINDS.includes(kind as ProjectPolicy["selfHealing"]["allowedTaskKinds"][number]),
      ),
    },
    promotion: {
      lowRiskMode: policy?.promotion?.lowRiskMode ?? DEFAULT_PROJECT_POLICY.promotion.lowRiskMode,
      mediumRiskMode: policy?.promotion?.mediumRiskMode ?? DEFAULT_PROJECT_POLICY.promotion.mediumRiskMode,
      highRiskMode: policy?.promotion?.highRiskMode ?? DEFAULT_PROJECT_POLICY.promotion.highRiskMode,
    },
  };
}

function normalizeScopePaths(paths: string[]): string[] {
  return [...new Set(paths.map(normalizeScopePath).filter((candidate) => candidate.length > 0))];
}

function normalizeScopePath(value: string): string {
  return value.replace(/\\/g, "/").replace(/^\.\//, "").replace(/^\//, "");
}

function matchesAnyGlob(candidate: string, globs: string[]): boolean {
  return globs.some((pattern) => matchesGlob(candidate, pattern));
}

function matchesGlob(candidate: string, pattern: string): boolean {
  const normalizedPattern = normalizeScopePath(pattern);
  if (normalizedPattern.length === 0) {
    return false;
  }

  const expression = globToRegExp(normalizedPattern);
  return expression.test(candidate);
}

function globToRegExp(pattern: string): RegExp {
  let expression = "^";

  for (let index = 0; index < pattern.length; index += 1) {
    const current = pattern[index];
    const next = pattern[index + 1];

    if (current === "*" && next === "*") {
      expression += ".*";
      index += 1;
      continue;
    }

    if (current === "*") {
      expression += "[^/]*";
      continue;
    }

    if (current === "?") {
      expression += "[^/]";
      continue;
    }

    expression += escapeRegExp(current);
  }

  expression += "$";
  return new RegExp(expression);
}

function escapeRegExp(value: string): string {
  return value.replace(/[|\\{}()[\]^$+?.]/g, "\\$&");
}

function maxRisk(left: ProjectTask["risk"], right: ProjectTask["risk"]): ProjectTask["risk"] {
  if (riskRank(left) >= riskRank(right)) {
    return left;
  }
  return right;
}

function riskRank(risk: ProjectTask["risk"]): number {
  if (risk === "low-risk") {
    return 0;
  }
  if (risk === "medium-risk") {
    return 1;
  }
  return 2;
}