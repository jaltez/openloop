import type { Argv, ArgumentsCamelCase } from "yargs";
import { getProject } from "../../core/project-registry.js";
import { assertPiOnPath, runPi } from "../../core/pi.js";
import { buildPrompt, determineWorkerRole, runProjectIteration, selectNextTask } from "../../core/scheduler.js";
import { loadTaskLedger } from "../../core/task-ledger.js";

type RunArgs = ArgumentsCamelCase<{
  project: string;
  model?: string;
  prompt: string;
  verbose?: boolean;
}>;

type RunOnceArgs = ArgumentsCamelCase<{
  project: string;
  model?: string;
  verbose?: boolean;
  dryRun?: boolean;
}>;

export function registerRunCommands(cli: Argv): void {
  cli.command(
    "run",
    "Run Pi against a linked project",
    (command: Argv) =>
      command
        .option("project", { type: "string", alias: "p", demandOption: true })
        .option("model", { type: "string" })
        .option("prompt", { type: "string", demandOption: true })
        .option("verbose", { type: "boolean", default: false, describe: "Print prompt to stderr before execution" }),
    async (args: RunArgs) => {
      assertPiOnPath();
      if (args.verbose) {
        process.stderr.write(`[prompt]\n${String(args.prompt)}\n[/prompt]\n`);
      }
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
    (command: Argv) =>
      command
        .option("project", { type: "string", alias: "p", demandOption: true })
        .option("model", { type: "string", describe: "Override the Pi model for this run" })
        .option("verbose", { type: "boolean", default: false, describe: "Print the Pi prompt to stderr before execution" })
        .option("dry-run", { type: "boolean", default: false, describe: "Select task and build prompt but do not invoke Pi" }),
    async (args: RunOnceArgs) => {
      const project = await getProject(String(args.project));

      if (args.dryRun) {
        const ledger = await loadTaskLedger(project.path);
        const selection = selectNextTask(ledger);
        if (!selection.task || selection.mode === "idle") {
          console.log(JSON.stringify({ mode: "idle", reason: selection.reason }, null, 2));
          return;
        }
        const role = determineWorkerRole(selection.task, selection.mode);
        const prompt = buildPrompt(selection.task, selection.mode, role);
        console.log(JSON.stringify({ mode: selection.mode, role, taskId: selection.task.id, reason: selection.reason, prompt }, null, 2));
        return;
      }

      assertPiOnPath();
      const result = await runProjectIteration(project, {
        modelOverride: args.model ? String(args.model) : undefined,
      });
      if (args.verbose && result.prompt) {
        process.stderr.write(`[prompt]\n${result.prompt}\n[/prompt]\n`);
      }
      console.log(JSON.stringify(result, null, 2));
    },
  );
}