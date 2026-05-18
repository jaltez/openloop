import { describe, it, expect } from "vitest";
import { decidePromotion, decidePromotionAction, resolveEffectivePromotionMode } from "../../src/core/scheduler/promotion.js";
import type { ProjectConfig, ProjectPolicy, ProjectTask, ValidationSummary } from "../../src/core/types.js";

function makeProjectTask(overrides: Partial<ProjectTask> & { id: string }): ProjectTask {
  return {
    title: "Test task",
    kind: "feature",
    status: "done",
    risk: "low-risk",
    scope: null,
    source: { type: "human", ref: "test" },
    specId: null,
    branch: null,
    owner: null,
    acceptanceCriteria: [],
    attempts: 0,
    lastFailureSignature: null,
    promotion: "auto-merge",
    notes: [],
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    ...overrides,
  };
}

function makeProjectPolicy(overrides?: Partial<ProjectPolicy>): ProjectPolicy {
  return {
    version: 1,
    scope: { allowGlobs: ["src/**"], denyGlobs: [], highRiskAreas: [] },
    riskClasses: {
      "low-risk": { autoMergeAllowed: true, requiresHumanReview: false },
      "medium-risk": { autoMergeAllowed: false, requiresHumanReview: true },
      "high-risk": { autoMergeAllowed: false, requiresHumanReview: true },
    },
    selfHealing: { enabled: true, allowedTaskKinds: ["lint-fix", "type-fix", "localized-test-fix"] },
    promotion: {
      lowRiskMode: "auto-merge",
      mediumRiskMode: "pull-request",
      highRiskMode: "pull-request",
    },
    ...overrides,
  };
}

function makeProjectConfig(overrides?: Partial<ProjectConfig>): ProjectConfig {
  return {
    version: 1,
    project: { alias: null, repoRoot: null, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: "npm run lint", testCommand: "npm test", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    ...overrides,
  };
}

const passingValidation: ValidationSummary[] = [
  { name: "lint", command: "npm run lint", exitCode: 0 },
];

const failingValidation: ValidationSummary[] = [
  { name: "lint", command: "npm run lint", exitCode: 1 },
];

describe("decidePromotion", () => {
  it("returns 'blocked' when validation fails", () => {
    const task = makeProjectTask({ id: "t1", status: "done" });
    const config = makeProjectConfig();
    const result = decidePromotion(task, failingValidation, "auto-merge", config);
    expect(result).toBe("blocked");
  });

  it("returns 'none' when task is not done", () => {
    const task = makeProjectTask({ id: "t1", status: "in_progress" });
    const config = makeProjectConfig();
    const result = decidePromotion(task, passingValidation, "auto-merge", config);
    expect(result).toBe("none");
  });

  it("returns 'auto-merge-eligible' for low-risk done task with passing validations", () => {
    const task = makeProjectTask({ id: "t1", status: "done", risk: "low-risk" });
    const config = makeProjectConfig({
      validation: { lintCommand: "npm run lint", testCommand: "npm test", typecheckCommand: null },
    });
    const multiValidation: ValidationSummary[] = [
      { name: "lint", command: "npm run lint", exitCode: 0 },
      { name: "test", command: "npm test", exitCode: 0 },
    ];
    const result = decidePromotion(task, multiValidation, "auto-merge", config);
    expect(result).toBe("auto-merge-eligible");
  });

  it("returns 'manual-review' for non-auto-merge promotion mode", () => {
    const task = makeProjectTask({ id: "t1", status: "done", risk: "medium-risk" });
    const config = makeProjectConfig();
    const result = decidePromotion(task, passingValidation, "pull-request", config);
    expect(result).toBe("manual-review");
  });

  it("returns 'manual-review' when no validations are configured (S3 safety gate)", () => {
    const task = makeProjectTask({ id: "t1", status: "done", risk: "low-risk" });
    const config = makeProjectConfig({
      validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    });
    const result = decidePromotion(task, [], "auto-merge", config);
    expect(result).toBe("manual-review");
  });
});

describe("decidePromotionAction", () => {
  it("maps auto-merge-eligible to queue-auto-merge", () => {
    expect(decidePromotionAction("auto-merge-eligible")).toBe("queue-auto-merge");
  });

  it("maps manual-review to queue-review", () => {
    expect(decidePromotionAction("manual-review")).toBe("queue-review");
  });

  it("maps blocked to block", () => {
    expect(decidePromotionAction("blocked")).toBe("block");
  });

  it("maps none to none", () => {
    expect(decidePromotionAction("none")).toBe("none");
  });
});

describe("resolveEffectivePromotionMode", () => {
  it("returns manual-only when task promotion is manual-only", () => {
    const task = makeProjectTask({ id: "t1", promotion: "manual-only" });
    const policy = makeProjectPolicy();
    const result = resolveEffectivePromotionMode(task, policy, false);
    expect(result).toBe("manual-only");
  });

  it("returns manual-only when policy overrides to manual-only", () => {
    const task = makeProjectTask({ id: "t1", risk: "low-risk", promotion: "auto-merge" });
    const policy = makeProjectPolicy({
      promotion: {
        lowRiskMode: "manual-only",
        mediumRiskMode: "pull-request",
        highRiskMode: "pull-request",
      },
    });
    const result = resolveEffectivePromotionMode(task, policy, false);
    expect(result).toBe("manual-only");
  });

  it("returns pull-request for medium-risk tasks", () => {
    const task = makeProjectTask({ id: "t1", risk: "medium-risk", promotion: "auto-merge" });
    const policy = makeProjectPolicy();
    const result = resolveEffectivePromotionMode(task, policy, false);
    expect(result).toBe("pull-request");
  });

  it("returns pull-request for high-risk tasks", () => {
    const task = makeProjectTask({ id: "t1", risk: "high-risk", promotion: "auto-merge" });
    const policy = makeProjectPolicy();
    const result = resolveEffectivePromotionMode(task, policy, false);
    expect(result).toBe("pull-request");
  });

  it("returns pull-request when requirePolicyForAutoMerge is true and autoMergeAllowed is false", () => {
    const task = makeProjectTask({ id: "t1", risk: "low-risk", promotion: "auto-merge" });
    const policy = makeProjectPolicy({
      riskClasses: {
        "low-risk": { autoMergeAllowed: false, requiresHumanReview: true },
        "medium-risk": { autoMergeAllowed: false, requiresHumanReview: true },
        "high-risk": { autoMergeAllowed: false, requiresHumanReview: true },
      },
    });
    const result = resolveEffectivePromotionMode(task, policy, true);
    expect(result).toBe("pull-request");
  });

  it("returns auto-merge for low-risk task when policy allows it", () => {
    const task = makeProjectTask({ id: "t1", risk: "low-risk", promotion: "auto-merge" });
    const policy = makeProjectPolicy();
    const result = resolveEffectivePromotionMode(task, policy, true);
    expect(result).toBe("auto-merge");
  });
});
