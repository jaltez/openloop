import fs from "node:fs/promises";
import path from "node:path";
import { fileExists } from "./fs.js";

export interface DetectedValidationCommands {
  lintCommand: string | null;
  testCommand: string | null;
  typecheckCommand: string | null;
}

export async function detectValidationCommands(projectPath: string): Promise<DetectedValidationCommands> {
  const packageJsonPath = path.join(projectPath, "package.json");
  if (await fileExists(packageJsonPath)) {
    return detectNodeValidationCommands(projectPath, packageJsonPath);
  }

  return detectPythonValidationCommands(projectPath);
}

async function detectNodeValidationCommands(projectPath: string, packageJsonPath: string): Promise<DetectedValidationCommands> {
  const raw = await fs.readFile(packageJsonPath, "utf8");
  const packageJson = JSON.parse(raw) as { scripts?: Record<string, string> };
  const scripts = packageJson.scripts ?? {};
  const runner = await detectNodeRunner(projectPath);

  return {
    lintCommand: scripts.lint ? `${runner} run lint` : null,
    testCommand: scripts.test ? `${runner} run test` : null,
    typecheckCommand: scripts.typecheck
      ? `${runner} run typecheck`
      : scripts.check
        ? `${runner} run check`
        : null,
  };
}

async function detectNodeRunner(projectPath: string): Promise<"bun" | "pnpm" | "yarn" | "npm"> {
  const candidates: Array<[string, "bun" | "pnpm" | "yarn" | "npm"]> = [
    ["bun.lock", "bun"],
    ["pnpm-lock.yaml", "pnpm"],
    ["yarn.lock", "yarn"],
    ["package-lock.json", "npm"],
  ];

  for (const [filename, runner] of candidates) {
    if (await fileExists(path.join(projectPath, filename))) {
      return runner;
    }
  }

  return "npm";
}

async function detectPythonValidationCommands(projectPath: string): Promise<DetectedValidationCommands> {
  const pyproject = path.join(projectPath, "pyproject.toml");
  const requirements = path.join(projectPath, "requirements.txt");
  if (!(await fileExists(pyproject)) && !(await fileExists(requirements))) {
    return {
      lintCommand: null,
      testCommand: null,
      typecheckCommand: null,
    };
  }

  return {
    lintCommand: "ruff check .",
    testCommand: "pytest",
    typecheckCommand: "mypy .",
  };
}