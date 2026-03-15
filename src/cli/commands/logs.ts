import fs from "node:fs/promises";
import path from "node:path";
import type { Argv, ArgumentsCamelCase } from "yargs";
import { fileExists } from "../../core/fs.js";
import { daemonLogPath } from "../../core/paths.js";
import { getProject } from "../../core/project-registry.js";

type LogsArgs = ArgumentsCamelCase<{
  project?: string;
  lines: number;
}>;

export function registerLogsCommands(cli: Argv): void {
  cli.command(
    "logs",
    "View daemon or project run logs",
    (command: Argv) =>
      command
        .option("project", { type: "string", describe: "Show run summaries for a specific project" })
        .option("lines", { type: "number", default: 50, describe: "Number of lines to show" }),
    async (args: LogsArgs) => {
      if (args.project) {
        const project = await getProject(String(args.project));
        const runsDir = path.join(project.path, ".openloop", "runs");
        if (!(await fileExists(runsDir))) {
          console.log("No run summaries found.");
          return;
        }
        const entries = await fs.readdir(runsDir);
        const mdFiles = entries.filter((name) => name.endsWith(".md")).sort().reverse().slice(0, args.lines);
        for (const file of mdFiles) {
          const content = await fs.readFile(path.join(runsDir, file), "utf8");
          console.log(`--- ${file} ---`);
          console.log(content);
        }
        return;
      }

      const logPath = daemonLogPath();
      if (!(await fileExists(logPath))) {
        console.log("No daemon log found.");
        return;
      }
      const content = await fs.readFile(logPath, "utf8");
      const lines = content.split("\n");
      const tail = lines.slice(-args.lines).join("\n");
      console.log(tail);
    },
  );
}
