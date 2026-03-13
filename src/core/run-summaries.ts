import fs from "node:fs/promises";
import path from "node:path";
import { ensureDir } from "./fs.js";
import type { SchedulerResult } from "./types.js";

export async function writeRunSummary(projectPath: string, summary: SchedulerResult): Promise<string> {
  const runsDir = path.join(projectPath, ".openloop", "runs");
  await ensureDir(runsDir);
  const validation = summary.validation ?? [];
  const promotionDecision = summary.promotionDecision ?? "none";
  const promotionAction = summary.promotionAction ?? "none";
  const taskStatus = summary.taskStatus ?? "none";
  const promotedAt = summary.promotedAt ?? "none";
  const stoppedBy = summary.stoppedBy ?? "none";
  const attemptNumber = summary.attemptNumber ?? "none";
  const dirtyTreeDetected = String(summary.dirtyTreeDetected ?? false);
  const budgetSnapshotUsd = summary.budgetSnapshotUsd ?? "none";
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  const taskId = summary.taskId ?? "idle";
  const filename = `${timestamp}-${taskId}.md`;
  const filePath = path.join(runsDir, filename);
  const content = [
    `# Run Summary`,
    "",
    `- project: ${summary.projectAlias}`,
    `- task: ${summary.taskId ?? "none"}`,
    `- mode: ${summary.mode}`,
    `- role: ${summary.role ?? "none"}`,
    `- reason: ${summary.reason}`,
    `- model: ${summary.model ?? "none"}`,
    `- exitCode: ${summary.exitCode ?? "none"}`,
    `- taskStatus: ${taskStatus}`,
    `- promotedAt: ${promotedAt}`,
    `- promotionDecision: ${promotionDecision}`,
    `- promotionAction: ${promotionAction}`,
    `- promotionArtifactPath: ${summary.promotionArtifactPath ?? "none"}`,
    `- promotionResultArtifactPath: ${summary.promotionResultArtifactPath ?? "none"}`,
    `- stoppedBy: ${stoppedBy}`,
    `- attemptNumber: ${attemptNumber}`,
    `- dirtyTreeDetected: ${dirtyTreeDetected}`,
    `- budgetSnapshotUsd: ${budgetSnapshotUsd}`,
    `- validationCount: ${validation.length}`,
    `- createdAt: ${new Date().toISOString()}`,
    "",
    `## Validation`,
    ...(validation.length > 0
      ? validation.map((item) => `- ${item.name}: ${item.command} => ${item.exitCode}`)
      : ["- none"]),
  ].join("\n");
  await fs.writeFile(filePath, `${content}\n`, "utf8");
  return filePath;
}