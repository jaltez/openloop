import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { expect, test } from "vitest";
import { runProjectIteration } from "../../src/core/scheduler.js";
import { fakeAgentRun } from "../helpers/factories.js";
import type { LinkedProject, ProjectConfig, TaskLedger } from "../../src/core/types.js";

async function seedProject(): Promise<{ projectRoot: string; project: LinkedProject }> {
  const projectRoot = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-verifier-"));
  await fs.mkdir(path.join(projectRoot, ".openloop"), { recursive: true });

  const projectConfig: ProjectConfig = {
    version: 1,
    project: { alias: "demo", repoRoot: projectRoot, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: "true", typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "low-risk", requirePolicyForAutoMerge: true },
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(projectConfig, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(projectRoot, ".openloop", "policy.yaml"), "version: 1\n", "utf8");

  const ledger: TaskLedger = {
    version: 1,
    updatedAt: new Date().toISOString(),
    tasks: [
      {
        id: "verify-task",
        title: "Verified task",
        kind: "feature",
        status: "ready",
        risk: "low-risk",
        source: { type: "human", ref: "test" },
        specId: null,
        branch: null,
        owner: null,
        acceptanceCriteria: ["Criterion one", "Criterion two"],
        attempts: 0,
        lastFailureSignature: null,
        promotion: "auto-merge",
        notes: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ],
  };
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  const project: LinkedProject = {
    alias: "demo",
    path: projectRoot,
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  };
  return { projectRoot, project };
}

async function writeVerdicts(projectRoot: string, content: string): Promise<void> {
  const dir = path.join(projectRoot, ".openloop", "verifications");
  await fs.mkdir(dir, { recursive: true });
  await fs.writeFile(path.join(dir, "verify-task.json"), content, "utf8");
}

test("verifier fail verdict fails the task", async () => {
  const { projectRoot, project } = await seedProject();
  const verifierRunner = async (prompt: string) => {
    expect(prompt).toContain("verification role");
    expect(prompt).toContain(projectRoot); // absolute control-plane path
    await writeVerdicts(
      projectRoot,
      JSON.stringify([
        { index: 1, criterion: "Criterion one", verdict: "pass", evidence: "ok" },
        { index: 2, criterion: "Criterion two", verdict: "fail", evidence: "missing" },
      ]),
    );
    return 0;
  };

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
    validationRunner: async () => 0,
    verifierRunner,
  });

  expect(result.taskStatus).toBe("failed");
  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.attempts).toBe(1);
  expect(persisted.tasks[0]?.lastFailureSignature).toBe("verifier-rejected");
  expect(persisted.tasks[0]?.notes?.some((note) => note.includes("Verifier rejected criterion 2"))).toBe(true);
});

test("verifier needs-human verdict forces manual review but completes the task", async () => {
  const { projectRoot, project } = await seedProject();
  const verifierRunner = async () => {
    await writeVerdicts(
      projectRoot,
      JSON.stringify([
        { index: 1, criterion: "Criterion one", verdict: "pass", evidence: "ok" },
        { index: 2, criterion: "Criterion two", verdict: "needs-human", evidence: "can't tell" },
      ]),
    );
    return 0;
  };

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
    validationRunner: async () => 0,
    verifierRunner,
  });

  expect(result.taskStatus).toBe("done");
  expect(result.promotionDecision).toBe("manual-review");
  expect(result.promotionAction).toBe("queue-review");
  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.notes?.some((note) => note.includes("Verifier verdict requires human review."))).toBe(true);
});

test("missing or malformed verifier output forces manual review", async () => {
  const { projectRoot, project } = await seedProject();
  await writeVerdicts(projectRoot, "{ this is not json");

  const result = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
    validationRunner: async () => 0,
    verifierRunner: async () => 0, // runs but the file it wrote is unreadable
  });

  expect(result.taskStatus).toBe("done");
  expect(result.promotionDecision).toBe("manual-review");
  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  expect(persisted.tasks[0]?.notes?.some((note) => note.includes("Verifier verdict requires human review."))).toBe(true);
});

test("verification disabled skips the stage entirely", async () => {
  const { projectRoot, project } = await seedProject();
  const config = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "project.json"), "utf8")) as ProjectConfig;
  config.verification = { enabled: false };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(config, null, 2)}\n`, "utf8");

  let runnerCalls = 0;
  const result = await runProjectIteration(project, {
    piRunner: async () => {
      runnerCalls += 1;
      return fakeAgentRun();
    },
    validationRunner: async () => 0,
  });

  expect(runnerCalls).toBe(1); // no verifier invocation
  expect(result.taskStatus).toBe("done");
  expect(result.promotionAction).toBe("queue-auto-merge");
});

test("stale verdicts from a previous attempt are not replayed against a retry", async () => {
  const { projectRoot, project } = await seedProject();

  // Attempt 1: the verifier writes a fail verdict.
  const first = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
    validationRunner: async () => 0,
    noProgressRepeatLimit: 10,
    verifierRunner: async () => {
      await writeVerdicts(projectRoot, JSON.stringify([{ index: 1, criterion: "Criterion one", verdict: "fail", evidence: "broken" }]));
      return 0;
    },
  });
  expect(first.taskStatus).toBe("failed");

  // Reset the failed task to ready (a retry cycle) and fill out the
  // validation config so no continuous-improvement task preempts it.
  const config = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "project.json"), "utf8")) as ProjectConfig;
  config.validation = { lintCommand: "true", testCommand: "true", typecheckCommand: "true" };
  await fs.writeFile(path.join(projectRoot, ".openloop", "project.json"), `${JSON.stringify(config, null, 2)}\n`, "utf8");
  const ledger = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  ledger.tasks[0]!.status = "ready";
  await fs.writeFile(path.join(projectRoot, ".openloop", "tasks.json"), `${JSON.stringify(ledger, null, 2)}\n`, "utf8");

  // Attempt 2: the verifier never runs successfully (simulated crash) — the
  // old fail verdict must not be replayed; the run degrades to human review.
  const second = await runProjectIteration(project, {
    piRunner: async () => fakeAgentRun(),
    validationRunner: async () => 0,
    noProgressRepeatLimit: 10,
    verifierRunner: async () => {
      throw new Error("verifier crashed");
    },
  });
  expect(second.taskStatus).toBe("done");
  expect(second.promotionAction).toBe("queue-review");
  const persisted = JSON.parse(await fs.readFile(path.join(projectRoot, ".openloop", "tasks.json"), "utf8")) as TaskLedger;
  // The stale fail verdict was not replayed: the retry completed instead of
  // failing again, and degraded to human review because the verifier left no
  // verdicts of its own.
  expect(persisted.tasks[0]?.status).toBe("done");
  expect(persisted.tasks[0]?.notes?.some((note) => note.includes("Verifier verdict requires human review."))).toBe(true);
});
