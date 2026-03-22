import { createRequire } from "node:module";
import { hideBin } from "yargs/helpers";
import yargs from "yargs/yargs";
import { registerDaemonCommands } from "./cli/commands/service.js";
import { registerConfigCommands } from "./cli/commands/config.js";
import { registerLogsCommands } from "./cli/commands/logs.js";
import { registerProjectCommands } from "./cli/commands/project.js";
import { registerPromotionCommands } from "./cli/commands/promotion.js";
import { registerRunCommands } from "./cli/commands/run.js";
import { registerRuntimeCommands } from "./cli/commands/runtime.js";
import { registerTaskCommands } from "./cli/commands/task.js";
import { registerStatusCommand } from "./cli/commands/status.js";
import { registerEventsCommand } from "./cli/commands/events.js";
import { registerWatchCommand } from "./cli/commands/watch.js";
import { registerDoctorCommand } from "./cli/commands/doctor.js";
import { registerSetupCommand } from "./cli/commands/setup.js";
import { registerReportCommand } from "./cli/commands/report.js";
import { registerIssueCommands } from "./cli/commands/issue.js";
import { registerDashboardCommands } from "./cli/commands/dashboard.js";

const require = createRequire(import.meta.url);
const { version } = require("../package.json") as { version: string };

function shouldLaunchTUI(): boolean {
  const args = hideBin(process.argv);
  return process.stdout.isTTY === true && args.length === 0;
}

async function main(): Promise<void> {
  if (shouldLaunchTUI()) {
    const { launchTUI } = await import("./tui/index.js");
    await launchTUI(version);
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
  registerStatusCommand(cli);
  registerEventsCommand(cli);
  registerWatchCommand(cli);
  registerDoctorCommand(cli);
  registerSetupCommand(cli);
  registerReportCommand(cli);
  registerIssueCommands(cli);
  registerDashboardCommands(cli);

  await cli.parseAsync();
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(message);
  process.exitCode = 1;
});