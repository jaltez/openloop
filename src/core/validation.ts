import { spawn } from "node:child_process";
import { RunTimeoutError } from "./timeout.js";
import type { ProjectConfig, ValidationSummary } from "./types.js";

export interface ValidationRunner {
  (projectPath: string, command: string, timeoutMs?: number): Promise<number>;
}

export async function runConfiguredValidations(
  projectPath: string,
  projectConfig: ProjectConfig,
  runner?: ValidationRunner,
  options?: {
    getTimeoutMs?: () => number | undefined;
  },
): Promise<ValidationSummary[]> {
  const validationRunner = runner ?? runShellCommand;
  const steps: Array<{ name: "lint" | "test" | "typecheck"; command: string | null }> = [
    { name: "lint", command: projectConfig.validation.lintCommand },
    { name: "test", command: projectConfig.validation.testCommand },
    { name: "typecheck", command: projectConfig.validation.typecheckCommand },
  ];

  const results: ValidationSummary[] = [];
  for (const step of steps) {
    if (!step.command) {
      continue;
    }
    const timeoutMs = options?.getTimeoutMs?.();
    if (timeoutMs !== undefined && timeoutMs <= 0) {
      throw new RunTimeoutError("Run exceeded timeout before validation completed.");
    }
    const exitCode = await validationRunner(projectPath, step.command, timeoutMs);
    results.push({
      name: step.name,
      command: step.command,
      exitCode,
    });
    if (exitCode !== 0) {
      break;
    }
  }

  return results;
}

async function runShellCommand(projectPath: string, command: string, timeoutMs?: number): Promise<number> {
  return new Promise<number>((resolve, reject) => {
    const child = spawn(command, {
      cwd: projectPath,
      stdio: "inherit",
      shell: true,
    });

    let timeout: NodeJS.Timeout | undefined;
    let settled = false;
    if (timeoutMs !== undefined) {
      timeout = setTimeout(() => {
        if (settled) {
          return;
        }
        settled = true;
        child.kill("SIGTERM");
        reject(new RunTimeoutError(`Validation command exceeded timeout of ${timeoutMs}ms.`));
      }, timeoutMs);
    }

    child.on("error", (error) => {
      if (settled) {
        return;
      }
      settled = true;
      if (timeout) {
        clearTimeout(timeout);
      }
      reject(error);
    });
    child.on("exit", (code) => {
      if (settled) {
        return;
      }
      settled = true;
      if (timeout) {
        clearTimeout(timeout);
      }
      resolve(code ?? 1);
    });
  });
}