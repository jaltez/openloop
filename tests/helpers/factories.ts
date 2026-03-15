import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import type { LinkedProject, ProjectConfig, ProjectTask, TaskLedger } from "../../src/core/types.js";

export async function createTempDir(prefix = "openloop-test-"): Promise<string> {
  return fs.mkdtemp(path.join(os.tmpdir(), prefix));
}

export function makeLinkedProject(overrides: Partial<LinkedProject> & { alias: string; path: string }): LinkedProject {
  return {
    defaultBranch: null,
    initialized: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    ...overrides,
  };
}

export function makeProjectTask(overrides: Partial<ProjectTask> & { id: string; title: string }): ProjectTask {
  const now = new Date().toISOString();
  return {
    kind: "feature",
    status: "proposed",
    risk: "medium-risk",
    scope: null,
    source: { type: "human", ref: "test" },
    specId: null,
    branch: null,
    owner: null,
    acceptanceCriteria: [],
    attempts: 0,
    lastFailureSignature: null,
    promotion: "pull-request",
    notes: [],
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

export function makeEmptyLedger(): TaskLedger {
  return {
    version: 1,
    updatedAt: new Date(0).toISOString(),
    tasks: [],
  };
}

export function makeProjectConfigJson(overrides?: Partial<ProjectConfig>): string {
  const config: ProjectConfig = {
    version: 1,
    project: { alias: null, repoRoot: null, initializedAt: null },
    pi: { model: null, promptFiles: [] },
    runtime: { autoCommit: true, useWorktree: false, branchPrefix: "openloop/" },
    validation: { lintCommand: null, testCommand: null, typecheckCommand: null },
    risk: { defaultUnknownAreaClassification: "medium-risk", requirePolicyForAutoMerge: true },
    ...overrides,
  };
  return JSON.stringify(config, null, 2);
}

export async function initGitRepo(dir: string): Promise<void> {
  execFileSync("git", ["init"], { cwd: dir });
  execFileSync("git", ["config", "user.email", "test@test.com"], { cwd: dir });
  execFileSync("git", ["config", "user.name", "Test"], { cwd: dir });
  await fs.writeFile(path.join(dir, "init.txt"), "init\n");
  execFileSync("git", ["add", "."], { cwd: dir });
  execFileSync("git", ["commit", "-m", "init"], { cwd: dir });
}
