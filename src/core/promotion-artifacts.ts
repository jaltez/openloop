import fs from "node:fs/promises";
import path from "node:path";
import { ensureDir, fileExists, readJsonFile, writeJsonFile } from "./fs.js";
import type { PromotionArtifact, PromotionResultArtifact } from "./types.js";

export async function writePromotionArtifact(projectPath: string, artifact: PromotionArtifact): Promise<string> {
  const promotionsDir = path.join(projectPath, ".openloop", "promotions");
  await ensureDir(promotionsDir);
  const timestamp = artifact.createdAt.replace(/[:.]/g, "-");
  const filePath = path.join(promotionsDir, `${timestamp}-${artifact.taskId}.json`);
  await writeJsonFile(filePath, artifact);
  return filePath;
}

export async function writePromotionResultArtifact(projectPath: string, artifact: PromotionResultArtifact): Promise<string> {
  const resultsDir = path.join(projectPath, ".openloop", "promotion-results");
  await ensureDir(resultsDir);
  const timestamp = artifact.createdAt.replace(/[:.]/g, "-");
  const filePath = path.join(resultsDir, `${timestamp}-${artifact.taskId}.json`);
  await writeJsonFile(filePath, artifact);
  return filePath;
}

export async function readPromotionResultArtifact(filePath: string): Promise<PromotionResultArtifact | null> {
  if (!(await fileExists(filePath))) {
    return null;
  }

  return readJsonFile<PromotionResultArtifact>(filePath, {
    version: 1,
    createdAt: new Date(0).toISOString(),
    projectAlias: "",
    taskId: "",
    sourcePromotionArtifactPath: "",
    sourcePromotionAction: "none",
    sourcePromotionDecision: "none",
    result: "rejected",
    branch: null,
    baseBranch: null,
    note: null,
    prUrl: null,
  });
}

export async function listPromotionResultArtifacts(projectPath: string, taskId?: string): Promise<Array<{ artifactPath: string; artifact: PromotionResultArtifact }>> {
  const resultsDir = path.join(projectPath, ".openloop", "promotion-results");
  if (!(await fileExists(resultsDir))) {
    return [];
  }

  const entries = await fs.readdir(resultsDir, { withFileTypes: true });
  const items: Array<{ artifactPath: string; artifact: PromotionResultArtifact }> = [];
  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.endsWith(".json")) {
      continue;
    }
    const artifactPath = path.join(resultsDir, entry.name);
    const artifact = await readPromotionResultArtifact(artifactPath);
    if (!artifact) {
      continue;
    }
    if (taskId && artifact.taskId !== taskId) {
      continue;
    }
    items.push({ artifactPath, artifact });
  }

  return items.sort((left, right) => right.artifact.createdAt.localeCompare(left.artifact.createdAt));
}