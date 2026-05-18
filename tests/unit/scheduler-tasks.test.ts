import { describe, it, expect } from "vitest";
import { buildPrompt } from "../../src/core/scheduler/tasks.js";
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
    acceptanceCriteria: ["Do the thing", "Pass tests"],
    attempts: 0,
    lastFailureSignature: null,
    promotion: "auto-merge",
    notes: [],
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    ...overrides,
  };
}

describe("buildPrompt", () => {
  it("includes task metadata in implement mode", () => {
    const task = makeTask({ id: "t1", title: "Add validation" });
    const prompt = buildPrompt(task, "implement", "implementer");

    expect(prompt).toContain("Implement the following task.");
    expect(prompt).toContain("Task ID: t1");
    expect(prompt).toContain("Title: Add validation");
    expect(prompt).toContain("Kind: feature");
    expect(prompt).toContain("Assigned Role: implementer");
    expect(prompt).toContain("Risk: low-risk");
    expect(prompt).toContain("Source: human / test");
  });

  it("uses plan-mode header for plan tasks", () => {
    const task = makeTask({ id: "t2" });
    const prompt = buildPrompt(task, "plan", "sdd-planner");

    expect(prompt).toContain("Plan the following task and prepare it for implementation.");
    expect(prompt).toContain("Spec Output: Write your implementation plan to .openloop/specs/t2.md");
  });

  it("includes scope paths when present", () => {
    const task = makeTask({ id: "t3", scope: { paths: ["src/auth.ts", "tests/auth.test.ts"] } });
    const prompt = buildPrompt(task, "implement", "implementer");

    expect(prompt).toContain("Scope Paths:");
    expect(prompt).toContain("- src/auth.ts");
    expect(prompt).toContain("- tests/auth.test.ts");
  });

  it("includes acceptance criteria", () => {
    const task = makeTask({ id: "t4" });
    const prompt = buildPrompt(task, "implement", "implementer");

    expect(prompt).toContain("Acceptance Criteria:");
    expect(prompt).toContain("- Do the thing");
    expect(prompt).toContain("- Pass tests");
  });

  it("includes spec content for implement mode when provided", () => {
    const task = makeTask({ id: "t5" });
    const prompt = buildPrompt(task, "implement", "implementer", "## Plan\n1. Add validation\n2. Write tests");

    expect(prompt).toContain("## Implementation Spec");
    expect(prompt).toContain("## Plan");
    expect(prompt).toContain("1. Add validation");
  });

  it("does not include spec content in plan mode", () => {
    const task = makeTask({ id: "t6" });
    const prompt = buildPrompt(task, "plan", "sdd-planner", "some spec content");

    expect(prompt).not.toContain("## Implementation Spec");
  });

  it("uses self-healing header for lint-fix tasks", () => {
    const task = makeTask({ id: "t7", kind: "lint-fix" });
    const prompt = buildPrompt(task, "implement", "ci-healer");

    expect(prompt).toContain("Repair the following lint issue");
    expect(prompt).toContain("Self-Healing Scope:");
  });

  it("uses self-healing header for type-fix tasks", () => {
    const task = makeTask({ id: "t8", kind: "type-fix" });
    const prompt = buildPrompt(task, "implement", "ci-healer");

    expect(prompt).toContain("Repair the following type issue");
  });

  it("uses self-healing header for localized-test-fix tasks", () => {
    const task = makeTask({ id: "t9", kind: "localized-test-fix" });
    const prompt = buildPrompt(task, "implement", "ci-healer");

    expect(prompt).toContain("Repair the following localized deterministic test failure");
  });
});
