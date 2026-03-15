import type { ProjectConfig, ValidationSummary } from "./types.js";

export function getConfiguredValidationNames(projectConfig: ProjectConfig): ValidationSummary["name"][] {
  const configured: ValidationSummary["name"][] = [];
  if (projectConfig.validation.lintCommand) {
    configured.push("lint");
  }
  if (projectConfig.validation.testCommand) {
    configured.push("test");
  }
  if (projectConfig.validation.typecheckCommand) {
    configured.push("typecheck");
  }
  return configured;
}

export function hasAllRequiredAutoMergeValidations(validation: ValidationSummary[], projectConfig: ProjectConfig): boolean {
  const requiredValidationNames = getConfiguredValidationNames(projectConfig);
  if (requiredValidationNames.length === 0) {
    return false;
  }

  return requiredValidationNames.every((name) => validation.some((item) => item.name === name && item.exitCode === 0));
}
