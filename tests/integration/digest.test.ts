import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, expect, test } from "vitest";
import { buildDigest, computeConfidence } from "../../src/core/digest.js";
import { addProject, markProjectInitialized } from "../../src/core/project-registry.js";
import { writeApprovalPacket } from "../../src/core/approval-packets.js";
import { makeProjectTask } from "../helpers/factories.js";
import type { ApprovalPacket } from "../../src/core/approval-packets.js";
import type { PromotionArtifact } from "../../src/core/types.js";

const tempDirs: string[] = [];
const originalOpenloopHome = process.env.OPENLOOP_HOME;

afterEach(async () => {
  process.env.OPENLOOP_HOME = originalOpenloopHome;
  for (const tempDir of tempDirs.splice(0)) {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
});

function pendingArtifact(taskId: string, validation: Array<{ name: "lint" | "test" | "typecheck"; exitCode: number }>): PromotionArtifact {
  return {
    version: 1,
    createdAt: new Date().toISOString(),
    projectAlias: "demo",
    taskId,
    baseBranch: "main",
    decision: "manual-review",
    action: "queue-review",
    effectivePromotionMode: "pull-request",
    validation: validation.map((entry) => ({ ...entry, command: `${entry.name}-cmd` })),
    piExitCode: 0,
    outcome: "completed",
    status: "pending",
    processedAt: null,
    note: null,
  };
}

test("computeConfidence grades pending promotions deterministically", () => {
  const packet = (overrides: Partial<ApprovalPacket>): ApprovalPacket => ({
    version: 1,
    createdAt: new Date().toISOString(),
    taskId: "t",
    title: "T",
    risk: "low-risk",
    scope: null,
    acceptanceCriteria: [],
    branch: null,
    diffStat: null,
    validation: [],
    reviewFindings: [],
    costUsd: 0.1,
    costSource: "estimated",
    attempts: 1,
    runSummaryPath: null,
    specPath: null,
    ...overrides,
  });
  const validation = [{ name: "test" as const, exitCode: 0 }];
  const configured = ["test"];

  expect(computeConfidence({ artifact: pendingArtifact("t", validation), packet: packet({}), configuredValidationNames: configured })).toBe(
    "high",
  );
  // Second attempt drops to medium.
  expect(
    computeConfidence({
      artifact: pendingArtifact("t", validation),
      packet: packet({ attempts: 2 }),
      configuredValidationNames: configured,
    }),
  ).toBe("medium");
  // Medium risk drops to medium.
  expect(
    computeConfidence({
      artifact: pendingArtifact("t", validation),
      packet: packet({ risk: "medium-risk" }),
      configuredValidationNames: configured,
    }),
  ).toBe("medium");
  // Blocking finding drops to low.
  expect(
    computeConfidence({
      artifact: pendingArtifact("t", validation),
      packet: packet({ reviewFindings: [{ rule: "agent-review", severity: "block", message: "nope" }] }),
      configuredValidationNames: configured,
    }),
  ).toBe("low");
  // Failed validation drops to low.
  expect(
    computeConfidence({
      artifact: pendingArtifact("t", [{ name: "test", exitCode: 1 }]),
      packet: packet({}),
      configuredValidationNames: configured,
    }),
  ).toBe("low");
  // Missing packet (unverifiable provenance) is low.
  expect(computeConfidence({ artifact: pendingArtifact("t", validation), packet: null, configuredValidationNames: configured })).toBe(
    "low",
  );
});

test("buildDigest aggregates ledgers, run summaries, and pending promotions", async () => {
  const appHome = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-home-"));
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-project-"));
  tempDirs.push(appHome, projectRoot);
  process.env.OPENLOOP_HOME = appHome;

  await addProject("demo", projectRoot, appHome);
  await markProjectInitialized("demo", appHome);
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const done = makeProjectTask({ id: "done-task", title: "Done", status: "done", risk: "low-risk" });
  done.lastRun = {
    completedAt: new Date().toISOString(),
    mode: "implement",
    piExitCode: 0,
    outcome: "completed",
    baseBranch: "main",
    validation: [],
    promotionDecision: "manual-review",
    effectivePromotionMode: "pull-request",
    promotionAction: "queue-review",
    promotionArtifactPath: null,
    promotionArtifactState: "pending",
    promotionResultArtifactPath: null,
  };
  const failed = makeProjectTask({ id: "failed-task", title: "Failed", status: "failed" });
  failed.lastRun = { ...done.lastRun!, outcome: "validation-failed" };
  const blocked = makeProjectTask({ id: "blocked-task", title: "Blocked", status: "blocked" });
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "tasks.json"),
    `${JSON.stringify({ version: 1, updatedAt: new Date(0).toISOString(), tasks: [done, failed, blocked] }, null, 2)}\n`,
  );

  // Config with one validation so confidence is computable.
  await fs.writeFile(
    path.join(projectRoot, ".openloop", "project.json"),
    `${JSON.stringify(
      {
        version: 1,
        project: { alias: "demo", repoRoot: null, initializedAt: null },
        pi: { model: null, promptFiles: [] },
        runtime: { useWorktree: false, branchPrefix: "openloop/" },
        validation: { lintCommand: null, testCommand: "bun test", typecheckCommand: null },
        risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
      },
      null,
      2,
    )}\n`,
  );

  // Two run summaries with measured/estimated cost split.
  const runsDir = path.join(projectRoot, ".openloop", "runs");
  await fs.mkdir(runsDir, { recursive: true });
  const summary = (costUsd: string, costSource: string) =>
    `# Run Summary\n\n- costUsd: ${costUsd}\n- costSource: ${costSource}\n- createdAt: ${new Date().toISOString()}\n`;
  await fs.writeFile(path.join(runsDir, "a-measured.md"), summary("0.42", "measured"));
  await fs.writeFile(path.join(runsDir, "b-estimated.md"), summary("0.10", "estimated"));

  // One pending promotion with an approval packet (first attempt, low risk, validations passed).
  const pending = makeProjectTask({ id: "pending-task", title: "Pending", status: "done", risk: "low-risk", attempts: 1 });
  await writeApprovalPacket(projectRoot, pending, {
    diffStat: null,
    validation: [{ name: "test", command: "bun test", exitCode: 0 }],
    reviewFindings: [],
    costUsd: 0.42,
    costSource: "measured",
    runSummaryPath: null,
    specPath: null,
  });
  const promotionsDir = path.join(projectRoot, ".openloop", "promotions");
  await fs.mkdir(promotionsDir, { recursive: true });
  await fs.writeFile(
    path.join(promotionsDir, "2026-03-09T10-00-00-000Z-pending-task.json"),
    `${JSON.stringify(pendingArtifact("pending-task", [{ name: "test", exitCode: 0 }]), null, 2)}\n`,
  );

  const digest = await buildDigest({ sinceMs: 60 * 60 * 1000 });

  expect(digest.projects).toHaveLength(1);
  const project = digest.projects[0]!;
  expect(project.alias).toBe("demo");
  expect(project.completed).toBe(1);
  expect(project.failed).toBe(1);
  expect(project.blocked).toBe(1);
  expect(project.promoted).toBe(0);
  expect(project.spendUsd).toEqual({ measured: 0.42, estimated: 0.1 });
  expect(project.reviewQueue).toEqual([{ taskId: "pending-task", title: "Pending", risk: "low-risk", confidence: "high" }]);
});
