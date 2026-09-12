import type { Argv, ArgumentsCamelCase } from "yargs";
import { spawn } from "node:child_process";
import { getProject } from "../../core/project-registry.js";
import { applyPromotionArtifact, dryRunPromotionApply, getPromotionDetail, getPromotionHistory, listPromotionArtifacts, listPromotionArtifactsForTask, updatePromotionArtifact } from "../../core/promotion-queue.js";
import { listPromotionResultArtifacts, writePromotionResultArtifact } from "../../core/promotion-artifacts.js";
import { withTaskLedger } from "../../core/task-ledger.js";
import { runLifecycleHooks } from "../../core/hooks.js";
import { loadGlobalConfig } from "../../core/global-config.js";
import { daemonLogPath } from "../../core/paths.js";
import { resolveOutputFormat, printTable } from "../../core/table.js";
import type { PromotionCheckRollupEntry, PromotionResultArtifact } from "../../core/types.js";

type PromotionListArgs = ArgumentsCamelCase<{
  project: string;
  status?: "pending" | "applied" | "rejected";
  task?: string;
  format?: string;
}>;

type PromotionUpdateArgs = ArgumentsCamelCase<{
  project: string;
  task: string;
  note?: string;
  dryRun?: boolean;
}>;

type PromotionShowArgs = ArgumentsCamelCase<{
  project: string;
  task: string;
}>;

type PromotionHistoryArgs = ArgumentsCamelCase<{
  project: string;
  task: string;
}>;

export function registerPromotionCommands(cli: Argv): void {
  cli.command(
    "promotion <command>",
    "Inspect or resolve promotion artifacts",
    (promotionCli: Argv) =>
      promotionCli
        .command(
          "list",
          "List promotion artifacts for a linked project",
          (command: Argv) =>
            command
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("task", { type: "string" })
              .option("status", { choices: ["pending", "applied", "rejected"] as const })
              .option("format", { type: "string", choices: ["table", "json"] as const }),
          async (args: PromotionListArgs) => {
            const project = await getProject(String(args.project));
            const artifacts = args.task
              ? await listPromotionArtifactsForTask(project.path, String(args.task))
              : await listPromotionArtifacts(project.path);
            const filtered = args.status ? artifacts.filter((item) => item.artifact.status === args.status) : artifacts;
            const fmt = resolveOutputFormat(args.format);
            if (fmt === "table") {
              printTable(filtered.map((item) => ({
                taskId: item.artifact.taskId,
                decision: item.artifact.decision,
                action: item.artifact.action,
                status: item.artifact.status,
              })));
            } else {
              console.log(
                JSON.stringify(
                  filtered.map((item) => ({
                    taskId: item.artifact.taskId,
                    decision: item.artifact.decision,
                    action: item.artifact.action,
                    status: item.artifact.status,
                    path: item.artifactPath,
                  })),
                  null,
                  2,
                ),
              );
            }
          },
        )
        .command(
          "show",
          "Show promotion artifact details for a task",
          (command: Argv) => command.option("project", { type: "string", alias: "p", demandOption: true }).option("task", { type: "string", demandOption: true }),
          async (args: PromotionShowArgs) => {
            const project = await getProject(String(args.project));
            const detail = await getPromotionDetail(project.path, String(args.task));
            console.log(JSON.stringify(detail, null, 2));
          },
        )
        .command(
          "history",
          "Show promotion artifact history for a task",
          (command: Argv) => command.option("project", { type: "string", alias: "p", demandOption: true }).option("task", { type: "string", demandOption: true }),
          async (args: PromotionHistoryArgs) => {
            const project = await getProject(String(args.project));
            const history = await getPromotionHistory(project.path, String(args.task));
            console.log(JSON.stringify(history, null, 2));
          },
        )
        .command(
          "apply",
          "Apply a pending promotion locally when supported",
          (command: Argv) =>
            command
              .option("project", { type: "string", alias: "p", demandOption: true })
              .option("task", { type: "string", demandOption: true })
              .option("note", { type: "string" })
              .option("dry-run", { type: "boolean", default: false, describe: "Preview what would happen without executing" }),
          async (args: PromotionUpdateArgs) => {
            const project = await getProject(String(args.project));
            if (args.dryRun) {
              const preview = await dryRunPromotionApply(project.path, String(args.task));
              console.log(JSON.stringify(preview, null, 2));
              return;
            }
            const item = await applyPromotionArtifact(project.path, String(args.task), args.note ? String(args.note) : undefined);
            console.log(JSON.stringify({ taskId: item.artifact.taskId, status: item.artifact.status, path: item.artifactPath, note: item.artifact.note }, null, 2));
          },
        )
        .command(
          "reject",
          "Mark a pending promotion as rejected",
          (command: Argv) => command.option("project", { type: "string", alias: "p", demandOption: true }).option("task", { type: "string", demandOption: true }).option("note", { type: "string" }),
          async (args: PromotionUpdateArgs) => {
            const project = await getProject(String(args.project));
            const item = await updatePromotionArtifact(project.path, String(args.task), "rejected", args.note ? String(args.note) : undefined);
            console.log(JSON.stringify({ taskId: item.artifact.taskId, status: item.artifact.status, path: item.artifactPath }, null, 2));
          },
        )
        .command(
          "refresh",
          "Refresh promotion status from the PR (requires gh; no daemon polling in this release)",
          (command: Argv) => command.option("project", { type: "string", alias: "p", demandOption: true }).option("task", { type: "string", demandOption: true }),
          async (args: PromotionRefreshArgs) => {
            await refreshPromotion(args);
          },
        )
        .demandCommand(),
  );
}

