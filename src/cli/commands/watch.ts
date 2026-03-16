import type { Argv } from "yargs";
import { readJsonFile } from "../../core/fs.js";
import { daemonStatePath } from "../../core/paths.js";
import { createDefaultDaemonState } from "../../core/daemon-state.js";
import { listProjects } from "../../core/project-registry.js";
import { loadTaskLedger } from "../../core/task-ledger.js";
import type { DaemonState } from "../../core/types.js";

const REFRESH_MS = 1000;
const CLEAR = "\x1b[2J\x1b[H";
const BOLD = "\x1b[1m";
const DIM = "\x1b[2m";
const RESET = "\x1b[0m";
const GREEN = "\x1b[32m";
const YELLOW = "\x1b[33m";
const RED = "\x1b[31m";
const CYAN = "\x1b[36m";

function padRight(str: string, len: number): string {
  return str.length >= len ? str.slice(0, len) : str + " ".repeat(len - str.length);
}

function elapsed(from: string): string {
  const ms = Date.now() - new Date(from).getTime();
  if (ms < 60_000) return `${Math.round(ms / 1000)}s`;
  if (ms < 3_600_000) return `${Math.round(ms / 60_000)}m`;
  return `${Math.round(ms / 3_600_000)}h`;
}

async function renderDashboard(): Promise<string> {
  const state = await readJsonFile<DaemonState>(daemonStatePath(), createDefaultDaemonState()).catch(
    () => createDefaultDaemonState(),
  );
  const projects = await listProjects().catch(() => []);

  const lines: string[] = [];

  const daemonStatus = state.paused
    ? `${YELLOW}paused${RESET}`
    : `${GREEN}running (PID ${state.pid})${RESET}`;
  const uptime = state.startedAt ? `uptime: ${elapsed(state.startedAt)}` : "";
  const budget = `$${state.budgetSpentUsd.toFixed(4)} spent today`;

  lines.push(`${BOLD}OpenLoop Watch${RESET} ${DIM}(Ctrl+C to exit)${RESET}`);
  lines.push(`Daemon: ${daemonStatus}  ${DIM}${uptime}  ${budget}${RESET}`);
  lines.push("");

  // Header
  lines.push(
    `${BOLD}${padRight("PROJECT", 16)}${padRight("QUEUE", 8)}${padRight("ACTIVE TASK", 24)}${padRight("STATUS", 14)}LAST RUN${RESET}`,
  );
  lines.push("─".repeat(80));

  for (const project of projects) {
    const ledger = await loadTaskLedger(project.path).catch(() => ({ tasks: [] }));
    const tasks = ledger.tasks;
    const queueSize = tasks.filter(
      (t) => t.status === "ready" || t.status === "proposed" || t.status === "planned",
    ).length;

    const projectState = state.projects.find((p) => p.alias === project.alias);
    const isActive = state.currentRun?.projectAlias === project.alias;
    const currentRun = isActive ? state.currentRun : null;

    const activeTaskId = currentRun?.taskId ?? "--";
    const elapsedRun = currentRun ? `${elapsed(currentRun.startedAt)} elapsed` : "";
    const statusStr = isActive
      ? `${GREEN}running${RESET}`
      : state.paused
        ? `${YELLOW}paused${RESET}`
        : queueSize > 0
          ? `${CYAN}idle${RESET}`
          : `${DIM}idle${RESET}`;
    const lastRun = projectState?.lastResult ?? "--";

    lines.push(
      `${padRight(project.alias, 16)}${padRight(String(queueSize), 8)}${padRight(activeTaskId + (elapsedRun ? ` (${elapsedRun})` : ""), 24)}${padRight(statusStr, 22)}${DIM}${lastRun.slice(0, 24)}${RESET}`,
    );
  }

  if (projects.length === 0) {
    lines.push(`${DIM}No linked projects.${RESET}`);
  }

  lines.push("");
  lines.push(`${DIM}Refreshing every ${REFRESH_MS / 1000}s...${RESET}`);

  return lines.join("\n");
}

export function registerWatchCommand(cli: Argv): void {
  cli.command(
    "watch",
    "Live terminal dashboard (refreshes every second)",
    () => {},
    async () => {
      if (!process.stdout.isTTY) {
        console.error("Error: 'watch' requires a TTY. Use 'status --format json' for scripted use.");
        process.exitCode = 1;
        return;
      }

      process.stdout.write(CLEAR);

      const render = async () => {
        const content = await renderDashboard();
        process.stdout.write(CLEAR + content + "\n");
      };

      await render();
      const interval = setInterval(() => {
        render().catch(() => {});
      }, REFRESH_MS);

      process.on("SIGINT", () => {
        clearInterval(interval);
        process.stdout.write(RESET + "\n");
        process.exit(0);
      });

      // Keep process alive.
      await new Promise(() => {});
    },
  );
}
