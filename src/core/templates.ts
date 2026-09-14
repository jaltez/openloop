import fs from "node:fs/promises";
import path from "node:path";
import { copyTree, ensureDir, fileExists, readJsonFile, writeJsonFile } from "./fs.js";
import { detectValidationCommands } from "./stack-detection.js";
import { templateRoot } from "./paths.js";
import type { LinkedProject } from "./types.js";

export async function initializeProjectFromTemplates(
  repoRoot: string,
  project: LinkedProject,
  options?: { force?: boolean },
): Promise<void> {
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

  // Non-Pi providers (claude, codex, …) read AGENTS.md at the repo root.
  // Materialize the openloop conventions there: create when absent, otherwise
  // merge a clearly fenced section so the project's own instructions survive.
  await materializeAgentsMd(project.path);

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

const OPENLOOP_AGENTS_SECTION_START = "<!-- openloop:start -->";
const OPENLOOP_AGENTS_SECTION_END = "<!-- openloop:end -->";

// Conventions for non-Pi providers, mirroring the openloop skill: same control
// plane files, workflow, and runtime-managed-file boundaries.
function openloopAgentsSection(): string {
  return [
    OPENLOOP_AGENTS_SECTION_START,
    "",
    "## Openloop",
    "",
    "This repository is linked to the Openloop control plane. Before planning or changing code, read:",
    "",
    "1. `.openloop/tasks.json` — task ledger with status, risk, scope, and acceptance criteria",
    "2. `.openloop/policy.yaml` — scope rules (`allowGlobs`, `denyGlobs`, `highRiskAreas`), risk classes, and promotion modes",
    "3. `.openloop/project.json` — validation commands and runtime settings",
    "4. Relevant spec under `.openloop/specs/` if one exists for the current task",
    "",
    "Workflow: implement with minimal, focused changes within the task's declared",
    "scope; run the project's validation commands (lint, test, typecheck) as",
    "configured in `project.json`; stop on validation failure.",
    "",
    "Do not modify runtime-managed files directly: `.openloop/tasks.json`,",
    "`.openloop/runs/`, `.openloop/promotions/`, `.openloop/promotion-results/`.",
    "",
    OPENLOOP_AGENTS_SECTION_END,
    "",
  ].join("\n");
}

async function materializeAgentsMd(projectPath: string): Promise<void> {
  const agentsMdPath = path.join(projectPath, "AGENTS.md");
  if (!(await fileExists(agentsMdPath))) {
    await fs.writeFile(agentsMdPath, openloopAgentsSection(), "utf8");
    return;
  }

  const existing = await fs.readFile(agentsMdPath, "utf8");
  const startIndex = existing.indexOf(OPENLOOP_AGENTS_SECTION_START);
  const endIndex = existing.indexOf(OPENLOOP_AGENTS_SECTION_END);
  let merged = existing;
  if (startIndex !== -1 && endIndex !== -1 && endIndex > startIndex) {
    // Replace any previous openloop block in place.
    merged = `${existing.slice(0, startIndex)}${openloopAgentsSection()}${existing.slice(endIndex + OPENLOOP_AGENTS_SECTION_END.length)}`;
  } else if (startIndex !== -1) {
    // Asymmetric markers (start without end): discard everything from the
    // dangling start marker onward — the openloop block is replaced wholesale.
    merged = `${existing.slice(0, startIndex).trimEnd()}\n\n${openloopAgentsSection()}`;
  } else if (endIndex !== -1) {
    // Orphaned end marker: drop it and append the canonical section.
    merged = `${existing.slice(0, endIndex)}${openloopAgentsSection()}${existing.slice(endIndex + OPENLOOP_AGENTS_SECTION_END.length)}`;
  } else {
    merged = `${existing.trimEnd()}\n\n${openloopAgentsSection()}`;
  }
  await fs.writeFile(agentsMdPath, merged, "utf8");
}
