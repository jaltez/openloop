import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { expect, test } from "vitest";
import {
  ensureCleanGitRepo,
  getGitDiffFingerprint,
  getGitWorkingTreeState,
  checkoutHandoffBranch,
  stageAllChanges,
  commitStagedChanges,
  mergeFastForward,
  getBranchHead,
  getMergeBase,
} from "../../src/core/git.js";

const execFileAsync = promisify(execFile);

async function initGitRepo(): Promise<string> {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-git-"));
  await execFileAsync("git", ["init"], { cwd: dir });
  await execFileAsync("git", ["config", "user.email", "test@test.com"], { cwd: dir });
  await execFileAsync("git", ["config", "user.name", "Test"], { cwd: dir });
  await fs.writeFile(path.join(dir, "README.md"), "init\n", "utf8");
  await execFileAsync("git", ["add", "."], { cwd: dir });
  await execFileAsync("git", ["commit", "-m", "initial"], { cwd: dir });
  return dir;
}

test("ensureCleanGitRepo passes on a clean repo", async () => {
  const dir = await initGitRepo();
  await expect(ensureCleanGitRepo(dir)).resolves.toBeUndefined();
});

test("ensureCleanGitRepo throws on a dirty repo", async () => {
  const dir = await initGitRepo();
  await fs.writeFile(path.join(dir, "dirty.txt"), "dirty\n", "utf8");
  await expect(ensureCleanGitRepo(dir)).rejects.toThrow("dirty");
});

test("getGitWorkingTreeState returns branch and dirty state", async () => {
  const dir = await initGitRepo();
  const state = await getGitWorkingTreeState(dir);
  expect(state.currentBranch).toBeTruthy();
  expect(state.dirty).toBe(false);

  await fs.writeFile(path.join(dir, "new.txt"), "change\n", "utf8");
  const dirtyState = await getGitWorkingTreeState(dir);
  expect(dirtyState.dirty).toBe(true);
});

test("getGitDiffFingerprint returns empty string for clean repo", async () => {
  const dir = await initGitRepo();
  const fingerprint = await getGitDiffFingerprint(dir);
  expect(fingerprint).toBe("");
});

test("getGitDiffFingerprint returns non-empty for dirty repo", async () => {
  const dir = await initGitRepo();
  await fs.writeFile(path.join(dir, "new.txt"), "change\n", "utf8");
  const fingerprint = await getGitDiffFingerprint(dir);
  expect(fingerprint).toBeTruthy();
  expect(fingerprint).toContain("new.txt");
});

test("getGitDiffFingerprint returns null for non-git directory", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "openloop-no-git-"));
  const fingerprint = await getGitDiffFingerprint(dir);
  expect(fingerprint).toBeNull();
});

test("stageAllChanges and commitStagedChanges create a commit", async () => {
  const dir = await initGitRepo();
  await fs.writeFile(path.join(dir, "file.txt"), "content\n", "utf8");
  await stageAllChanges(dir);
  await commitStagedChanges(dir, "add file");

  const state = await getGitWorkingTreeState(dir);
  expect(state.dirty).toBe(false);

  const log = await execFileAsync("git", ["log", "--oneline"], { cwd: dir });
  expect(log.stdout).toContain("add file");
});

test("checkoutHandoffBranch creates and switches to a new branch", async () => {
  const dir = await initGitRepo();
  await checkoutHandoffBranch(dir, "openloop/test-branch");

  const state = await getGitWorkingTreeState(dir);
  expect(state.currentBranch).toBe("openloop/test-branch");
});

test("checkoutHandoffBranch switches to existing branch", async () => {
  const dir = await initGitRepo();
  await execFileAsync("git", ["checkout", "-b", "existing-branch"], { cwd: dir });
  await execFileAsync("git", ["checkout", "-"], { cwd: dir });

  await checkoutHandoffBranch(dir, "existing-branch");
  const state = await getGitWorkingTreeState(dir);
  expect(state.currentBranch).toBe("existing-branch");
});

test("mergeFastForward merges a branch fast-forward", async () => {
  const dir = await initGitRepo();
  const mainBranch = (await getGitWorkingTreeState(dir)).currentBranch!;

  await execFileAsync("git", ["checkout", "-b", "feature"], { cwd: dir });
  await fs.writeFile(path.join(dir, "feature.txt"), "feature\n", "utf8");
  await stageAllChanges(dir);
  await commitStagedChanges(dir, "add feature");
  const featureHead = await getBranchHead(dir, "feature");

  await execFileAsync("git", ["checkout", mainBranch], { cwd: dir });
  await mergeFastForward(dir, "feature");

  const mainHead = await getBranchHead(dir, mainBranch);
  expect(mainHead).toBe(featureHead);
});

test("mergeFastForward fails when branches have diverged", async () => {
  const dir = await initGitRepo();
  const mainBranch = (await getGitWorkingTreeState(dir)).currentBranch!;

  await execFileAsync("git", ["checkout", "-b", "feature2"], { cwd: dir });
  await fs.writeFile(path.join(dir, "feature.txt"), "feature\n", "utf8");
  await stageAllChanges(dir);
  await commitStagedChanges(dir, "feature commit");

  await execFileAsync("git", ["checkout", mainBranch], { cwd: dir });
  await fs.writeFile(path.join(dir, "main.txt"), "main\n", "utf8");
  await stageAllChanges(dir);
  await commitStagedChanges(dir, "main commit");

  await expect(mergeFastForward(dir, "feature2")).rejects.toThrow();
});

test("getMergeBase returns the common ancestor commit", async () => {
  const dir = await initGitRepo();
  const mainBranch = (await getGitWorkingTreeState(dir)).currentBranch!;
  const baseCommit = await getBranchHead(dir, mainBranch);

  await execFileAsync("git", ["checkout", "-b", "feature3"], { cwd: dir });
  await fs.writeFile(path.join(dir, "feature.txt"), "feature\n", "utf8");
  await stageAllChanges(dir);
  await commitStagedChanges(dir, "feature commit");

  const mergeBase = await getMergeBase(dir, mainBranch, "feature3");
  expect(mergeBase).toBe(baseCommit);
});
