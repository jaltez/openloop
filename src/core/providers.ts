import { spawn, execFileSync } from "node:child_process";
import { RunTimeoutError } from "./timeout.js";

export interface AgentRunOptions {
  prompt: string;
  model?: string;
  projectPath: string;
  timeoutMs?: number;
}

export interface AgentProvider {
  /** Short identifier used in config files. */
  name: string;
  /** Human-readable label. */
  label: string;
  /** Check whether the provider's binary is available. */
  checkAvailable(): boolean;
  /** Execute a prompt and return the process exit code. */
  run(options: AgentRunOptions): Promise<number>;
}

// ---------------------------------------------------------------------------
// Built-in providers
// ---------------------------------------------------------------------------

function binaryExists(name: string): boolean {
  try {
    execFileSync("which", [name], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

function spawnAndWait(
  binary: string,
  args: string[],
  cwd: string,
  timeoutMs?: number,
  env?: Record<string, string | undefined>,
): Promise<number> {
  return new Promise<number>((resolve, reject) => {
    const child = spawn(binary, args, { cwd, env: env ?? process.env, stdio: "inherit" });
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
  });
}

const piProvider: AgentProvider = {
  name: "pi",
  label: "Pi",
  checkAvailable: () => binaryExists("pi"),
  run(options) {
    const args = ["-p", options.prompt];
    if (options.model) args.push("--model", options.model);
    return spawnAndWait("pi", args, options.projectPath, options.timeoutMs);
  },
};

const claudeProvider: AgentProvider = {
  name: "claude",
  label: "Claude Code",
  checkAvailable: () => binaryExists("claude"),
  run(options) {
    const args = ["-p", options.prompt];
    if (options.model) args.push("--model", options.model);
    return spawnAndWait("claude", args, options.projectPath, options.timeoutMs);
  },
};

const aiderProvider: AgentProvider = {
  name: "aider",
  label: "Aider",
  checkAvailable: () => binaryExists("aider"),
  run(options) {
    const args = ["--message", options.prompt, "--yes"];
    if (options.model) args.push("--model", options.model);
    return spawnAndWait("aider", args, options.projectPath, options.timeoutMs);
  },
};

const codexProvider: AgentProvider = {
  name: "codex",
  label: "OpenAI Codex",
  checkAvailable: () => binaryExists("codex"),
  run(options) {
    const args = ["-q", options.prompt];
    if (options.model) args.push("--model", options.model);
    return spawnAndWait("codex", args, options.projectPath, options.timeoutMs);
  },
};

const opencodeProvider: AgentProvider = {
  name: "opencode",
  label: "OpenCode",
  checkAvailable: () => binaryExists("opencode"),
  run(options) {
    const args = ["run", options.prompt];
    if (options.model) args.push("--model", options.model);
    return spawnAndWait("opencode", args, options.projectPath, options.timeoutMs);
  },
};

// ---------------------------------------------------------------------------
// Custom command provider (instantiated per-project)
// ---------------------------------------------------------------------------

export function createCustomProvider(command: string): AgentProvider {
  return {
    name: "custom",
    label: `Custom (${command})`,
    checkAvailable: () => true,
    run(options) {
      return spawnAndWait("sh", ["-c", `${command} "$OPENLOOP_PROMPT"`], options.projectPath, options.timeoutMs, {
        ...process.env,
        OPENLOOP_PROMPT: options.prompt,
        OPENLOOP_MODEL: options.model ?? "",
      });
    },
  };
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

const BUILTIN_PROVIDERS: AgentProvider[] = [
  piProvider,
  claudeProvider,
  aiderProvider,
  codexProvider,
  opencodeProvider,
];

const providerMap = new Map<string, AgentProvider>(
  BUILTIN_PROVIDERS.map((p) => [p.name, p]),
);

export type ProviderName = "pi" | "claude" | "aider" | "codex" | "opencode" | "custom";

export const PROVIDER_NAMES: readonly string[] = BUILTIN_PROVIDERS.map((p) => p.name);

export function getProvider(name: string): AgentProvider | undefined {
  return providerMap.get(name);
}

export function listProviders(): AgentProvider[] {
  return [...BUILTIN_PROVIDERS];
}

/**
 * Resolve the provider for a given project configuration and optional custom
 * command. Falls back to the supplied default (usually from global config).
 */
export function resolveProvider(
  agentType: string | undefined,
  customCommand: string | null | undefined,
  defaultProvider?: string,
): AgentProvider {
  const name = agentType ?? defaultProvider ?? "pi";

  if (name === "custom" && customCommand) {
    return createCustomProvider(customCommand);
  }

  return providerMap.get(name) ?? piProvider;
}
