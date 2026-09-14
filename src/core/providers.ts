import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { registerActiveRun } from "./active-runs.js";
import { RunTimeoutError } from "./timeout.js";
import type { AgentRunResult, AgentUsage } from "./types.js";

export interface AgentRunOptions {
  prompt: string;
  model?: string;
  projectPath: string;
  /** Alias of the project this run belongs to (active-run registry). */
  projectAlias?: string | null;
  timeoutMs?: number;
  /** Extra CLI flags appended verbatim (agent.extraArgs escape hatch). */
  extraArgs?: string[];
}

export interface AgentProvider {
  /** Short identifier used in config files. */
  name: string;
  /** Human-readable label. */
  label: string;
  /** Check whether the provider's binary is available. */
  checkAvailable(): boolean;
  /** Execute a prompt and return the captured run result (stdout/stderr/usage). */
  run(options: AgentRunOptions): Promise<AgentRunResult>;
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

interface RawRunResult {
  exitCode: number;
  stdout: string;
  stderr: string;
}

function spawnAndWait(
  binary: string,
  args: string[],
  cwd: string,
  options?: {
    timeoutMs?: number;
    env?: Record<string, string | undefined>;
    projectAlias?: string | null;
  },
): Promise<RawRunResult> {
  const { promise, resolve, reject } = Promise.withResolvers<RawRunResult>();
  // detached: the agent gets its own process group so a timeout kill reaches
  // the whole subprocess tree, not just the launcher binary.
  const child = spawn(binary, args, {
    cwd,
    env: options?.env ?? process.env,
    stdio: ["ignore", "pipe", "pipe"],
    detached: true,
  });
  let stdout = "";
  let stderr = "";
  let settled = false;
  let timeout: NodeJS.Timeout | undefined;

  // Rolling capture capped at 4 MiB per stream: keeps usage-JSON tails and
  // transcript slices intact while bounding memory for chatty agents.
  const captureLimit = 4 * 1024 * 1024;
  const append = (target: "stdout" | "stderr", text: string): void => {
    if (target === "stdout") {
      stdout += text;
      if (stdout.length > captureLimit) stdout = stdout.slice(stdout.length - captureLimit);
    } else {
      stderr += text;
      if (stderr.length > captureLimit) stderr = stderr.slice(stderr.length - captureLimit);
    }
  };
  child.stdout?.on("data", (chunk: Buffer | string) => {
    append("stdout", String(chunk));
  });
  child.stderr?.on("data", (chunk: Buffer | string) => {
    append("stderr", String(chunk));
  });

  const unregister = options?.projectAlias
    ? registerActiveRun(options.projectAlias, () => killProcessGroup(child, "SIGTERM"))
    : null;

  const finish = (settle: () => void) => {
    if (settled) return;
    settled = true;
    clearTimeout(timeout);
    unregister?.();
    settle();
  };

  if (options?.timeoutMs !== undefined) {
    timeout = setTimeout(() => {
      finish(() => {
        killProcessGroup(child, "SIGTERM");
        // Escalate after a grace period; ESRCH for already-dead groups is fine.
        const escalation = setTimeout(() => killProcessGroup(child, "SIGKILL"), 5000);
        escalation.unref();
        reject(new RunTimeoutError(`Agent run exceeded timeout of ${options.timeoutMs}ms.`));
      });
    }, options.timeoutMs);
  }

  child.on("error", (error) => finish(() => reject(error)));
  // Settle on 'close', not 'exit': grandchildren of a shell-wrapped custom
  // command may still hold the stdout pipe when the direct child exits, and
  // 'exit'-time settling truncates their output (and the usage JSON tail).
  child.on("close", (code) => finish(() => resolve({ exitCode: code ?? 1, stdout, stderr })));

  return promise;
}

function killProcessGroup(child: ChildProcess, signal: "SIGTERM" | "SIGKILL" = "SIGTERM"): void {
  try {
    process.kill(-child.pid!, signal);
  } catch {
    try {
      child.kill(signal);
    } catch {
      // Process group and leader both gone — nothing to kill.
    }
  }
}

// ---------------------------------------------------------------------------
// Usage parsing — best-effort by design. A provider whose output cannot be
// parsed simply reports no usage and the run cost falls back to the estimate.
// ---------------------------------------------------------------------------

function toNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function extractUsage(obj: unknown): AgentUsage | undefined {
  if (!obj || typeof obj !== "object") {
    return undefined;
  }
  const record = obj as Record<string, unknown>;
  const nested = record.usage && typeof record.usage === "object"
    ? (record.usage as Record<string, unknown>)
    : {};
  const usage: AgentUsage = {
    inputTokens: toNumber(nested.input_tokens) ?? toNumber(nested.inputTokens),
    outputTokens: toNumber(nested.output_tokens) ?? toNumber(nested.outputTokens),
    totalTokens: toNumber(nested.total_tokens) ?? toNumber(nested.totalTokens) ?? toNumber(record.total_tokens),
    costUsd: toNumber(record.total_cost_usd) ?? toNumber(record.cost_usd)
      ?? toNumber(nested.total_cost_usd) ?? toNumber(nested.cost_usd),
  };
  const defined = Object.entries(usage).filter(([, value]) => value !== undefined);
  if (defined.length === 0) {
    return undefined;
  }
  return Object.fromEntries(defined) as AgentUsage;
}

/** claude --output-format json prints a single JSON object on stdout. */
function parseClaudeUsage(stdout: string): AgentUsage | undefined {
  return extractUsage(JSON.parse(stdout));
}

/** pi/codex print usage (if at all) in the last JSON line of stdout. */
function parseLastLineUsage(stdout: string): AgentUsage | undefined {
  const lines = stdout.trim().split("\n");
  const last = lines[lines.length - 1];
  if (!last) {
    return undefined;
  }
  return extractUsage(JSON.parse(last));
}

async function withUsage(
  raw: Promise<RawRunResult>,
  parse: (stdout: string) => AgentUsage | undefined,
): Promise<AgentRunResult> {
  const result = await raw;
  try {
    const usage = parse(result.stdout);
    return usage ? { ...result, usage } : result;
  } catch {
    return result; // Malformed output is non-fatal — cost falls back to estimated.
  }
}

function appendExtraArgs(args: string[], extraArgs?: string[]): void {
  if (extraArgs && extraArgs.length > 0) {
    args.push(...extraArgs);
  }
}

const piProvider: AgentProvider = {
  name: "pi",
  label: "Pi",
  checkAvailable: () => binaryExists("pi"),
  run(options) {
    const args = ["-p", options.prompt];
    if (options.model) args.push("--model", options.model);
    appendExtraArgs(args, options.extraArgs);
    return withUsage(
      spawnAndWait("pi", args, options.projectPath, { timeoutMs: options.timeoutMs, projectAlias: options.projectAlias }),
      parseLastLineUsage,
    );
  },
};

const claudeProvider: AgentProvider = {
  name: "claude",
  label: "Claude Code",
  checkAvailable: () => binaryExists("claude"),
  run(options) {
    // Headless defaults: JSON stdout for usage/cost parsing, acceptEdits so
    // unattended runs can modify files. Stronger bypass modes stay opt-in via
    // agent.extraArgs.
    const args = ["-p", options.prompt, "--output-format", "json", "--permission-mode", "acceptEdits"];
    if (options.model) args.push("--model", options.model);
    appendExtraArgs(args, options.extraArgs);
    return withUsage(
      spawnAndWait("claude", args, options.projectPath, { timeoutMs: options.timeoutMs, projectAlias: options.projectAlias }),
      parseClaudeUsage,
    );
  },
};

const aiderProvider: AgentProvider = {
  name: "aider",
  label: "Aider",
  checkAvailable: () => binaryExists("aider"),
  run(options) {
    const args = ["--message", options.prompt, "--yes"];
    if (options.model) args.push("--model", options.model);
    appendExtraArgs(args, options.extraArgs);
    return spawnAndWait("aider", args, options.projectPath, { timeoutMs: options.timeoutMs, projectAlias: options.projectAlias });
  },
};

const codexProvider: AgentProvider = {
  name: "codex",
  label: "OpenAI Codex",
  checkAvailable: () => binaryExists("codex"),
  run(options) {
    const args = ["exec", "--json", options.prompt];
    if (options.model) args.push("--model", options.model);
    appendExtraArgs(args, options.extraArgs);
    return withUsage(
      spawnAndWait("codex", args, options.projectPath, { timeoutMs: options.timeoutMs, projectAlias: options.projectAlias }),
      parseLastLineUsage,
    );
  },
};

const opencodeProvider: AgentProvider = {
  name: "opencode",
  label: "OpenCode",
  checkAvailable: () => binaryExists("opencode"),
  run(options) {
    const args = ["run", options.prompt];
    if (options.model) args.push("--model", options.model);
    appendExtraArgs(args, options.extraArgs);
    return spawnAndWait("opencode", args, options.projectPath, { timeoutMs: options.timeoutMs, projectAlias: options.projectAlias });
  },
};

const kaProvider: AgentProvider = {
  name: "ka",
  label: "ka",
  checkAvailable: () => binaryExists("ka"),
  run(options) {
    // Guarded mode keeps ka's permission gate on for unattended runs; --trust
    // and sandbox overrides stay opt-in via agent.extraArgs.
    const args = ["run", "--mode", "guarded"];
    if (options.model) args.push("--model", options.model);
    args.push(options.prompt);
    appendExtraArgs(args, options.extraArgs);
    return withUsage(
      spawnAndWait("ka", args, options.projectPath, { timeoutMs: options.timeoutMs, projectAlias: options.projectAlias }),
      parseLastLineUsage,
    );
  },
};

const ompProvider: AgentProvider = {
  name: "omp",
  label: "Oh My Pi",
  checkAvailable: () => binaryExists("omp"),
  run(options) {
    // Print mode with the JSON event stream so usage can be parsed from the
    // last NDJSON line. Requires a one-time interactive `omp` /login.
    const args = ["-p", options.prompt, "--mode", "json"];
    if (options.model) args.push("--model", options.model);
    appendExtraArgs(args, options.extraArgs);
    return withUsage(
      spawnAndWait("omp", args, options.projectPath, { timeoutMs: options.timeoutMs, projectAlias: options.projectAlias }),
      parseLastLineUsage,
    );
  },
};

// ---------------------------------------------------------------------------
// Custom command provider (instantiated per-project)
// ---------------------------------------------------------------------------

/** POSIX single-quote: safe verbatim argument passing into `sh -c` strings. */
function shellQuotePosix(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

// ---------------------------------------------------------------------------
// Custom command provider (instantiated per-project)
// ---------------------------------------------------------------------------

export function createCustomProvider(command: string): AgentProvider {
  return {
    name: "custom",
    label: `Custom (${command})`,
    checkAvailable: () => true,
    run(options) {
      let shellCommand = `${command} "$OPENLOOP_PROMPT"`;
      if (options.extraArgs && options.extraArgs.length > 0) {
        shellCommand += ` ${options.extraArgs.map(shellQuotePosix).join(" ")}`;
      }
      return spawnAndWait("sh", ["-c", shellCommand], options.projectPath, {
        timeoutMs: options.timeoutMs,
        projectAlias: options.projectAlias,
        env: {
          ...process.env,
          OPENLOOP_PROMPT: options.prompt,
          OPENLOOP_MODEL: options.model ?? "",
        },
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
  kaProvider,
  ompProvider,
];

const providerMap = new Map<string, AgentProvider>(
  BUILTIN_PROVIDERS.map((p) => [p.name, p]),
);

export type ProviderName = "pi" | "claude" | "aider" | "codex" | "opencode" | "ka" | "omp" | "custom";

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
