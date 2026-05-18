import { describe, it, expect } from "vitest";
import { getSelfHealingBlock, determineWorkerRole, isSupportedSelfHealingTask, isSelfHealingTask, describeSelfHealingTask } from "../../src/core/scheduler/self-healing.js";
import type { ProjectPolicy, ProjectTask } from "../../src/core/types.js";

function makePolicy(overrides?: Partial<ProjectPolicy>): ProjectPolicy {
  return {
    version: 1,
    scope: { allowGlobs: [], denyGlobs: [], highRiskAreas: [] },
    riskClasses: {
      "low-risk": { autoMergeAllowed: true, requiresHumanReview: false },
      "medium-risk": { autoMergeAllowed: false, requiresHumanReview: true },
      "high-risk": { autoMergeAllowed: false, requiresHumanReview: true },
    },
    selfHealing: {
      enabled: true,
      allowedTaskKinds: ["lint-fix", "type-fix", "localized-test-fix"],
    },
    promotion: {
      lowRiskMode: "auto-merge",
      mediumRiskMode: "pull-request",
      highRiskMode: "pull-request",
    },
    ...overrides,
  };
}

function makeTask(overrides: Partial<ProjectTask> & { id: string }): ProjectTask {
  return {
    title: "Test task",
    kind: "feature",
    status: "ready",
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

describe("isSelfHealingTask", () => {
  it("returns true for lint-fix", () => {
    expect(isSelfHealingTask("lint-fix")).toBe(true);
  });

  it("returns true for type-fix", () => {
    expect(isSelfHealingTask("type-fix")).toBe(true);
  });

  it("returns true for localized-test-fix", () => {
    expect(isSelfHealingTask("localized-test-fix")).toBe(true);
  });

  it("returns true for ci-heal", () => {
    expect(isSelfHealingTask("ci-heal")).toBe(true);
  });

  it("returns false for feature", () => {
    expect(isSelfHealingTask("feature")).toBe(false);
  });

  it("returns false for bugfix", () => {
    expect(isSelfHealingTask("bugfix")).toBe(false);
  });
});

describe("isSupportedSelfHealingTask", () => {
  it("returns true for lint-fix", () => {
    expect(isSupportedSelfHealingTask("lint-fix")).toBe(true);
  });

  it("returns true for type-fix", () => {
    expect(isSupportedSelfHealingTask("type-fix")).toBe(true);
  });

  it("returns true for localized-test-fix", () => {
    expect(isSupportedSelfHealingTask("localized-test-fix")).toBe(true);
  });

  it("returns false for ci-heal (not in SUPPORTED list)", () => {
    expect(isSupportedSelfHealingTask("ci-heal")).toBe(false);
  });
});

describe("getSelfHealingBlock", () => {
  it("returns null for non-self-healing tasks", () => {
    const task = makeTask({ id: "t1", kind: "feature" });
    expect(getSelfHealingBlock(task, makePolicy())).toBeNull();
  });

  it("returns block when self-healing is disabled", () => {
    const task = makeTask({ id: "t1", kind: "lint-fix" });
    const policy = makePolicy({
      selfHealing: { enabled: false, allowedTaskKinds: [] },
    });
    const block = getSelfHealingBlock(task, policy);
    expect(block).not.toBeNull();
    expect(block!.failureSignature).toBe("self-healing-disabled");
    expect(block!.reason).toContain("disabled");
  });

  it("returns block for unsupported kind (ci-heal)", () => {
    const task = makeTask({ id: "t1", kind: "ci-heal" });
    const policy = makePolicy();
    const block = getSelfHealingBlock(task, policy);
    expect(block).not.toBeNull();
    expect(block!.failureSignature).toBe("self-healing-kind-not-supported");
    expect(block!.reason).toContain("outside the V1 scope");
  });

  it("returns block when kind is not in allowed list", () => {
    const task = makeTask({ id: "t1", kind: "lint-fix" });
    const policy = makePolicy({
      selfHealing: { enabled: true, allowedTaskKinds: ["type-fix"] },
    });
    const block = getSelfHealingBlock(task, policy);
    expect(block).not.toBeNull();
    expect(block!.failureSignature).toBe("self-healing-kind-not-allowed");
  });

  it("returns null when self-healing is enabled and kind is allowed", () => {
    const task = makeTask({ id: "t1", kind: "lint-fix" });
    const policy = makePolicy();
    expect(getSelfHealingBlock(task, policy)).toBeNull();
  });
});

describe("determineWorkerRole", () => {
  it("returns repo-improver for discovery tasks", () => {
    const task = makeTask({ id: "t1", kind: "discovery" });
    expect(determineWorkerRole(task, "plan")).toBe("repo-improver");
  });

  it("returns repo-improver for scope-proposal tasks", () => {
    const task = makeTask({ id: "t1", kind: "scope-proposal" });
    expect(determineWorkerRole(task, "plan")).toBe("repo-improver");
  });

  it("returns ci-healer for lint-fix tasks", () => {
    const task = makeTask({ id: "t1", kind: "lint-fix" });
    expect(determineWorkerRole(task, "implement")).toBe("ci-healer");
  });

  it("returns ci-healer for type-fix tasks", () => {
    const task = makeTask({ id: "t1", kind: "type-fix" });
    expect(determineWorkerRole(task, "implement")).toBe("ci-healer");
  });

  it("returns sdd-planner for plan mode", () => {
    const task = makeTask({ id: "t1", kind: "feature" });
    expect(determineWorkerRole(task, "plan")).toBe("sdd-planner");
  });

  it("returns implementer for implement mode feature task", () => {
    const task = makeTask({ id: "t1", kind: "feature" });
    expect(determineWorkerRole(task, "implement")).toBe("implementer");
  });
});

describe("describeSelfHealingTask", () => {
  it("describes lint-fix", () => {
    expect(describeSelfHealingTask("lint-fix")).toBe("lint issue");
  });

  it("describes type-fix", () => {
    expect(describeSelfHealingTask("type-fix")).toBe("type issue");
  });

  it("describes localized-test-fix", () => {
    expect(describeSelfHealingTask("localized-test-fix")).toBe("localized deterministic test failure");
  });
});
