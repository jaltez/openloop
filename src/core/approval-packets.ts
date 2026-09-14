import fs from "node:fs/promises";
import path from "node:path";
import { ensureDir, fileExists, readJsonFile, writeJsonFile } from "./fs.js";
import type { DiffStatEntry } from "./git.js";
import type { ProjectTask, ReviewFinding, SchedulerResult, ValidationSummary } from "./types.js";

/**
 * Human-approval packet: everything an approver needs to judge a pending
 * promotion (diff stat, validation, review findings, cost, provenance) in one
 * deterministic JSON artifact at `.openloop/approvals/<taskId>.json`.
 */
export interface ApprovalPacket {
  version: 1;
  createdAt: string;
  taskId: string;
  title: string;
  risk: ProjectTask["risk"];
  scope: ProjectTask["scope"];
  acceptanceCriteria: string[];
  branch: string | null;
  diffStat: DiffStatEntry[] | null;
  validation: ValidationSummary[];
  reviewFindings: ReviewFinding[];
  costUsd: number;
  costSource: Exclude<SchedulerResult["costSource"], null>;
  attempts: number;
  runSummaryPath: string | null;
  specPath: string | null;
}

export interface ApprovalPacketData {
  diffStat: DiffStatEntry[] | null;
  validation: ValidationSummary[];
  reviewFindings: ReviewFinding[];
  costUsd: number;
  costSource: "measured" | "estimated";
  runSummaryPath: string | null;
  specPath: string | null;
}

export function approvalPacketPath(controlPlanePath: string, taskId: string): string {
  return path.join(controlPlanePath, ".openloop", "approvals", `${taskId}.json`);
}

export async function writeApprovalPacket(controlPlanePath: string, task: ProjectTask, data: ApprovalPacketData): Promise<string> {
  const filePath = approvalPacketPath(controlPlanePath, task.id);
  await ensureDir(path.dirname(filePath));
  const packet: ApprovalPacket = {
    version: 1,
    createdAt: new Date().toISOString(),
    taskId: task.id,
    title: task.title,
    risk: task.risk,
    scope: task.scope ?? null,
    acceptanceCriteria: task.acceptanceCriteria,
    branch: task.branch,
    diffStat: data.diffStat,
    validation: data.validation,
    reviewFindings: data.reviewFindings,
    costUsd: data.costUsd,
    costSource: data.costSource,
    attempts: task.attempts,
    runSummaryPath: data.runSummaryPath,
    specPath: data.specPath,
  };
  await writeJsonFile(filePath, packet);
  return filePath;
}

export async function readApprovalPacket(controlPlanePath: string, taskId: string): Promise<ApprovalPacket | null> {
  const filePath = approvalPacketPath(controlPlanePath, taskId);
  if (!(await fileExists(filePath))) {
    return null;
  }
  return readJsonFile<ApprovalPacket>(filePath, null as never);
}

export async function listApprovalPackets(controlPlanePath: string): Promise<Array<{ packetPath: string; packet: ApprovalPacket }>> {
  const approvalsDir = path.join(controlPlanePath, ".openloop", "approvals");
  if (!(await fileExists(approvalsDir))) {
    return [];
  }

  const entries = await fs.readdir(approvalsDir, { withFileTypes: true });
  const items: Array<{ packetPath: string; packet: ApprovalPacket }> = [];
  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.endsWith(".json")) {
      continue;
    }
    const packetPath = path.join(approvalsDir, entry.name);
    const packet = await readJsonFile<ApprovalPacket>(packetPath, null as never);
    if (packet) {
      items.push({ packetPath, packet });
    }
  }

  return items.sort((left, right) => right.packet.createdAt.localeCompare(left.packet.createdAt));
}
