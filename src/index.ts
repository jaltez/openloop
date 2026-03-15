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

async function main(): Promise<void> {
  const cli = yargs(hideBin(process.argv))
    .scriptName("openloop")
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

  await cli.parseAsync();
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(message);
  process.exitCode = 1;
});