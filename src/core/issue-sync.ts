import https from "node:https";
import http from "node:http";
import path from "node:path";
import { readJsonFile, writeJsonFile } from "./fs.js";
import { addTask } from "./task-ledger.js";
import { loadProjectConfig, saveProjectConfig } from "./project-config.js";
import type { IssueSourceConfig, IssueSyncLedger, ProjectTask, SyncedIssue } from "./types.js";

const SYNC_LEDGER_FILE = "issue-sync.json";

function issueSyncPath(projectPath: string): string {
  return path.join(projectPath, ".openloop", SYNC_LEDGER_FILE);
}

const EMPTY_SYNC_LEDGER: IssueSyncLedger = {
  version: 1,
  source: { provider: "github", repo: "", label: "openloop" },
  issues: [],
  updatedAt: new Date(0).toISOString(),
};

export async function loadIssueSyncLedger(projectPath: string): Promise<IssueSyncLedger> {
  return readJsonFile<IssueSyncLedger>(issueSyncPath(projectPath), EMPTY_SYNC_LEDGER);
}

export async function saveIssueSyncLedger(projectPath: string, ledger: IssueSyncLedger): Promise<void> {
  ledger.updatedAt = new Date().toISOString();
  await writeJsonFile(issueSyncPath(projectPath), ledger);
}

interface RemoteIssue {
  number: number;
  title: string;
  body: string;
  labels: string[];
  state: "open" | "closed";
  url: string;
}

function httpGet(url: string, headers: Record<string, string>): Promise<{ statusCode: number; body: string }> {
  return new Promise((resolve, reject) => {
    const mod = url.startsWith("https") ? https : http;
    const req = mod.get(url, { headers }, (res) => {
      const chunks: Buffer[] = [];
      res.on("data", (chunk: Buffer) => chunks.push(chunk));
      res.on("end", () => {
        resolve({
          statusCode: res.statusCode ?? 0,
          body: Buffer.concat(chunks).toString("utf8"),
        });
      });
    });
    req.on("error", reject);
    req.setTimeout(30_000, () => {
      req.destroy(new Error("Request timed out"));
    });
  });
}

function httpPost(url: string, headers: Record<string, string>, body: string): Promise<{ statusCode: number; body: string }> {
  return new Promise((resolve, reject) => {
    const parsed = new URL(url);
    const mod = parsed.protocol === "https:" ? https : http;
    const req = mod.request(
      {
        hostname: parsed.hostname,
        port: parsed.port || (parsed.protocol === "https:" ? 443 : 80),
        path: parsed.pathname + parsed.search,
        method: "POST",
        headers: { ...headers, "Content-Type": "application/json", "Content-Length": Buffer.byteLength(body) },
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on("data", (chunk: Buffer) => chunks.push(chunk));
        res.on("end", () => {
          resolve({
            statusCode: res.statusCode ?? 0,
            body: Buffer.concat(chunks).toString("utf8"),
          });
        });
      },
    );
    req.on("error", reject);
    req.setTimeout(30_000, () => {
      req.destroy(new Error("Request timed out"));
    });
    req.write(body);
    req.end();
  });
}

export async function fetchGitHubIssues(repo: string, label: string, token?: string | null): Promise<RemoteIssue[]> {
  const encodedLabel = encodeURIComponent(label);
  const url = `https://api.github.com/repos/${repo}/issues?labels=${encodedLabel}&state=open&per_page=100`;
  const headers: Record<string, string> = {
    "User-Agent": "openloop-cli",
    Accept: "application/vnd.github+json",
  };
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const res = await httpGet(url, headers);
  if (res.statusCode !== 200) {
    throw new Error(`GitHub API returned ${res.statusCode}: ${res.body.slice(0, 200)}`);
  }

  const data = JSON.parse(res.body) as Array<{
    number: number;
    title: string;
    body: string | null;
    labels: Array<{ name: string }>;
    state: string;
    html_url: string;
    pull_request?: unknown;
  }>;

  return data
    .filter((issue) => !issue.pull_request)
    .map((issue) => ({
      number: issue.number,
      title: issue.title,
      body: issue.body ?? "",
      labels: issue.labels.map((l) => l.name),
      state: issue.state === "open" ? ("open" as const) : ("closed" as const),
      url: issue.html_url,
    }));
}

