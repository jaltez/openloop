import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { hideBin } from "yargs/helpers";
import yargs from "yargs/yargs";
import { registerDaemonCommands } from "./cli/commands/service.js";
import { registerConfigCommands } from "./cli/commands/config.js";
import { registerLogsCommands } from "./cli/commands/logs.js";
import { registerProjectCommands } from "./cli/commands/project.js";
import { registerPromotionCommands } from "./cli/commands/promotion.js";
import { registerRunCommands } from "./cli/commands/run.js";
import { registerRuntimeCommands } from "./cli/commands/runtime.js";
import { registerApprovalCommands, registerTaskCommands } from "./cli/commands/task.js";
import { registerStatusCommand } from "./cli/commands/status.js";
import { registerEventsCommand } from "./cli/commands/events.js";
import { registerWatchCommand } from "./cli/commands/watch.js";
import { registerDoctorCommand } from "./cli/commands/doctor.js";
import { registerSetupCommand } from "./cli/commands/setup.js";
import { registerDigestCommand, registerReportCommand } from "./cli/commands/report.js";
import { registerIssueCommands } from "./cli/commands/issue.js";
import { registerDashboardCommands } from "./cli/commands/dashboard.js";
import { registerMcpCommand } from "./cli/commands/mcp.js";
import { version } from "./version.js";

function shouldLaunchTUI(): boolean {
  const args = hideBin(process.argv);
  return process.stdout.isTTY === true && args.length === 0;
}

async function launchTUI(): Promise<void> {
  if (process.versions.bun) {
    const { launchTUI: launch } = await import("./tui/index.js");
    await launch(version);
    return;
  }
  // @opentui/core loads its native core through bun:ffi, so the interactive
  // TUI can only run under Bun. When the CLI was started with Node, re-exec
  // the same entrypoint under bun; if bun is not installed, fall back to a
  // clear message instead of a module-resolution crash.
  const child = spawn("bun", [fileURLToPath(import.meta.url)], { stdio: "inherit" });
  child.on("error", (error: NodeJS.ErrnoException) => {
    if (error.code !== "ENOENT") {
      throw error;
    }
    console.error("The interactive TUI requires Bun (https://bun.sh).");
    console.error("Install Bun, or use `openloop watch` for a live dashboard and `openloop --help` for commands.");
    process.exitCode = 1;
  });
  child.on("exit", (code, signal) => {
    process.exitCode = signal !== null ? 1 : (code ?? 0);
  });
}

async function main(): Promise<void> {
  if (shouldLaunchTUI()) {
    await launchTUI();
    return;
  }

  const cli = yargs(hideBin(process.argv))
    .scriptName("openloop")
    .version(version)
    .alias("version", "V")
    .strict()
    .demandCommand()
    .help();

  registerProjectCommands(cli);
  registerPromotionCommands(cli);
  registerConfigCommands(cli);
  registerDaemonCommands(cli);
  registerLogsCommands(cli);
  registerRunCommands(cli);
  registerRuntimeCommands(cli);
  registerTaskCommands(cli);
  registerApprovalCommands(cli);
  registerStatusCommand(cli);
  registerEventsCommand(cli);
  registerWatchCommand(cli);
  registerDoctorCommand(cli);
  registerReportCommand(cli);
  registerDigestCommand(cli);
  registerSetupCommand(cli);
  registerIssueCommands(cli);
  registerDashboardCommands(cli);
  registerMcpCommand(cli);

  await cli.parseAsync();
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(message);
  process.exitCode = 1;
});