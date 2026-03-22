import { spawn, execFileSync } from "node:child_process";
import { RunTimeoutError } from "./timeout.js";
import { resolveProvider, type AgentRunOptions } from "./providers.js";
import type { LinkedProject, ProjectConfig } from "./types.js";

export interface PiRunOptions {
  prompt: string;
  model?: string;
  project: LinkedProject;
  timeoutMs?: number;
}

export function assertPiOnPath(): void {
  try {
    execFileSync("which", ["pi"], { stdio: "ignore" });
  } catch {
    throw new Error(
      `'pi' binary not found on PATH. Install Pi from https://pi.dev and ensure it is in your PATH.`,
    );
  }
}

export async function runPi(options: PiRunOptions): Promise<number> {
  const args = ["-p", options.prompt];
  if (options.model) {
    args.push("--model", options.model);
  }

  return spawnAgent("pi", args, options.project.path, options.timeoutMs);
}

// D6 / A1: Model-agnostic agent runner dispatching to the configured provider.
export async function runAgent(options: PiRunOptions, projectConfig?: ProjectConfig, defaultProvider?: string): Promise<number> {
  const agentType = projectConfig?.agent?.type ?? undefined;
  const customCommand = projectConfig?.agent?.command ?? null;
  const provider = resolveProvider(agentType, customCommand, defaultProvider);

  const runOptions: AgentRunOptions = {
    prompt: options.prompt,
    model: options.model,
    projectPath: options.project.path,
    timeoutMs: options.timeoutMs,
  };

  return provider.run(runOptions);
}

function spawnAgent(
  binary: string,
  args: string[],
  cwd: string,
  timeoutMs?: number,
): Promise<number> {
  return new Promise<number>((resolve, reject) => {
    const child = spawn(binary, args, { cwd, stdio: "inherit" });
    setupAgentProcess(child, timeoutMs, resolve, reject);
  });
}

function setupAgentProcess(
  child: ReturnType<typeof spawn>,
  timeoutMs: number | undefined,
  resolve: (code: number) => void,
  reject: (err: Error) => void,
): void {
  let timeout: NodeJS.Timeout | undefined;
  let settled = false;

  if (timeoutMs !== undefined) {
    timeout = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill("SIGTERM");
      reject(new RunTimeoutError(`Agent run exceeded timeout of ${timeoutMs}ms.`));
    }, timeoutMs);
  }

  child.on("error", (error) => {
    if (settled) return;
    settled = true;
    if (timeout) clearTimeout(timeout);
    reject(error);
  });
  child.on("exit", (code) => {
    if (settled) return;
    settled = true;
    if (timeout) clearTimeout(timeout);
    resolve(code ?? 1);
  });
}