export async function fetchGitLabIssues(repo: string, label: string, token?: string | null): Promise<RemoteIssue[]> {
  const encodedProject = encodeURIComponent(repo);
  const encodedLabel = encodeURIComponent(label);
  const url = `https://gitlab.com/api/v4/projects/${encodedProject}/issues?labels=${encodedLabel}&state=opened&per_page=100`;
  const headers: Record<string, string> = {
    "User-Agent": "openloop-cli",
  };
  if (token) {
    headers["PRIVATE-TOKEN"] = token;
  }

  const res = await httpGet(url, headers);
  if (res.statusCode !== 200) {
    throw new Error(`GitLab API returned ${res.statusCode}: ${res.body.slice(0, 200)}`);
  }

  const data = JSON.parse(res.body) as Array<{
    iid: number;
    title: string;
    description: string | null;
    labels: string[];
    state: string;
    web_url: string;
  }>;

  return data.map((issue) => ({
    number: issue.iid,
    title: issue.title,
    body: issue.description ?? "",
    labels: issue.labels,
    state: issue.state === "opened" ? ("open" as const) : ("closed" as const),
    url: issue.web_url,
  }));
}

export async function postGitHubComment(repo: string, issueNumber: number, body: string, token: string): Promise<void> {
  const url = `https://api.github.com/repos/${repo}/issues/${issueNumber}/comments`;
  const headers: Record<string, string> = {
    "User-Agent": "openloop-cli",
    Accept: "application/vnd.github+json",
    Authorization: `Bearer ${token}`,
  };
  const res = await httpPost(url, headers, JSON.stringify({ body }));
  if (res.statusCode !== 201) {
    throw new Error(`GitHub comment failed (${res.statusCode}): ${res.body.slice(0, 200)}`);
  }
}

export async function postGitLabComment(repo: string, issueNumber: number, body: string, token: string): Promise<void> {
  const encodedProject = encodeURIComponent(repo);
  const url = `https://gitlab.com/api/v4/projects/${encodedProject}/issues/${issueNumber}/notes`;
  const headers: Record<string, string> = {
    "User-Agent": "openloop-cli",
    "PRIVATE-TOKEN": token,
  };
  const res = await httpPost(url, headers, JSON.stringify({ body }));
  if (res.statusCode !== 201) {
    throw new Error(`GitLab comment failed (${res.statusCode}): ${res.body.slice(0, 200)}`);
  }
}

function issueToTaskId(issue: RemoteIssue): string {
  const slug = issue.title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "")
    .slice(0, 50);
  return slug ? `issue-${issue.number}-${slug}` : `issue-${issue.number}`;
}

function guessIssueKind(issue: RemoteIssue): ProjectTask["kind"] {
  const lower = (issue.title + " " + issue.body).toLowerCase();
  if (lower.includes("bug") || lower.includes("fix")) return "bugfix";
  if (lower.includes("test")) return "test";
  if (lower.includes("refactor")) return "refactor";
  if (lower.includes("docs") || lower.includes("documentation")) return "docs";
  if (lower.includes("lint")) return "lint-fix";
  if (lower.includes("type")) return "type-fix";
  return "feature";
}

export interface SyncResult {
  imported: number;
  skipped: number;
  total: number;
  errors: string[];
}

