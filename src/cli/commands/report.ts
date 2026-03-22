import type { Argv, ArgumentsCamelCase } from "yargs";
import { generateRunReport, formatRunReport } from "../../core/run-report.js";
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
