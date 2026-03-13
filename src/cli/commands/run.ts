import type { Argv, ArgumentsCamelCase } from "yargs";
import { getProject } from "../../core/project-registry.js";
import { runPi } from "../../core/pi.js";
import { runProjectIteration } from "../../core/scheduler.js";

type RunArgs = ArgumentsCamelCase<{
  project: string;
  model?: string;
  prompt: string;
}>;

type RunOnceArgs = ArgumentsCamelCase<{
  project: string;
}>;

export function registerRunCommands(cli: Argv): void {
  cli.command(
    "run",
    "Run Pi against a linked project",
    (command: Argv) =>
      command
        .option("project", { type: "string", demandOption: true })
        .option("model", { type: "string" })
        .option("prompt", { type: "string", demandOption: true }),
    async (args: RunArgs) => {
      const project = await getProject(String(args.project));
      const code = await runPi({
        project,
        prompt: String(args.prompt),
        model: args.model ? String(args.model) : undefined,
      });
      process.exitCode = code;
    },
  );

  cli.command(
    "run-once",
    "Run one scheduler iteration for a linked project",
    (command: Argv) => command.option("project", { type: "string", demandOption: true }),
    async (args: RunOnceArgs) => {
      const project = await getProject(String(args.project));
      const result = await runProjectIteration(project);
      console.log(JSON.stringify(result, null, 2));
    },
  );
}