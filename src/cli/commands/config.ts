import type { Argv, ArgumentsCamelCase, Options } from "yargs";
import { getProject } from "../../core/project-registry.js";
import { loadGlobalConfig, saveGlobalConfig } from "../../core/global-config.js";
import { loadProjectConfig, saveProjectConfig } from "../../core/project-config.js";
import { listProviders, PROVIDER_NAMES, getProvider } from "../../core/providers.js";
import type { LifecycleHookConfig } from "../../core/types.js";

type ModelArgs = ArgumentsCamelCase<{ model: string }>;
type ProjectArgs = ArgumentsCamelCase<{ project: string }>;
type ProjectModelArgs = ArgumentsCamelCase<{ project: string; model: string }>;
type ConfigSetArgs = ArgumentsCamelCase<{ key: string; value: string }>;
type HookArgs = ArgumentsCamelCase<{
  type: string;
  events: string;
  command?: string;
  url?: string;
  timeoutSeconds?: number;
}>;

const SETTABLE_CONFIG_KEYS: Record<string, { type: "number" | "string"; min?: number; max?: number }> = {
  "budgets.dailyCostUsd": { type: "number", min: 0 },
  "budgets.estimatedCostPerRunUsd": { type: "number", min: 0 },
  "runtime.runTimeoutSeconds": { type: "number", min: 1 },
  "runtime.maxAttemptsPerTask": { type: "number", min: 1 },
  "runtime.noProgressRepeatLimit": { type: "number", min: 1 },
  "runtime.tickIntervalSeconds": { type: "number", min: 1 },
  "runtime.projectSelectionStrategy": { type: "string" },
  "notifications.onTaskComplete": { type: "string" },
  "notifications.onTaskFailed": { type: "string" },
  "notifications.onBudgetBlocked": { type: "string" },
  "notifications.onAllTasksDone": { type: "string" },
};

function setNestedValue(obj: Record<string, unknown>, keyPath: string, value: unknown): void {
  const parts = keyPath.split(".");
  let current = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    const part = parts[i]!;
    if (typeof current[part] !== "object" || current[part] === null) {
      current[part] = {};
    }
    current = current[part] as Record<string, unknown>;
  }
  current[parts[parts.length - 1]!] = value;
}

function parseEvents(raw: string): string[] {
  return String(raw).split(",").map((item) => item.trim()).filter(Boolean);
}

function buildHookConfig(args: HookArgs): LifecycleHookConfig | null {
  const type = String(args.type) as LifecycleHookConfig["type"];
  const events = parseEvents(String(args.events || "*"));
  const timeoutSeconds = args.timeoutSeconds === undefined ? 10 : Number(args.timeoutSeconds);

  if (!Number.isFinite(timeoutSeconds) || timeoutSeconds < 1) {
    console.error("Error: --timeout-seconds must be a number >= 1");
    return null;
  }

  if (type === "command") {
    if (!args.command) {
      console.error("Error: --command is required for command hooks");
      return null;
    }
    return {
      type,
      events,
      command: String(args.command),
      timeoutSeconds,
    };
  }

  if (!args.url) {
    console.error("Error: --url is required for webhook hooks");
    return null;
  }
  try {
    new URL(String(args.url));
  } catch {
    console.error("Error: --url must be a valid URL");
    return null;
  }

  return {
    type,
    events,
    url: String(args.url),
    timeoutSeconds,
  };
}

function formatHook(hook: LifecycleHookConfig, index: number): string {
  const target = hook.type === "command" ? hook.command : hook.url;
  const events = hook.events.join(", ") || "*";
  const timeout = hook.timeoutSeconds ?? 10;
  return `[${index}] ${hook.type} → ${target} (events: ${events}; timeout: ${timeout}s)`;
}

