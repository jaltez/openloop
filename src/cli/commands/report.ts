import type { Argv, ArgumentsCamelCase } from "yargs";
import { generateRunReport, formatRunReport } from "../../core/run-report.js";
import { buildDigest } from "../../core/digest.js";
import { resolveOutputFormat } from "../../core/table.js";

type ReportArgs = ArgumentsCamelCase<{
  hours?: number;
  format?: string;
}>;

export function registerReportCommand(cli: Argv): void {
  cli.command(
    "report",
    "Generate a human-readable summary of recent daemon activity",
    (cmd: Argv) =>
      cmd
        .option("hours", {
          type: "number",
          default: 24,
          describe: "Report period in hours (default: 24)",
        })
        .option("format", {
          type: "string",
          choices: ["table", "json"] as const,
          describe: "Output format (default: table for TTY, json for pipes)",
        }),
    async (args: ReportArgs) => {
      const hours = args.hours ?? 24;
      const sinceMs = hours * 60 * 60 * 1000;
      const report = await generateRunReport({ sinceMs });
      const fmt = resolveOutputFormat(args.format);

      if (fmt === "json") {
        console.log(JSON.stringify(report, null, 2));
      } else {
        console.log(formatRunReport(report));
      }
    },
  );
}

export function registerDigestCommand(cli: Argv): void {
  cli.command(
    "digest",
    "Fleet confidence digest: activity, spend split, and pending review queue",
    (cmd: Argv) =>
      cmd
        .option("since-hours", {
          type: "number",
          default: 24,
          describe: "Digest window in hours (default: 24)",
        })
        .option("format", {
          type: "string",
          choices: ["table", "json"] as const,
          describe: "Output format (default: table for TTY, json for pipes)",
        }),
    async (args: ArgumentsCamelCase<{ sinceHours?: number; format?: string }>) => {
      const sinceMs = (args.sinceHours ?? 24) * 60 * 60 * 1000;
      const digest = await buildDigest({ sinceMs });
      const fmt = resolveOutputFormat(args.format);

      if (fmt === "json") {
        console.log(JSON.stringify(digest, null, 2));
        return;
      }

      for (const project of digest.projects) {
        console.log(`${project.alias}`);
        console.log(`  completed=${project.completed} failed=${project.failed} blocked=${project.blocked} promoted=${project.promoted}`);
        console.log(`  spend: measured=$${project.spendUsd.measured} estimated=$${project.spendUsd.estimated}`);
        if (project.reviewQueue.length === 0) {
          console.log("  review queue: empty");
          continue;
        }
        console.log("  review queue:");
        for (const entry of project.reviewQueue) {
          console.log(`    [${entry.confidence}] ${entry.taskId} (${entry.risk}) ${entry.title.slice(0, 60)}`);
        }
      }
    },
  );
}
