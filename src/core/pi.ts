import { spawn } from "node:child_process";
import { RunTimeoutError } from "./timeout.js";
import type { LinkedProject } from "./types.js";

export interface PiRunOptions {
  prompt: string;
  model?: string;
  project: LinkedProject;
  timeoutMs?: number;
}

export async function runPi(options: PiRunOptions): Promise<number> {
  const args = ["-p", options.prompt];
  if (options.model) {
    args.push("--model", options.model);
  }

  return new Promise<number>((resolve, reject) => {
    const child = spawn("pi", args, {
      cwd: options.project.path,
      stdio: "inherit",
    });

    let timeout: NodeJS.Timeout | undefined;
    let settled = false;

    if (options.timeoutMs !== undefined) {
      timeout = setTimeout(() => {
        if (settled) {
          return;
        }
        settled = true;
        child.kill("SIGTERM");
        reject(new RunTimeoutError(`Pi run exceeded timeout of ${options.timeoutMs}ms.`));
      }, options.timeoutMs);
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