import path from "node:path";
import { getDiffPatch } from "./git.js";
import { ensureDir, fileExists, readJsonFile } from "./fs.js";
import { matchesAnyGlob, normalizeScopePath } from "./project-policy.js";
import type { ProjectConfig, ProjectPolicy, ProjectTask, ReviewFinding, ReviewResult } from "./types.js";

// ---------------------------------------------------------------------------
// Deterministic checks — the spine of orthogonality. Pure code, no agent.
// ---------------------------------------------------------------------------

/**
 * Check whether the agent's actual changes respect the task's declared scope
 * and the project's deny/high-risk globs. These catch what validation structurally
 * cannot: the agent touching files it was not supposed to touch.
 */
export function checkScopeDrift(
  changedFiles: string[],
  task: ProjectTask,
  policy: ProjectPolicy,
): ReviewFinding[] {
  const findings: ReviewFinding[] = [];
  const scopePaths = (task.scope?.paths ?? []).map(normalizeScopePath);

  for (const file of changedFiles) {
    const normalized = normalizeScopePath(file);

    if (matchesAnyGlob(normalized, policy.scope.denyGlobs)) {
      findings.push({
        rule: "deny-glob-violation",
        severity: "block",
        message: `Changed file '${file}' matches a denied glob in project policy.`,
        file,
      });
    }

    if (matchesAnyGlob(normalized, policy.scope.highRiskAreas)) {
      findings.push({
        rule: "high-risk-area-touched",
        severity: "warn",
        message: `Changed file '${file}' is in a declared high-risk area.`,
        file,
      });
    }

    if (scopePaths.length > 0 && !matchesAnyGlob(normalized, scopePaths)) {
      findings.push({
        rule: "scope-drift",
        severity: "warn",
        message: `Changed file '${file}' is outside the task's declared scope paths.`,
        file,
      });
    }
  }

  return findings;
}

