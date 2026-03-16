import path from "node:path";
import { readJsonFile, writeJsonFile } from "./fs.js";
import type { ProjectConfig } from "./types.js";

export const DEFAULT_PROJECT_CONFIG: ProjectConfig = {
  version: 1,
  project: {
    alias: null,
    repoRoot: null,
    initializedAt: null,
  },
  pi: {
    model: null,
    promptFiles: [".agents/skills/openloop/SKILL.md", ".pi/SYSTEM.md", ".pi/APPEND_SYSTEM.md"],
  },
  agent: {
    type: "pi" as const,
    command: null,
  },
  runtime: {
    autoCommit: true,
    useWorktree: false,
    branchPrefix: "openloop/",
    prCommand: null,
  },
  validation: {
    lintCommand: null,
    testCommand: null,
    typecheckCommand: null,
  },
  risk: {
    defaultUnknownAreaClassification: "medium-risk",
    requirePolicyForAutoMerge: true,
  },
};

export async function loadProjectConfig(projectPath: string): Promise<ProjectConfig> {
  const configPath = path.join(projectPath, ".openloop", "project.json");
  return readJsonFile<ProjectConfig>(configPath, DEFAULT_PROJECT_CONFIG);
}

export async function saveProjectConfig(projectPath: string, config: ProjectConfig): Promise<void> {
  const configPath = path.join(projectPath, ".openloop", "project.json");
  await writeJsonFile(configPath, config);
}