import type { Argv, ArgumentsCamelCase } from "yargs";
import { getProject } from "../../core/project-registry.js";
import { loadGlobalConfig, saveGlobalConfig } from "../../core/global-config.js";
import { loadProjectConfig, saveProjectConfig } from "../../core/project-config.js";

type ModelArgs = ArgumentsCamelCase<{ model: string }>;
type ProjectArgs = ArgumentsCamelCase<{ project: string }>;
type ProjectModelArgs = ArgumentsCamelCase<{ project: string; model: string }>;
type ConfigSetArgs = ArgumentsCamelCase<{ key: string; value: string }>;

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
        .demandCommand(),
  );
}