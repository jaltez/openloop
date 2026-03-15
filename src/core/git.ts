import { spawn } from "node:child_process";

export interface GitWorkingTreeState {
  currentBranch: string | null;
  dirty: boolean;
}

export async function ensureCleanGitRepo(projectPath: string): Promise<void> {
  const result = await runGit(projectPath, ["status", "--porcelain"]);
  if (result.stdout.trim().length > 0) {
    throw new Error("Git working tree is dirty.");
  }
}

export async function checkoutHandoffBranch(projectPath: string, branchName: string): Promise<void> {
  await ensureCleanGitRepo(projectPath);
  if (await branchExists(projectPath, branchName)) {
    await runGit(projectPath, ["checkout", branchName]);
    return;
  }

  await runGit(projectPath, ["checkout", "-b", branchName]);
}

export async function getGitWorkingTreeState(projectPath: string): Promise<GitWorkingTreeState> {
  const branch = await runGit(projectPath, ["branch", "--show-current"]);
  const status = await runGit(projectPath, ["status", "--porcelain"]);
  return {
    currentBranch: branch.stdout.trim() || null,
    dirty: status.stdout.trim().length > 0,
  };
}

export async function getGitDiffFingerprint(projectPath: string): Promise<string | null> {
  try {
    const result = await runGit(projectPath, ["status", "--porcelain=v1", "--untracked-files=all"]);
    return result.stdout.trim();
  } catch {
    return null;
  }
}

export async function stageAllChanges(projectPath: string): Promise<void> {
  await runGit(projectPath, ["add", "-A"]);
}

export async function commitStagedChanges(projectPath: string, message: string): Promise<void> {
  await runGit(projectPath, ["commit", "-m", message]);
}

export async function checkoutBranch(projectPath: string, branchName: string): Promise<void> {
  await runGit(projectPath, ["checkout", branchName]);
}

export async function mergeFastForward(projectPath: string, branchName: string): Promise<void> {
  await runGit(projectPath, ["merge", "--ff-only", branchName]);
}

export async function getBranchHead(projectPath: string, branchName: string): Promise<string> {
  const result = await runGit(projectPath, ["rev-parse", branchName]);
  return result.stdout.trim();
}

export async function getMergeBase(projectPath: string, leftBranch: string, rightBranch: string): Promise<string> {
  const result = await runGit(projectPath, ["merge-base", leftBranch, rightBranch]);
  return result.stdout.trim();
}

async function branchExists(projectPath: string, branchName: string): Promise<boolean> {
  try {
    await runGit(projectPath, ["rev-parse", "--verify", `refs/heads/${branchName}`]);
    return true;
  } catch {
    return false;
  }
}

async function runGit(projectPath: string, args: string[]): Promise<{ stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn("git", args, {
      cwd: projectPath,
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk: Buffer | string) => {
      stdout += String(chunk);
    });
    child.stderr.on("data", (chunk: Buffer | string) => {
      stderr += String(chunk);
    });
    child.on("error", reject);
    child.on("exit", (code) => {
      if (code === 0) {
        resolve({ stdout, stderr });
        return;
      }

      reject(new Error(stderr.trim() || `git ${args.join(" ")} failed with exit ${code ?? 1}`));
    });
  });
}