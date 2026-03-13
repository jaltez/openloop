import { loadGlobalConfig } from "./global-config.js";
import { loadProjectConfig } from "./project-config.js";

export async function resolveModel(projectPath: string, cliOverride?: string, appHomeOverride?: string): Promise<string | null> {
  if (cliOverride && cliOverride.length > 0) {
    return cliOverride;
  }

  const projectConfig = await loadProjectConfig(projectPath);
  if (projectConfig.pi.model && projectConfig.pi.model.length > 0) {
    return projectConfig.pi.model;
  }

  const globalConfig = await loadGlobalConfig(appHomeOverride);
  return globalConfig.model ?? null;
}