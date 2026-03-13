import { readJsonFile, writeJsonFile } from "./fs.js";
import { globalConfigPath } from "./paths.js";
import type { GlobalConfig } from "./types.js";

const DEFAULT_GLOBAL_CONFIG: GlobalConfig = {
  version: 1,
  model: null,
  activeProjectAlias: null,
  budgets: {
    dailyCostUsd: 25,
  },
  runtime: {
    runTimeoutSeconds: 1800,
    maxAttemptsPerTask: 3,
    noProgressRepeatLimit: 2,
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
    },
    runtime: {
      runTimeoutSeconds: config.runtime?.runTimeoutSeconds ?? DEFAULT_GLOBAL_CONFIG.runtime.runTimeoutSeconds,
      maxAttemptsPerTask: config.runtime?.maxAttemptsPerTask ?? DEFAULT_GLOBAL_CONFIG.runtime.maxAttemptsPerTask,
      noProgressRepeatLimit: config.runtime?.noProgressRepeatLimit ?? DEFAULT_GLOBAL_CONFIG.runtime.noProgressRepeatLimit,
    },
  };
}

export async function saveGlobalConfig(config: GlobalConfig, appHomeOverride?: string): Promise<void> {
  await writeJsonFile(globalConfigPath(appHomeOverride), config);
}