import { resolveProvider, type AgentRunOptions } from "./providers.js";
import type { AgentRunResult, LinkedProject, ProjectConfig } from "./types.js";

export interface PiRunOptions {
  prompt: string;
  model?: string;
  project: LinkedProject;
  timeoutMs?: number;
}

/**
 * Gate a command on the availability of the resolved provider's binary.
 * Replaces the Pi-only gate: whichever agent the project config resolves to
 * (claude, codex, custom, …) must be on PATH before the command proceeds.
 */
export function assertProviderAvailable(projectConfig: ProjectConfig | null, defaultProvider?: string | null): void {
  const provider = resolveProvider(
    projectConfig?.agent?.type ?? undefined,
    projectConfig?.agent?.command ?? null,
    defaultProvider ?? undefined,
  );
  if (!provider.checkAvailable()) {
    throw new Error(`'${provider.name}' binary not found on PATH`);
  }
}

export async function runPi(options: PiRunOptions): Promise<AgentRunResult> {
  return resolveProvider("pi", null).run({
    prompt: options.prompt,
    model: options.model,
    projectPath: options.project.path,
    projectAlias: options.project.alias,
    timeoutMs: options.timeoutMs,
  });
}

// D6 / A1: Model-agnostic agent runner dispatching to the configured provider.
export async function runAgent(
  options: PiRunOptions,
  projectConfig?: ProjectConfig,
  defaultProvider?: string,
): Promise<AgentRunResult> {
  const agentType = projectConfig?.agent?.type ?? undefined;
  const customCommand = projectConfig?.agent?.command ?? null;
  const provider = resolveProvider(agentType, customCommand, defaultProvider);

  const runOptions: AgentRunOptions = {
    prompt: options.prompt,
    model: options.model,
    projectPath: options.project.path,
    projectAlias: options.project.alias,
    timeoutMs: options.timeoutMs,
    extraArgs: projectConfig?.agent?.extraArgs,
  };

  return provider.run(runOptions);
}
