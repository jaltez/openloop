import { describe, it, expect } from "vitest";
import { shouldStopForNoProgress } from "../../src/core/scheduler/no-progress.js";
import type { ProjectTask } from "../../src/core/types.js";

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

describe("shouldStopForNoProgress", () => {
  it("does not block when fingerprints differ", () => {
    const task = makeTask({ id: "t1" });
    const result = shouldStopForNoProgress({
      task,
      previousFailureSignature: null,
      previousPromotionDecision: null,
      currentPromotionDecision: "auto-merge-eligible",
      beforeFingerprint: "abc",
      afterFingerprint: "def",
      noProgressRepeatLimit: 3,
    });
    expect(result).toBe(false);
  });

  it("does not block on first unchanged diff", () => {
    const task = makeTask({ id: "t1" });
    const result = shouldStopForNoProgress({
      task,
      previousFailureSignature: null,
      previousPromotionDecision: null,
      currentPromotionDecision: "auto-merge-eligible",
      beforeFingerprint: "abc",
      afterFingerprint: "abc",
      noProgressRepeatLimit: 3,
    });
    expect(result).toBe(false);
    // But should have added a note
    expect(task.notes).toHaveLength(1);
    expect(task.notes![0]).toContain("diff-unchanged:1");
  });

  it("blocks after reaching the no-progress limit for unchanged diffs", () => {
    const task = makeTask({ id: "t1" });
    const limit = 2;

    // First check
    shouldStopForNoProgress({
      task,
      previousFailureSignature: null,
      previousPromotionDecision: null,
      currentPromotionDecision: "auto-merge-eligible",
      beforeFingerprint: "abc",
      afterFingerprint: "abc",
      noProgressRepeatLimit: limit,
    });

    // Second check — should now block
    const result = shouldStopForNoProgress({
      task,
      previousFailureSignature: null,
      previousPromotionDecision: null,
      currentPromotionDecision: "auto-merge-eligible",
      beforeFingerprint: "abc",
      afterFingerprint: "abc",
      noProgressRepeatLimit: limit,
    });

    expect(result).toBe(true);
  });

  it("detects repeated failure signatures", () => {
    const task = makeTask({ id: "t1", lastFailureSignature: "validation-test" });
    const result = shouldStopForNoProgress({
      task,
      previousFailureSignature: "validation-test",
      previousPromotionDecision: null,
      currentPromotionDecision: "blocked",
      beforeFingerprint: "abc",
      afterFingerprint: "def",
      noProgressRepeatLimit: 1,
    });
    expect(result).toBe(true);
    expect(task.notes!.some((n) => n.includes("failure-validation-test:1"))).toBe(true);
  });

  it("detects repeated promotion-blocked decisions", () => {
    const task = makeTask({ id: "t1" });
    const result = shouldStopForNoProgress({
      task,
      previousFailureSignature: null,
      previousPromotionDecision: "blocked",
      currentPromotionDecision: "blocked",
      beforeFingerprint: "abc",
      afterFingerprint: "def",
      noProgressRepeatLimit: 1,
    });
    expect(result).toBe(true);
    expect(task.notes!.some((n) => n.includes("promotion-blocked:1"))).toBe(true);
  });

  it("accumulates multiple observation types in a single check", () => {
    const task = makeTask({ id: "t1", lastFailureSignature: "validation-test" });
    shouldStopForNoProgress({
      task,
      previousFailureSignature: "validation-test",
      previousPromotionDecision: "blocked",
      currentPromotionDecision: "blocked",
      beforeFingerprint: "abc",
      afterFingerprint: "abc",
      noProgressRepeatLimit: 3,
    });
    // Should have 3 observation notes: failure, promotion-blocked, diff-unchanged
    expect(task.notes!.length).toBeGreaterThanOrEqual(3);
  });
});
