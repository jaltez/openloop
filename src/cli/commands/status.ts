import type { Argv } from "yargs";
import { listProjects } from "../../core/project-registry.js";
import { loadTaskLedger } from "../../core/task-ledger.js";
import { readJsonFile } from "../../core/fs.js";
import { daemonStatePath } from "../../core/paths.js";
import { createDefaultDaemonState } from "../../core/daemon-state.js";
import type { DaemonState } from "../../core/types.js";
import { resolveOutputFormat, printTable } from "../../core/table.js";

export function registerStatusCommand(cli: Argv): void {
  cli.command(
    "status",
    "Show multi-project dashboard",
    (cmd: Argv) =>
      cmd.option("format", {
        type: "string",
        choices: ["table", "json"] as const,
        describe: "Output format (default: table for TTY, json for pipes)",
      }),
    async (args) => {
      const projects = await listProjects();
      const daemonState = await readJsonFile<DaemonState>(daemonStatePath(), createDefaultDaemonState()).catch(
        () => createDefaultDaemonState(),
      );

      const rows = await Promise.all(
        projects.map(async (project) => {
          const ledger = await loadTaskLedger(project.path).catch(() => ({ tasks: [] }));
          const tasks = ledger.tasks;
          const ready = tasks.filter((t) => t.status === "ready").length;
          const inProgress = tasks.filter((t) => t.status === "in_progress").length;
          const blocked = tasks.filter((t) => t.status === "blocked" || t.status === "failed").length;
          const done = tasks.filter((t) => t.status === "done" || t.status === "promoted").length;
          const total = tasks.length;

          const projectState = daemonState.projects.find((p) => p.alias === project.alias);
          const lastRun = projectState?.lastResult ?? null;
          const paused = projectState?.paused ?? daemonState.paused;

          return {
            alias: project.alias,
            initialized: String(project.initialized),
            paused: String(paused),
            total: String(total),
            ready: String(ready),
            in_progress: String(inProgress),
            blocked: String(blocked),
            done: String(done),
            lastRun: lastRun ?? "--",
          };
        }),
      );

      const fmt = resolveOutputFormat(args.format as string | undefined);

      if (fmt === "table") {
        if (rows.length === 0) {
          console.log("No linked projects. Use 'openloop project add' to register one.");
          return;
        }
        const daemonStatus = daemonState.paused
          ? "paused"
          : `running (PID ${daemonState.pid})`;
        console.log(`Daemon: ${daemonStatus}`);
        console.log(`Budget: $${daemonState.budgetSpentUsd.toFixed(4)} / $${daemonState.budgetSpentUsd.toFixed(4)} today`);
        console.log("");
        printTable(rows);
      } else {
        console.log(
          JSON.stringify(
            {
              daemon: {
                pid: daemonState.pid,
                paused: daemonState.paused,
                budgetSpentUsd: daemonState.budgetSpentUsd,
                activeProject: daemonState.activeProject,
              },
              projects: rows,
            },
            null,
            2,
          ),
        );
      }
    },
  );
}
