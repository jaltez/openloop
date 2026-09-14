import fs from "node:fs/promises";
import path from "node:path";
import { listProjects } from "./project-registry.js";
import { loadTaskLedger } from "./task-ledger.js";
import { loadProjectConfig } from "./project-config.js";
import { listPromotionArtifacts } from "./promotion-queue.js";
import { readApprovalPacket } from "./approval-packets.js";
import { getConfiguredValidationNames } from "./validation-utils.js";
import type { ApprovalPacket } from "./approval-packets.js";
import type { PromotionArtifact } from "./types.js";

export type DigestConfidence = "high" | "medium" | "low";

export interface FleetDigestReviewQueueEntry {
  taskId: string;
  title: string;
  risk: string;
  confidence: DigestConfidence;
}

export interface FleetDigestProject {
  alias: string;
  completed: number;
  failed: number;
  blocked: number;
  promoted: number;
  spendUsd: {
    measured: number;
    estimated: number;
  };
  reviewQueue: FleetDigestReviewQueueEntry[];
}

export interface FleetDigest {
  generatedAt: string;
  since: string;
  projects: FleetDigestProject[];
}

/**
 * Deterministic confidence for a pending promotion:
 * - high: all configured validations passed AND no block-severity review
 *   findings AND first attempt AND low risk.
 * - medium: validations passed and no blocking findings.
 * - low: anything else (including unverifiable provenance).
 */
export function computeConfidence(input: {
  artifact: PromotionArtifact;
  packet: ApprovalPacket | null;
  configuredValidationNames: string[];
}): DigestConfidence {
  const configured = input.configuredValidationNames;
  const ran: string[] = input.artifact.validation.filter((item) => item.exitCode === 0).map((item) => item.name);
  const validationsPassed = configured.length > 0 && configured.every((name) => ran.includes(name));

  const hasBlockingFinding = input.packet?.reviewFindings.some((finding) => finding.severity === "block") ?? true;
  const attempts = input.packet?.attempts ?? Number.NaN;
  const risk = input.packet?.risk ?? null;

  if (validationsPassed && !hasBlockingFinding) {
    if (attempts === 1 && risk === "low-risk") {
      return "high";
    }
    return "medium";
  }
  return "low";
}

interface RunSummaryCost {
  costUsd: number;
  costSource: "measured" | "estimated";
  completedAt: string;
}

/**
 * Parse cost lines from a run summary written by writeRunSummary. The format
 * is owned by this codebase (`- costUsd: <n>` / `- costSource: <s>` /
 * `- createdAt: <iso>`).
 */
function parseRunSummaryCost(content: string): RunSummaryCost | null {
  const costUsd = /^- costUsd: ([0-9.]+)$/m.exec(content);
  const costSource = /^- costSource: (measured|estimated)$/m.exec(content);
  const createdAt = /^- createdAt: (.+)$/m.exec(content);
  if (!costUsd || !costSource) {
    return null;
  }
  return {
    costUsd: parseFloat(costUsd[1]!),
    costSource: costSource[1] as "measured" | "estimated",
    completedAt: createdAt?.[1] ?? new Date(0).toISOString(),
  };
}

export async function buildDigest(options?: { sinceMs?: number; appHomeOverride?: string }): Promise<FleetDigest> {
  const sinceMs = options?.sinceMs ?? 24 * 60 * 60 * 1000;
  const since = new Date(Date.now() - sinceMs).toISOString();
  const projects = await listProjects(options?.appHomeOverride);

  const entries: FleetDigestProject[] = [];
  for (const project of projects) {
    try {
      entries.push(await buildProjectDigest(project.alias, project.path, since));
    } catch {
      // A single unreadable project must not break the fleet digest.
      entries.push({
        alias: project.alias,
        completed: 0,
        failed: 0,
        blocked: 0,
        promoted: 0,
        spendUsd: { measured: 0, estimated: 0 },
        reviewQueue: [],
      });
    }
  }

  return {
    generatedAt: new Date().toISOString(),
    since,
    projects: entries.sort((left, right) => left.alias.localeCompare(right.alias)),
  };
}

async function buildProjectDigest(alias: string, projectPath: string, since: string): Promise<FleetDigestProject> {
  const ledger = await loadTaskLedger(projectPath);
  const projectConfig = await loadProjectConfig(projectPath).catch(() => null);
  const configuredValidationNames = projectConfig ? getConfiguredValidationNames(projectConfig) : [];

  const completed = ledger.tasks.filter(
    (task) => (task.status === "done" || task.status === "promoted") && (task.lastRun?.completedAt ?? task.updatedAt) >= since,
  ).length;
  const failed = ledger.tasks.filter((task) => task.status === "failed" && (task.lastRun?.completedAt ?? task.updatedAt) >= since).length;
  const blocked = ledger.tasks.filter((task) => task.status === "blocked" && task.updatedAt >= since).length;
  const promoted = ledger.tasks.filter((task) => task.status === "promoted" && (task.promotedAt ?? task.updatedAt) >= since).length;

  const spendUsd = { measured: 0, estimated: 0 };
  const runsDir = path.join(projectPath, ".openloop", "runs");
  const summaries = await fs.readdir(runsDir, { withFileTypes: true }).catch(() => []);
  for (const entry of summaries) {
    if (!entry.isFile() || !entry.name.endsWith(".md")) {
      continue;
    }
    const content = await fs.readFile(path.join(runsDir, entry.name), "utf8").catch(() => null);
    if (!content) {
      continue;
    }
    const cost = parseRunSummaryCost(content);
    if (!cost || cost.completedAt < since) {
      continue;
    }
    spendUsd[cost.costSource] = parseFloat((spendUsd[cost.costSource] + cost.costUsd).toFixed(6));
  }

  const reviewQueue: FleetDigestReviewQueueEntry[] = [];
  const promotions = await listPromotionArtifacts(projectPath);
  for (const { artifact } of promotions) {
    if (artifact.status !== "pending") {
      continue;
    }
    const packet = await readApprovalPacket(projectPath, artifact.taskId);
    const task = ledger.tasks.find((candidate) => candidate.id === artifact.taskId);
    reviewQueue.push({
      taskId: artifact.taskId,
      title: packet?.title ?? task?.title ?? artifact.taskId,
      risk: packet?.risk ?? task?.risk ?? "unknown",
      confidence: computeConfidence({ artifact, packet, configuredValidationNames }),
    });
  }

  return {
    alias,
    completed,
    failed,
    blocked,
    promoted,
    spendUsd,
    reviewQueue,
  };
}
