import { describe, expect, it } from "vitest";
import { getConfiguredValidationNames, hasAllRequiredAutoMergeValidations } from "../../src/core/validation-utils.js";
import type { ProjectConfig, ValidationSummary } from "../../src/core/types.js";

function makeConfig(overrides?: Partial<ProjectConfig["validation"]>): ProjectConfig {
  return {
    version: 1,
    project: { alias: null, repoRoot: null, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null, ...overrides },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
  };
}

describe("getConfiguredValidationNames", () => {
  it("returns empty array when no commands configured", () => {
    expect(getConfiguredValidationNames(makeConfig())).toEqual([]);
  });

  it("returns lint when lintCommand configured", () => {
    expect(getConfiguredValidationNames(makeConfig({ lintCommand: "eslint ." }))).toEqual(["lint"]);
  });

  it("returns all three when all configured", () => {
    const config = makeConfig({
      lintCommand: "eslint .",
      testCommand: "vitest run",
      typecheckCommand: "tsc --noEmit",
    });
    expect(getConfiguredValidationNames(config)).toEqual(["lint", "test", "typecheck"]);
  });
});

describe("hasAllRequiredAutoMergeValidations", () => {
  it("returns false when no validations configured", () => {
    expect(hasAllRequiredAutoMergeValidations([], makeConfig())).toBe(false);
  });

  it("returns true when all configured validations pass", () => {
    const config = makeConfig({ lintCommand: "eslint .", testCommand: "vitest run" });
    const validation: ValidationSummary[] = [
      { name: "lint", command: "eslint .", exitCode: 0 },
      { name: "test", command: "vitest run", exitCode: 0 },
    ];
    expect(hasAllRequiredAutoMergeValidations(validation, config)).toBe(true);
  });

  it("returns false when a configured validation fails", () => {
    const config = makeConfig({ lintCommand: "eslint .", testCommand: "vitest run" });
    const validation: ValidationSummary[] = [
      { name: "lint", command: "eslint .", exitCode: 0 },
      { name: "test", command: "vitest run", exitCode: 1 },
    ];
    expect(hasAllRequiredAutoMergeValidations(validation, config)).toBe(false);
  });

  it("returns false when a configured validation is missing from results", () => {
    const config = makeConfig({ lintCommand: "eslint .", testCommand: "vitest run" });
    const validation: ValidationSummary[] = [
      { name: "lint", command: "eslint .", exitCode: 0 },
    ];
    expect(hasAllRequiredAutoMergeValidations(validation, config)).toBe(false);
  });
});
