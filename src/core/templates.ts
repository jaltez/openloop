import fs from "node:fs/promises";
import path from "node:path";
import { copyTree, ensureDir, fileExists, readJsonFile, writeJsonFile } from "./fs.js";
import { detectValidationCommands } from "./stack-detection.js";
import { templateRoot } from "./paths.js";
import type { LinkedProject } from "./types.js";

export async function initializeProjectFromTemplates(repoRoot: string, project: LinkedProject, options?: { force?: boolean }): Promise<void> {
  const templatesRoot = templateRoot(repoRoot);
  const openloopDir = path.join(project.path, ".openloop");

  // W8: Back up existing control-plane files before re-materializing with --force.
  if (options?.force && (await fileExists(openloopDir))) {
    const backupDir = path.join(openloopDir, "backup", new Date().toISOString().replace(/[:.]/g, "-"));
    await ensureDir(backupDir);
    const entries = await fs.readdir(openloopDir, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.name === "backup") continue;
      const src = path.join(openloopDir, entry.name);
      const dest = path.join(backupDir, entry.name);
      await fs.cp(src, dest, { recursive: true }).catch(() => {});
    }
  }

  await copyTree(templatesRoot, project.path, { overwrite: options?.force ?? false });

  const templateProjectConfigPath = path.join(templatesRoot, ".openloop", "project.json");
  const projectConfigPath = path.join(project.path, ".openloop", "project.json");
  const templateProjectConfig = await readJsonFile<Record<string, unknown>>(templateProjectConfigPath, {});
  const existingProjectConfig = await readJsonFile<Record<string, unknown>>(projectConfigPath, {});
  const projectConfig = mergeJsonObjects(templateProjectConfig, existingProjectConfig);
  projectConfig.project = {
    alias: project.alias,
    repoRoot: project.path,
    initializedAt: new Date().toISOString(),
  };
  projectConfig.validation = await detectValidationCommands(project.path);
  await writeJsonFile(projectConfigPath, projectConfig);

  const tasksPath = path.join(project.path, ".openloop", "tasks.json");
  if (await fileExists(tasksPath)) {
    const ledger = await readJsonFile<Record<string, unknown>>(tasksPath, {});
    ledger.updatedAt = new Date().toISOString();
    await writeJsonFile(tasksPath, ledger);
  }
}

function mergeJsonObjects(base: Record<string, unknown>, override: Record<string, unknown>): Record<string, unknown> {
  const merged: Record<string, unknown> = { ...base };

  for (const [key, value] of Object.entries(override)) {
    const baseValue = merged[key];
    if (isPlainObject(baseValue) && isPlainObject(value)) {
      merged[key] = mergeJsonObjects(baseValue, value);
      continue;
    }

    merged[key] = value;
  }

  return merged;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}