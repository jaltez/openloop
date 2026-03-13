import { listPromotionResultArtifacts } from "./promotion-artifacts.js";
import { listPromotionArtifactsForTask } from "./promotion-queue.js";
import { getTask } from "./task-ledger.js";
import type { PromotionArtifact, PromotionResultArtifact, ProjectTask } from "./types.js";

export interface TaskInspectionHistoryEntry {
  createdAt: string;
  kind: "request" | "result";
  artifactPath: string;
  payload: PromotionArtifact | PromotionResultArtifact;
}

export interface TaskInspection {
  task: ProjectTask;
  latestPromotionRequest: { artifactPath: string; artifact: PromotionArtifact } | null;
  latestPromotionResult: { artifactPath: string; artifact: PromotionResultArtifact } | null;
  promotionHistory: TaskInspectionHistoryEntry[];
}

export async function getTaskInspection(projectPath: string, taskId: string): Promise<TaskInspection> {
  const task = await getTask(projectPath, taskId);
  const promotionRequests = await listPromotionArtifactsForTask(projectPath, taskId);
  const promotionResults = await listPromotionResultArtifacts(projectPath, taskId);

  const promotionHistory: TaskInspectionHistoryEntry[] = [
    ...promotionRequests.map((item) => ({
      createdAt: item.artifact.createdAt,
      kind: "request" as const,
      artifactPath: item.artifactPath,
      payload: item.artifact,
    })),
    ...promotionResults.map((item) => ({
      createdAt: item.artifact.createdAt,
      kind: "result" as const,
      artifactPath: item.artifactPath,
      payload: item.artifact,
    })),
  ].sort((left, right) => left.createdAt.localeCompare(right.createdAt));

  return {
    task,
    latestPromotionRequest: promotionRequests[0] ?? null,
    latestPromotionResult: promotionResults[0] ?? null,
    promotionHistory,
  };
}