const HOOK_OPTIONS: Record<string, Options> = {
  type: { type: "string", demandOption: true, choices: ["command", "webhook"] as const, describe: "Hook type" },
  events: { type: "string", default: "*", describe: "Comma-separated events or * for all" },
  command: { type: "string", describe: "Shell command for command hooks" },
  url: { type: "string", describe: "URL for webhook hooks" },
  timeoutSeconds: { type: "number", default: 10, describe: "Hook timeout in seconds" },
};

export function registerConfigCommands(cli: Argv): void {
  cli.command(
    "config <command>",
    "Manage global and project configuration",
    (configCli: Argv) =>
      configCli
        .command("show", "Show global config", () => {}, async () => {
          console.log(JSON.stringify(await loadGlobalConfig(), null, 2));
        })
        .command(
          "set <key> <value>",
          "Set a global config value by dot-notation key",
          {
            key: { type: "string", demandOption: true },
            value: { type: "string", demandOption: true },
          },
          async (args: ConfigSetArgs) => {
            const key = String(args.key);
            const raw = String(args.value);
            const meta = SETTABLE_CONFIG_KEYS[key];
            if (!meta) {
              console.error(`Error: '${key}' is not a settable config key.`);
              console.error(`Valid keys: ${Object.keys(SETTABLE_CONFIG_KEYS).join(", ")}`);
              process.exitCode = 1;
              return;
            }
            let parsed: unknown = raw;
            if (meta.type === "number") {
              const num = Number(raw);
              if (!Number.isFinite(num)) {
                console.error(`Error: '${raw}' is not a valid number for ${key}`);
                process.exitCode = 1;
                return;
              }
              if (meta.min !== undefined && num < meta.min) {
                console.error(`Error: ${key} must be >= ${meta.min}`);
                process.exitCode = 1;
                return;
              }
              if (meta.max !== undefined && num > meta.max) {
                console.error(`Error: ${key} must be <= ${meta.max}`);
                process.exitCode = 1;
                return;
              }
              parsed = num;
            } else if (raw === "null") {
              parsed = null;
            }
            const config = await loadGlobalConfig();
            setNestedValue(config as unknown as Record<string, unknown>, key, parsed);
            await saveGlobalConfig(config);
            console.log(`Set ${key} = ${JSON.stringify(parsed)}`);
          },
        )
        .command(
          "set-model <model>",
          "Set the global default model",
          { model: { type: "string", demandOption: true } },
          async (args: ModelArgs) => {
            const config = await loadGlobalConfig();
            config.model = String(args.model);
            await saveGlobalConfig(config);
            console.log(`Global model set to ${config.model}`);
          },
        )
        .command(
          "project-show <project>",
          "Show project-local config",
          { project: { type: "string", demandOption: true } },
          async (args: ProjectArgs) => {
            const project = await getProject(String(args.project));
            console.log(JSON.stringify(await loadProjectConfig(project.path), null, 2));
          },
        )
        .command(
          "project-set-model <project> <model>",
          "Set the model for a linked project",
          {
            project: { type: "string", demandOption: true },
            model: { type: "string", demandOption: true },
          },
          async (args: ProjectModelArgs) => {
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);
            config.pi.model = String(args.model);
            await saveProjectConfig(project.path, config);
            console.log(`Project ${project.alias} model set to ${config.pi.model}`);
          },
        )
        .command(
          "set-provider <provider>",
          "Set the global default agent provider",
          { provider: { type: "string", demandOption: true, choices: [...PROVIDER_NAMES] } },
          async (args: ArgumentsCamelCase<{ provider: string }>) => {
            const name = String(args.provider);
            const provider = getProvider(name);
            if (!provider) {
              console.error(`Error: unknown provider '${name}'. Available: ${PROVIDER_NAMES.join(", ")}`);
              process.exitCode = 1;
              return;
            }
            const config = await loadGlobalConfig();
            config.defaultProvider = name;
            await saveGlobalConfig(config);
            console.log(`Global provider set to ${provider.label} (${name})`);
          },
        )
        .command(
          "list-providers",
          "List all available agent providers",
          () => {},
          async () => {
            const config = await loadGlobalConfig();
            const defaultName = config.defaultProvider ?? "pi";
            for (const provider of listProviders()) {
              const available = provider.checkAvailable();
              const isDefault = provider.name === defaultName;
              const icon = available ? "✅" : "❌";
              const tag = isDefault ? " (default)" : "";
              console.log(`${icon} ${provider.name} — ${provider.label}${tag}`);
            }
            console.log(`  + custom — Run any shell command (configure per-project)`);
          },
        )
        .command(
          "project-set-agent <project> <provider>",
          "Set the agent provider for a linked project",
          {
            project: { type: "string", demandOption: true },
            provider: { type: "string", demandOption: true, choices: [...PROVIDER_NAMES, "custom"] },
            command: { type: "string", describe: "Custom command (required when provider is 'custom')" },
          },
          async (args: ArgumentsCamelCase<{ project: string; provider: string; command?: string }>) => {
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);
            const providerName = String(args.provider) as typeof config.agent extends { type: infer T } ? T : string;
            if (providerName === "custom" && !args.command) {
              console.error("Error: --command is required when provider is 'custom'");
              process.exitCode = 1;
              return;
            }
            config.agent = {
              type: providerName as "pi" | "claude" | "aider" | "codex" | "opencode" | "custom",
              command: providerName === "custom" ? String(args.command) : null,
            };
            await saveProjectConfig(project.path, config);
            console.log(`Project ${project.alias} agent set to ${providerName}`);
          },
        )
        .command(
          "add-hook",
          "Add a global lifecycle hook",
          HOOK_OPTIONS,
          async (args) => {
            const hook = buildHookConfig(args as HookArgs);
            if (!hook) {
              process.exitCode = 1;
              return;
            }
            const config = await loadGlobalConfig();
            config.hooks = [...(config.hooks ?? []), hook];
            await saveGlobalConfig(config);
            console.log(`Added global ${hook.type} hook (events: ${hook.events.join(", ")})`);
          },
        )
        .command(
          "remove-hook <index>",
          "Remove a global lifecycle hook by index",
          {
            index: { type: "number", demandOption: true },
          },
          async (args: ArgumentsCamelCase<{ index: number }>) => {
            const idx = Number(args.index);
            const config = await loadGlobalConfig();
            const hooks = config.hooks ?? [];
            if (idx < 0 || idx >= hooks.length) {
              console.error(`Error: index ${idx} out of range (${hooks.length} hooks configured)`);
              process.exitCode = 1;
              return;
            }
            const removed = hooks.splice(idx, 1)[0]!;
            config.hooks = hooks;
            await saveGlobalConfig(config);
            console.log(`Removed global ${removed.type} hook at index ${idx}`);
          },
        )
        .command(
          "list-hooks",
          "List configured global lifecycle hooks",
          () => {},
          async () => {
            const config = await loadGlobalConfig();
            const hooks = config.hooks ?? [];
            if (hooks.length === 0) {
              console.log("No global hooks configured.");
              return;
            }
            for (let i = 0; i < hooks.length; i++) {
              console.log(formatHook(hooks[i]!, i));
            }
          },
        )
        .command(
          "project-add-hook <project>",
          "Add a project-local lifecycle hook",
          {
            project: { type: "string", demandOption: true },
            ...HOOK_OPTIONS,
          },
          async (args) => {
            const hook = buildHookConfig(args as unknown as HookArgs);
            if (!hook) {
              process.exitCode = 1;
              return;
            }
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);
            config.hooks = [...(config.hooks ?? []), hook];
            await saveProjectConfig(project.path, config);
            console.log(`Added project hook for ${project.alias} (events: ${hook.events.join(", ")})`);
          },
        )
        .command(
          "project-remove-hook <project> <index>",
          "Remove a project-local lifecycle hook by index",
          {
            project: { type: "string", demandOption: true },
            index: { type: "number", demandOption: true },
          },
          async (args: ArgumentsCamelCase<{ project: string; index: number }>) => {
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);
            const hooks = config.hooks ?? [];
            const idx = Number(args.index);
            if (idx < 0 || idx >= hooks.length) {
              console.error(`Error: index ${idx} out of range (${hooks.length} hooks configured)`);
              process.exitCode = 1;
              return;
            }
            const removed = hooks.splice(idx, 1)[0]!;
            config.hooks = hooks;
            await saveProjectConfig(project.path, config);
            console.log(`Removed project ${removed.type} hook at index ${idx} for ${project.alias}`);
          },
        )
        .command(
          "project-list-hooks <project>",
          "List configured project-local lifecycle hooks",
          {
            project: { type: "string", demandOption: true },
          },
          async (args: ProjectArgs) => {
            const project = await getProject(String(args.project));
            const config = await loadProjectConfig(project.path);
            const hooks = config.hooks ?? [];
            if (hooks.length === 0) {
              console.log(`No project hooks configured for ${project.alias}.`);
              return;
            }
            for (let i = 0; i < hooks.length; i++) {
              console.log(formatHook(hooks[i]!, i));
            }
          },
        )
        .command(
          "add-channel",
          "Add a notification channel (webhook or desktop)",
          {
            type: { type: "string", demandOption: true, choices: ["webhook", "desktop"], describe: "Channel type" },
            url: { type: "string", describe: "Webhook URL (required for type=webhook)" },
            events: { type: "string", describe: "Comma-separated event names (e.g. task-complete,task-failed) or * for all", default: "*" },
          },
          async (args: ArgumentsCamelCase<{ type: string; url?: string; events: string }>) => {
            const channelType = String(args.type) as "webhook" | "desktop";
            const events = String(args.events).split(",").map((e) => e.trim()).filter(Boolean);

            if (channelType === "webhook") {
              if (!args.url) {
                console.error("Error: --url is required for webhook channels");
                process.exitCode = 1;
                return;
              }
              try {
                new URL(String(args.url));
              } catch {
                console.error("Error: --url must be a valid URL");
                process.exitCode = 1;
                return;
              }
            }

            const config = await loadGlobalConfig();
            if (!config.notificationChannels) config.notificationChannels = [];

            if (channelType === "webhook") {
              config.notificationChannels.push({ type: "webhook", url: String(args.url), events });
            } else {
              config.notificationChannels.push({ type: "desktop", events });
            }

            await saveGlobalConfig(config);
            console.log(`Added ${channelType} notification channel (events: ${events.join(", ")})`);
          },
        )
        .command(
          "remove-channel <index>",
          "Remove a notification channel by index (0-based)",
          {
            index: { type: "number", demandOption: true, describe: "Channel index (see config show)" },
          },
          async (args: ArgumentsCamelCase<{ index: number }>) => {
            const idx = Number(args.index);
            const config = await loadGlobalConfig();
            const channels = config.notificationChannels ?? [];
            if (idx < 0 || idx >= channels.length) {
              console.error(`Error: index ${idx} out of range (${channels.length} channels configured)`);
              process.exitCode = 1;
              return;
            }
            const removed = channels.splice(idx, 1)[0]!;
            config.notificationChannels = channels;
            await saveGlobalConfig(config);
            console.log(`Removed ${removed.type} channel at index ${idx}`);
          },
        )
        .command(
          "list-channels",
          "List configured notification channels",
          () => {},
          async () => {
            const config = await loadGlobalConfig();
            const channels = config.notificationChannels ?? [];
            if (channels.length === 0) {
              console.log("No notification channels configured.");
              console.log("Add one with: openloop config add-channel --type webhook --url <URL>");
              return;
            }
            for (let i = 0; i < channels.length; i++) {
              const ch = channels[i]!;
              const events = ch.events.join(", ") || "*";
              if (ch.type === "webhook") {
                console.log(`[${i}] webhook → ${ch.url} (events: ${events})`);
              } else {
                console.log(`[${i}] desktop (events: ${events})`);
              }
            }
          },
        )
        .demandCommand(),
  );
}