type PromotionRefreshArgs = ArgumentsCamelCase<{
  project: string;
  task: string;
}>;

function runGh(args: string[]): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const { promise, resolve } = Promise.withResolvers<{ stdout: string; stderr: string; exitCode: number }>();
  const child = spawn("gh", args, { stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout?.on("data", (chunk: Buffer | string) => {
    stdout += String(chunk);
  });
  child.stderr?.on("data", (chunk: Buffer | string) => {
    stderr += String(chunk);
  });
  child.on("error", (error) => resolve({ stdout, stderr: error.message, exitCode: 127 }));
  child.on("exit", (code) => resolve({ stdout, stderr, exitCode: code ?? 1 }));
  return promise;
}

// `promotion refresh`: pull live PR state via `gh pr view` and feed it back
// into the ledger. Daemon-side polling is intentionally out of scope for this
// release — refresh is invoked on demand.
export async function refreshPromotion(args: PromotionRefreshArgs): Promise<void> {
  const project = await getProject(String(args.project));
  const taskId = String(args.task);
  const results = await listPromotionResultArtifacts(project.path, taskId);
  const withPr = results.find((item) => item.artifact.prUrl);
  if (!withPr) {
    console.error(`No PR promotion result found for task ${taskId} in ${project.alias}.`);
    process.exitCode = 1;
    return;
  }
  const prUrl = withPr.artifact.prUrl!;

  const gh = await runGh(["pr", "view", prUrl, "--json", "state,statusCheckRollup"]);
  if (gh.exitCode !== 0) {
    console.error(gh.stderr.trim() || `gh pr view failed with exit ${gh.exitCode}`);
    process.exitCode = 1;
    return;
  }

  let state: string | null = null;
  let checks: PromotionCheckRollupEntry[] = [];
  try {
    const view = JSON.parse(gh.stdout) as { state?: unknown; statusCheckRollup?: unknown };
    state = typeof view.state === "string" ? view.state : null;
    if (Array.isArray(view.statusCheckRollup)) {
      checks = (view.statusCheckRollup as Array<Record<string, unknown>>).map((entry) => {
        // gh mixes CheckRun entries (name + conclusion) with legacy
        // StatusContext entries (context + state). Normalize both into one
        // shape: a StatusContext's state acts as the conclusion.
        const name = typeof entry.name === "string" ? entry.name
          : typeof entry.context === "string" ? entry.context
            : null;
        return {
          name,
          status: typeof entry.status === "string" ? entry.status : null,
          conclusion: typeof entry.conclusion === "string" ? entry.conclusion
            : typeof entry.state === "string" ? entry.state
              : null,
        };
      });
    }
  } catch {
    console.error(`gh pr view returned unparseable output: ${gh.stdout.slice(0, 200)}`);
    process.exitCode = 1;
    return;
  }

  const refreshed: PromotionResultArtifact = {
    ...withPr.artifact,
    createdAt: new Date().toISOString(),
    result: "refreshed",
    note: `Refreshed from ${prUrl}: state=${state ?? "unknown"}, checks=${checks.length}`,
    state,
    checks,
  };
  await writePromotionResultArtifact(project.path, refreshed);

  const FAILING_CONCLUSIONS = new Set(["FAILURE", "ERROR", "TIMED_OUT", "ACTION_REQUIRED"]);
  const failedChecks = checks
    .filter((check) => check.conclusion !== null && FAILING_CONCLUSIONS.has(check.conclusion))
    .map((check) => check.name ?? "unknown-check");

  if (state === "MERGED") {
    await withTaskLedger(project.path, (ledger) => {
      const task = ledger.tasks.find((candidate) => candidate.id === taskId);
      if (task) {
        task.status = "promoted";
        task.promotedAt = new Date().toISOString();
        task.notes = [...(task.notes ?? []), `PR merged: ${prUrl}`];
        task.updatedAt = new Date().toISOString();
      }
    });
  } else if (state === "CLOSED") {
    try {
      await updatePromotionArtifact(project.path, taskId, "rejected", `PR closed: ${prUrl}`);
    } catch {
      await withTaskLedger(project.path, (ledger) => {
        const task = ledger.tasks.find((candidate) => candidate.id === taskId);
        if (task) {
          task.notes = [...(task.notes ?? []), `PR closed without merging: ${prUrl}`];
          task.updatedAt = new Date().toISOString();
        }
      });
    }
  }

  if (failedChecks.length > 0) {
    const note = `PR checks failed: ${failedChecks.join(", ")}`;
    await withTaskLedger(project.path, (ledger) => {
      const task = ledger.tasks.find((candidate) => candidate.id === taskId);
      if (task) {
        task.notes = [...(task.notes ?? []), note];
        task.updatedAt = new Date().toISOString();
      }
    });
    const config = await loadGlobalConfig();
    await runLifecycleHooks({
      globalConfig: config,
      payload: {
        event: "promotion-ci-failed",
        project: project.alias,
        taskId,
        message: note,
        timestamp: new Date().toISOString(),
        mode: "idle",
        failedChecks,
      },
      daemonLogPath: daemonLogPath(),
    }).catch(() => {});
  }

  console.log(JSON.stringify({ taskId, prUrl, state, failedChecks }, null, 2));
}