export async function syncIssues(
  projectPath: string,
  source: IssueSourceConfig,
): Promise<SyncResult> {
  const fetcher = source.provider === "github" ? fetchGitHubIssues : fetchGitLabIssues;
  const remoteIssues = await fetcher(source.repo, source.label, source.token);

  const syncLedger = await loadIssueSyncLedger(projectPath);
  syncLedger.source = source;

  const result: SyncResult = { imported: 0, skipped: 0, total: remoteIssues.length, errors: [] };

  for (const issue of remoteIssues) {
    const existing = syncLedger.issues.find((synced) => synced.number === issue.number);
    if (existing) {
      result.skipped++;
      continue;
    }

    const taskId = issueToTaskId(issue);
    const now = new Date().toISOString();
    const providerPrefix = source.provider === "github" ? "github" : "gitlab";

    const task: ProjectTask = {
      id: taskId,
      title: issue.title,
      kind: guessIssueKind(issue),
      status: "proposed",
      risk: "medium-risk",
      scope: null,
      source: { type: "issue", ref: `${providerPrefix}#${issue.number}` },
      specId: null,
      branch: null,
      owner: "openloop",
      acceptanceCriteria: issue.body
        ? [`From issue: ${issue.url}`, issue.body.slice(0, 500)]
        : [`From issue: ${issue.url}`],
      attempts: 0,
      lastFailureSignature: null,
      promotion: "pull-request",
      notes: [`Synced from ${providerPrefix} issue #${issue.number}`],
      createdAt: now,
      updatedAt: now,
    };

    try {
      await addTask(projectPath, task);
      const syncedIssue: SyncedIssue = {
        number: issue.number,
        title: issue.title,
        body: issue.body.slice(0, 1000),
        labels: issue.labels,
        state: issue.state,
        url: issue.url,
        taskId,
        syncedAt: now,
      };
      syncLedger.issues.push(syncedIssue);
      result.imported++;
    } catch (err) {
      result.errors.push(`Issue #${issue.number}: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  await saveIssueSyncLedger(projectPath, syncLedger);

  // Update last synced timestamp on project config
  const projectConfig = await loadProjectConfig(projectPath);
  if (projectConfig.issueSource) {
    projectConfig.issueSource.lastSyncedAt = new Date().toISOString();
    await saveProjectConfig(projectPath, projectConfig);
  }

  return result;
}

export async function postTaskStatusToIssue(
  projectPath: string,
  taskId: string,
  status: string,
  details?: string,
): Promise<void> {
  const syncLedger = await loadIssueSyncLedger(projectPath);
  const syncedIssue = syncLedger.issues.find((i) => i.taskId === taskId);
  if (!syncedIssue) return;

  const source = syncLedger.source;
  if (!source.token || !source.postStatusComments) return;

  const statusEmoji: Record<string, string> = {
    done: "✅",
    promoted: "🚀",
    failed: "❌",
    blocked: "⚠️",
    "in_progress": "🔄",
    ready: "📋",
    planned: "📝",
  };

  const emoji = statusEmoji[status] ?? "ℹ️";
  const body = [
    `${emoji} **OpenLoop Update** — Task status: \`${status}\``,
    details ? `\n${details}` : "",
    `\n_Updated by [OpenLoop](https://github.com/openloop) daemon_`,
  ].join("");

  if (source.provider === "github") {
    await postGitHubComment(source.repo, syncedIssue.number, body, source.token);
  } else {
    await postGitLabComment(source.repo, syncedIssue.number, body, source.token);
  }
}

/**
 * Post a PR link comment to the originating issue when a task creates a PR.
 */
export async function postPrLinkToIssue(
  projectPath: string,
  taskId: string,
  prUrl: string,
  branch?: string | null,
): Promise<void> {
  const syncLedger = await loadIssueSyncLedger(projectPath);
  const syncedIssue = syncLedger.issues.find((i) => i.taskId === taskId);
  if (!syncedIssue) return;

  const source = syncLedger.source;
  if (!source.token || !source.postStatusComments) return;

  const body = [
    `🔗 **OpenLoop PR** — A pull request was created for this issue`,
    branch ? `\nBranch: \`${branch}\`` : "",
    `\nPR: ${prUrl}`,
    `\n_Created by [OpenLoop](https://github.com/openloop) daemon_`,
  ].join("");

  if (source.provider === "github") {
    await postGitHubComment(source.repo, syncedIssue.number, body, source.token);
  } else {
    await postGitLabComment(source.repo, syncedIssue.number, body, source.token);
  }
}

export async function listSyncedIssues(projectPath: string): Promise<SyncedIssue[]> {
  const ledger = await loadIssueSyncLedger(projectPath);
  return ledger.issues;
}
