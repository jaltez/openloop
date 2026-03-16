import { readJsonFile, writeJsonFile } from "./fs.js";
import { globalConfigPath } from "./paths.js";
import type { GlobalConfig } from "./types.js";

const DEFAULT_GLOBAL_CONFIG: GlobalConfig = {
  version: 1,
  model: null,
  activeProjectAlias: null,
  budgets: {
    dailyCostUsd: 25,
    estimatedCostPerRunUsd: 0.10,
  },
  runtime: {
    runTimeoutSeconds: 1800,
    maxAttemptsPerTask: 3,
    noProgressRepeatLimit: 2,
    tickIntervalSeconds: 5,
    projectSelectionStrategy: "round-robin" as const,
  },
  notifications: {
    onTaskComplete: null,
    onTaskFailed: null,
    onBudgetBlocked: null,
    onAllTasksDone: null,
  },
};

export async function loadGlobalConfig(appHomeOverride?: string): Promise<GlobalConfig> {
  const config = await readJsonFile<Partial<GlobalConfig>>(globalConfigPath(appHomeOverride), DEFAULT_GLOBAL_CONFIG);
  return {
    version: 1,
    model: config.model ?? DEFAULT_GLOBAL_CONFIG.model,
    activeProjectAlias: config.activeProjectAlias ?? DEFAULT_GLOBAL_CONFIG.activeProjectAlias,
    budgets: {
      dailyCostUsd: config.budgets?.dailyCostUsd ?? DEFAULT_GLOBAL_CONFIG.budgets.dailyCostUsd,
      estimatedCostPerRunUsd: config.budgets?.estimatedCostPerRunUsd ?? DEFAULT_GLOBAL_CONFIG.budgets.estimatedCostPerRunUsd,
    },
    runtime: {
      runTimeoutSeconds: config.runtime?.runTimeoutSeconds ?? DEFAULT_GLOBAL_CONFIG.runtime.runTimeoutSeconds,
      maxAttemptsPerTask: config.runtime?.maxAttemptsPerTask ?? DEFAULT_GLOBAL_CONFIG.runtime.maxAttemptsPerTask,
      noProgressRepeatLimit: config.runtime?.noProgressRepeatLimit ?? DEFAULT_GLOBAL_CONFIG.runtime.noProgressRepeatLimit,
      tickIntervalSeconds: config.runtime?.tickIntervalSeconds ?? DEFAULT_GLOBAL_CONFIG.runtime.tickIntervalSeconds,
      projectSelectionStrategy: config.runtime?.projectSelectionStrategy ?? DEFAULT_GLOBAL_CONFIG.runtime.projectSelectionStrategy,
    },
    notifications: {
      onTaskComplete: config.notifications?.onTaskComplete ?? null,
      onTaskFailed: config.notifications?.onTaskFailed ?? null,
      onBudgetBlocked: config.notifications?.onBudgetBlocked ?? null,
      onAllTasksDone: config.notifications?.onAllTasksDone ?? null,
    },
  };
}

export async function saveGlobalConfig(config: GlobalConfig, appHomeOverride?: string): Promise<void> {
  await writeJsonFile(globalConfigPath(appHomeOverride), config);
}