// High-confidence secret patterns — conservative to avoid false-positive blocks.
const SECRET_PATTERNS: readonly { name: string; pattern: RegExp }[] = [
  { name: "aws-access-key-id", pattern: /AKIA[0-9A-Z]{16}/g },
  { name: "github-token", pattern: /gh[ps]_[A-Za-z0-9]{36,}/g },
  { name: "private-key-block", pattern: /-----BEGIN (?:RSA |EC |OPENSSH |DSA |)PRIVATE KEY-----/g },
  { name: "generic-secret-assignment", pattern: /(?:api[_-]?key|secret|token|password|passwd)\s*[:=]\s*["'][A-Za-z0-9+/=_-]{32,}["']/gi },
];

/**
 * Scan added diff lines for common hardcoded-secret patterns. Only inspects
 * additions (lines starting with +), not context or deletions.
 */
export function checkSecrets(diffPatch: string): ReviewFinding[] {
  const findings: ReviewFinding[] = [];
  const addedContent = diffPatch
    .split("\n")
    .filter((line) => line.startsWith("+") && !line.startsWith("+++"))
    .join("\n");

  for (const { name, pattern } of SECRET_PATTERNS) {
    pattern.lastIndex = 0;
    const matches = addedContent.match(pattern);
    if (matches && matches.length > 0) {
      findings.push({
        rule: `secret-detection:${name}`,
        severity: "block",
        message: `Possible hardcoded secret detected in diff (${name}): ${matches.length} occurrence(s).`,
      });
    }
  }

  return findings;
}

/**
 * Extract changed file paths from a git diff patch by parsing `diff --git` headers.
 */
export function extractChangedFiles(patch: string): string[] {
  const files = new Set<string>();
  for (const line of patch.split("\n")) {
    const match = /^diff --git a\/(.+) b\/(.+)$/.exec(line);
    if (match && match[2]) {
      files.add(match[2]);
    }
  }
  return [...files];
}

// ---------------------------------------------------------------------------
// LLM review — the bonus layer. Agent inspects the diff and writes structured
// findings to a JSON file. Fail-open: if the agent doesn't write a valid file,
// only deterministic findings apply.
// ---------------------------------------------------------------------------

export function buildReviewPrompt(task: ProjectTask, diffPatch: string): string {
  const maxDiffChars = 20_000;
  const truncatedDiff = diffPatch.length > maxDiffChars
    ? `${diffPatch.slice(0, maxDiffChars)}\n\n... (diff truncated, ${diffPatch.length - maxDiffChars} chars omitted)`
    : diffPatch;

  return [
    "You are reviewing code changes made by another AI agent for safety and correctness.",
    "Do NOT modify any code files. Only review and report findings.",
    "",
    `Task: ${task.title} (${task.id})`,
    `Kind: ${task.kind}`,
    `Risk: ${task.risk}`,
    "",
    "## Changed Code (git diff HEAD)",
    "```diff",
    truncatedDiff,
    "```",
    "",
    "## Review Checklist",
    "- Security vulnerabilities: injection, path traversal, hardcoded secrets, unsafe deserialization",
    "- Logic errors or incorrect implementations that pass tests but are wrong",
    "- Breaking changes to public APIs or interfaces",
    "- Missing error handling for new code paths",
    "",
    `## Output`,
    `Write your review as JSON to \`.openloop/reviews/${task.id}.json\` with exactly this schema:`,
    "```json",
    '{',
    '  "verdict": "approve" | "request-changes",',
    '  "findings": [',
    '    { "severity": "block" | "warn" | "info", "message": "concise description" }',
    '  ]',
    "}",
    "```",
    "",
    'If the changes look safe and correct, write verdict "approve" with an empty findings array.',
    'If you find issues, set verdict "request-changes" and list each finding with appropriate severity.',
    '"block" = must not auto-merge, "warn" = should review, "info" = FYI.',
  ].join("\n");
}

interface AgentReviewFile {
  verdict?: unknown;
  findings?: unknown;
}

/**
 * Read and parse the LLM reviewer's output file.
 * Returns null if the file is missing or malformed (fail-open for the bonus layer).
 */
async function readAgentReview(projectPath: string, taskId: string): Promise<ReviewFinding[] | null> {
  const reviewPath = path.join(projectPath, ".openloop", "reviews", `${taskId}.json`);
  if (!(await fileExists(reviewPath))) {
    return null;
  }

  try {
    const data = await readJsonFile<AgentReviewFile>(reviewPath, { verdict: "approve" });
    if (data.verdict !== "request-changes" || !Array.isArray(data.findings)) {
      return [];
    }

    const findings: ReviewFinding[] = [];
    for (const entry of data.findings as Record<string, unknown>[]) {
      const message = typeof entry.message === "string" ? entry.message : null;
      if (!message) continue;
      const rawSeverity = typeof entry.severity === "string" ? entry.severity : "info";
      const severity = rawSeverity === "block" ? "block" : rawSeverity === "warn" ? "warn" : "info";
      findings.push({ rule: "agent-review", severity, message });
    }
    return findings;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

export interface RunReviewOptions {
  projectPath: string;
  task: ProjectTask;
  projectConfig: ProjectConfig;
  projectPolicy: ProjectPolicy;
  /** Optional agent runner for LLM review. If absent, only deterministic checks run. */
  reviewerRunner?: (prompt: string) => Promise<number>;
}

/**
 * Run the post-implementation review. Deterministic checks (scope drift, secret
 * detection) always execute. The LLM review runs only when a reviewerRunner is
 * provided. Never throws — review failures degrade gracefully to deterministic-only.
 */
export async function runReview(options: RunReviewOptions): Promise<ReviewResult> {
  const { projectPath, task, projectPolicy } = options;
  const findings: ReviewFinding[] = [];

  const diffPatch = await getDiffPatch(projectPath).catch(() => null);
  const changedFiles = diffPatch ? extractChangedFiles(diffPatch) : [];

  findings.push(...checkScopeDrift(changedFiles, task, projectPolicy));
  if (diffPatch) {
    findings.push(...checkSecrets(diffPatch));
  }

  if (options.reviewerRunner && diffPatch) {
    await ensureDir(path.join(projectPath, ".openloop", "reviews")).catch(() => {});
    const prompt = buildReviewPrompt(task, diffPatch);
    try {
      await options.reviewerRunner(prompt);
    } catch {
      // Agent review failure is non-fatal — deterministic checks are the spine.
    }
    const agentFindings = await readAgentReview(projectPath, task.id);
    if (agentFindings) {
      findings.push(...agentFindings);
    }
  }

  return {
    findings,
    hasBlocking: findings.some((f) => f.severity === "block"),
  };
}
