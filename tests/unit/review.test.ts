import { expect, test } from "vitest";
import { checkSecrets, checkScopeDrift, extractChangedFiles } from "../../src/core/review.js";
import type { ProjectPolicy, ProjectTask } from "../../src/core/types.js";

const baseTask: ProjectTask = {
  id: "t1",
  title: "Test task",
  kind: "feature",
  status: "ready",
  risk: "low-risk",
  source: { type: "human", ref: "test" },
  scope: { paths: ["src/**"] },
  specId: null,
  branch: null,
  owner: null,
  acceptanceCriteria: [],
  attempts: 0,
  lastFailureSignature: null,
  promotion: "auto-merge",
  notes: [],
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

const basePolicy: ProjectPolicy = {
  version: 1,
  scope: { allowGlobs: [], denyGlobs: ["secrets/**"], highRiskAreas: ["src/auth/**"] },
  riskClasses: {
    "low-risk": { autoMergeAllowed: true, requiresHumanReview: false },
    "medium-risk": { autoMergeAllowed: false, requiresHumanReview: true },
    "high-risk": { autoMergeAllowed: false, requiresHumanReview: true },
  },
  selfHealing: { enabled: true, allowedTaskKinds: ["lint-fix", "type-fix", "localized-test-fix"] },
  promotion: { lowRiskMode: "auto-merge", mediumRiskMode: "pull-request", highRiskMode: "pull-request" },
};

// --- checkSecrets ---

test("checkSecrets detects AWS access key IDs in added lines", () => {
  const diff = "+const key = \"AKIAIOSFODNN7EXAMPLE\";\n-const old = \"nothing\";";
  const findings = checkSecrets(diff);
  expect(findings.some((f) => f.rule === "secret-detection:aws-access-key-id" && f.severity === "block")).toBe(true);
});

test("checkSecrets detects GitHub tokens in added lines", () => {
  const diff = "+token = \"ghp_1234567890abcdefghijklmnopqrstuvwxyz\";";
  const findings = checkSecrets(diff);
  expect(findings.some((f) => f.rule === "secret-detection:github-token")).toBe(true);
});

test("checkSecrets detects private key blocks", () => {
  const diff = "+-----BEGIN RSA PRIVATE KEY-----\n+MIIEpAIBAAKCAQEA...";
  const findings = checkSecrets(diff);
  expect(findings.some((f) => f.rule === "secret-detection:private-key-block")).toBe(true);
});

test("checkSecrets ignores deletion lines", () => {
  const diff = "-const key = \"AKIAIOSFODNN7EXAMPLE\";";
  const findings = checkSecrets(diff);
  expect(findings).toHaveLength(0);
});

test("checkSecrets returns empty for clean diffs", () => {
  const diff = "+const greeting = \"hello world\";\n+export default greeting;";
  const findings = checkSecrets(diff);
  expect(findings).toHaveLength(0);
});

// --- checkScopeDrift ---

test("checkScopeDrift blocks on deny-glob violations", () => {
  const findings = checkScopeDrift(["secrets/env.json"], baseTask, basePolicy);
  expect(findings.some((f) => f.rule === "deny-glob-violation" && f.severity === "block")).toBe(true);
});

test("checkScopeDrift warns on high-risk area touches", () => {
  const findings = checkScopeDrift(["src/auth/login.ts"], baseTask, basePolicy);
  expect(findings.some((f) => f.rule === "high-risk-area-touched" && f.severity === "warn")).toBe(true);
});

test("checkScopeDrift warns on files outside declared scope", () => {
  const findings = checkScopeDrift(["README.md"], baseTask, basePolicy);
  expect(findings.some((f) => f.rule === "scope-drift" && f.severity === "warn")).toBe(true);
});

test("checkScopeDrift finds nothing when changes stay in scope", () => {
  const findings = checkScopeDrift(["src/index.ts", "src/utils.ts"], baseTask, basePolicy);
  expect(findings).toHaveLength(0);
});

test("checkScopeDrift does not flag scope drift when task has no declared scope", () => {
  const task = { ...baseTask, scope: null };
  const findings = checkScopeDrift(["README.md"], task, basePolicy);
  expect(findings.some((f) => f.rule === "scope-drift")).toBe(false);
});

// --- extractChangedFiles ---

test("extractChangedFiles parses diff --git headers", () => {
  const patch = [
    "diff --git a/src/foo.ts b/src/foo.ts",
    "index abc..def 100644",
    "--- a/src/foo.ts",
    "+++ b/src/foo.ts",
    "+new line",
    "diff --git a/README.md b/README.md",
    "--- a/README.md",
    "+++ b/README.md",
    "+change",
  ].join("\n");
  const files = extractChangedFiles(patch);
  expect(files).toEqual(["src/foo.ts", "README.md"]);
});

test("extractChangedFiles returns empty for empty patch", () => {
  expect(extractChangedFiles("")).toEqual([]);
});
