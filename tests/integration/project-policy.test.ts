import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { evaluateTaskScopePolicy, loadProjectPolicy } from "../../src/core/project-policy.js";

test("loadProjectPolicy narrows self-healing allowlist to supported task kinds", async () => {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-policy-self-heal-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "policy.yaml"),
    [
      "version: 1",
      "selfHealing:",
      "  enabled: true",
      "  allowedTaskKinds:",
      "    - lint-fix",
      "    - localized-test-fix",
      "    - ci-heal",
      "    - type-fix",
    ].join("\n") + "\n",
    "utf8",
  );

  const policy = await loadProjectPolicy(projectRoot);

  expect(policy.selfHealing.allowedTaskKinds).toEqual(["lint-fix", "localized-test-fix", "type-fix"]);
});

test("evaluateTaskScopePolicy blocks denied scope paths", () => {
  const decision = evaluateTaskScopePolicy(
    {
      id: "task-denied",
      title: "Denied scope",
      kind: "feature",
      status: "ready",
      risk: "low-risk",
      scope: { paths: ["src/secrets/token.ts"] },
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: ["blocked"],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "auto-merge",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    },
    {
      version: 1,
      scope: {
        allowGlobs: ["src/**"],
        denyGlobs: ["src/secrets/**"],
        highRiskAreas: [],
      },
      riskClasses: {
        "low-risk": { autoMergeAllowed: true, requiresHumanReview: false },
        "medium-risk": { autoMergeAllowed: false, requiresHumanReview: true },
        "high-risk": { autoMergeAllowed: false, requiresHumanReview: true },
      },
      selfHealing: { enabled: true, allowedTaskKinds: ["lint-fix", "type-fix", "localized-test-fix"] },
      promotion: { lowRiskMode: "auto-merge", mediumRiskMode: "pull-request", highRiskMode: "pull-request" },
    },
    "medium-risk",
  );

  expect(decision.blocked).toBe(true);
  expect(decision.failureSignature).toBe("policy-scope-denied");
});

test("evaluateTaskScopePolicy escalates unknown scope conservatively when scope rules exist", () => {
  const decision = evaluateTaskScopePolicy(
    {
      id: "task-unknown",
      title: "Unknown scope",
      kind: "feature",
      status: "ready",
      risk: "low-risk",
      scope: null,
      source: { type: "human", ref: "test" },
      specId: null,
      branch: null,
      owner: null,
      acceptanceCriteria: ["classified"],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "auto-merge",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    },
    {
      version: 1,
      scope: {
        allowGlobs: ["src/**"],
        denyGlobs: [],
        highRiskAreas: ["src/payments/**"],
      },
      riskClasses: {
        "low-risk": { autoMergeAllowed: true, requiresHumanReview: false },
        "medium-risk": { autoMergeAllowed: false, requiresHumanReview: true },
        "high-risk": { autoMergeAllowed: false, requiresHumanReview: true },
      },
      selfHealing: { enabled: true, allowedTaskKinds: ["lint-fix", "type-fix", "localized-test-fix"] },
      promotion: { lowRiskMode: "auto-merge", mediumRiskMode: "pull-request", highRiskMode: "pull-request" },
    },
    "medium-risk",
  );

  expect(decision.blocked).toBe(false);
  expect(decision.adjustedRisk).toBe("medium-risk");
});